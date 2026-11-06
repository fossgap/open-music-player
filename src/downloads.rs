//! Background transfers and recordings. Only completed, validated audio is published.
mod http;
use crate::{
    model::{Kind, Track, YoutubeAccount},
    youtube_music,
};
use gst::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

const RUN: u8 = 0;
const CANCEL: u8 = 1;
const STOP: u8 = 2;
const OVERFLOW: u8 = 3;
const COMMIT: u8 = 4;
const DISCARD: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Download,
    Record,
    Playback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Queued,
    Starting,
    Running,
    Finishing,
    Finished,
    Cancelled,
    Failed(String),
}
impl Status {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Starting | Self::Running | Self::Finishing
        )
    }
}

#[derive(Clone, Default)]
pub struct Control(Arc<AtomicU8>);
impl Control {
    fn value(&self) -> u8 {
        self.0.load(Ordering::Acquire)
    }
    fn request(&self, value: u8) -> bool {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                (old == RUN
                    || (old == STOP && value == CANCEL)
                    || (value == DISCARD && old != COMMIT))
                    .then_some(value)
            })
            .is_ok()
    }
    pub fn finish(&self) {
        self.request(STOP);
    }
    fn interrupted(&self) -> bool {
        matches!(self.value(), CANCEL | OVERFLOW | DISCARD)
    }
}

/// A bounded, nonblocking PCM tap: slow storage must never stall playback.
#[derive(Clone)]
pub struct Capture {
    tx: mpsc::SyncSender<Vec<u8>>,
    control: Control,
}
impl Capture {
    pub fn write(&self, pcm: &[u8]) {
        if self.control.value() == RUN && self.tx.try_send(pcm.to_vec()).is_err() {
            self.control.request(OVERFLOW);
        }
    }
    pub fn finish(&self) {
        self.control.finish();
    }
}

pub struct Job {
    pub id: u64,
    pub track: Track,
    pub mode: Mode,
    pub status: Status,
    pub bytes: u64,
    pub total: Option<u64>,
    pub seconds: f64,
    pub path: Option<PathBuf>,
    pub running: bool,
    directory: PathBuf,
    partial: Arc<Mutex<Option<Partial>>>,
    control: Control,
}

impl Job {
    pub fn can_resume(&self) -> bool {
        self.mode == Mode::Download
            && !self.running
            && matches!(self.status, Status::Cancelled | Status::Failed(_))
    }
}

struct Partial {
    file: NamedTempFile,
    bytes: u64,
    total: Option<u64>,
    validator: Option<String>,
}

struct Task {
    id: u64,
    track: Track,
    mode: Mode,
    directory: PathBuf,
    account: Option<YoutubeAccount>,
    pcm: Option<mpsc::Receiver<Vec<u8>>>,
    control: Control,
    partial: Arc<Mutex<Option<Partial>>>,
}

enum Update {
    Status(Status),
    Progress(u64, Option<u64>, f64),
    Complete(Result<PathBuf, String>),
}
pub struct Completed {
    pub id: u64,
    pub track: Track,
    pub mode: Mode,
    pub path: PathBuf,
}

pub struct Manager {
    pub jobs: Vec<Job>,
    next: u64,
    pending: VecDeque<Task>,
    workers: Vec<(Mode, JoinHandle<()>)>,
    tx: mpsc::Sender<(u64, Update)>,
    rx: mpsc::Receiver<(u64, Update)>,
}
impl Default for Manager {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            jobs: Vec::new(),
            next: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
            pending: VecDeque::new(),
            workers: Vec::new(),
            tx,
            rx,
        }
    }
}
impl Manager {
    pub fn start(
        &mut self,
        track: Track,
        mode: Mode,
        directory: PathBuf,
        account: Option<YoutubeAccount>,
    ) -> Result<(u64, Option<Capture>), String> {
        if !matches!(
            track.kind,
            Kind::Podcast | Kind::Radio | Kind::Stream | Kind::YoutubeMusic | Kind::Spotify
        ) {
            return Err("Choose an online source to save.".into());
        }
        if (track.kind == Kind::Spotify) != (mode == Mode::Playback) {
            return Err("Spotify can be recorded during playback.".into());
        }
        if self
            .jobs
            .iter()
            .any(|j| j.track.id == track.id && j.status.active())
        {
            return Err("This source is already being saved.".into());
        }
        self.next += 1;
        let id = self.next;
        let control = Control::default();
        let partial = Arc::new(Mutex::new(None));
        let (capture, pcm) = if mode == Mode::Playback {
            let (tx, rx) = mpsc::sync_channel(64);
            (
                Some(Capture {
                    tx,
                    control: control.clone(),
                }),
                Some(rx),
            )
        } else {
            (None, None)
        };
        self.jobs.push(Job {
            id,
            track: track.clone(),
            mode,
            status: Status::Queued,
            bytes: 0,
            total: None,
            seconds: 0.0,
            path: None,
            running: false,
            directory: directory.clone(),
            partial: partial.clone(),
            control: control.clone(),
        });
        self.pending.push_back(Task {
            id,
            track,
            mode,
            directory,
            account,
            pcm,
            control,
            partial,
        });
        self.launch();
        Ok((id, capture))
    }

    fn launch(&mut self) {
        self.workers.retain(|(_, worker)| !worker.is_finished());
        // Playback capture must start immediately, even during network transfers.
        while let Some(index) = self.pending.iter().position(|task| {
            task.mode == Mode::Playback
                || self
                    .workers
                    .iter()
                    .filter(|(m, _)| *m != Mode::Playback)
                    .count()
                    < 3
        }) {
            let task = self.pending.remove(index).unwrap();
            if task.control.value() != RUN {
                continue;
            }
            let tx = self.tx.clone();
            if let Some(job) = self.jobs.iter_mut().find(|j| j.id == task.id) {
                job.running = true;
            }
            self.workers.push((
                task.mode,
                thread::spawn(move || {
                    let id = task.id;
                    let _ = tx.send((id, Update::Status(Status::Starting)));
                    let result = execute(task, &tx);
                    let _ = tx.send((id, Update::Complete(result)));
                }),
            ));
        }
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(job) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id && j.status.active())
        {
            if job.control.request(CANCEL) {
                job.status = Status::Cancelled;
            }
        }
        self.pending.retain(|t| t.id != id);
    }
    pub fn stop(&mut self, id: u64) {
        if self.pending.iter().any(|t| t.id == id) {
            self.cancel(id);
            return;
        }
        if let Some(job) = self
            .jobs
            .iter()
            .find(|j| j.id == id && j.mode != Mode::Download && j.status.active())
        {
            job.control.finish();
        }
    }
    pub fn resume(&mut self, id: u64, account: Option<YoutubeAccount>) -> Result<(), String> {
        let job = self
            .jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or("Download not found.")?;
        if !job.can_resume() {
            return Err("Wait for cancellation to finish before resuming.".into());
        }
        if self
            .jobs
            .iter()
            .any(|j| j.id != id && j.track.id == job.track.id && j.status.active())
        {
            return Err("This source is already being saved.".into());
        }
        let job = self.jobs.iter_mut().find(|j| j.id == id).unwrap();
        job.control = Control::default();
        job.status = Status::Queued;
        job.seconds = 0.0;
        {
            let partial = job.partial.lock().unwrap();
            job.bytes = partial.as_ref().map_or(0, |p| p.bytes);
            job.total = partial.as_ref().and_then(|p| p.total);
        }
        self.pending.push_back(Task {
            id,
            track: job.track.clone(),
            mode: job.mode,
            directory: job.directory.clone(),
            account,
            pcm: None,
            control: job.control.clone(),
            partial: job.partial.clone(),
        });
        self.launch();
        Ok(())
    }
    /// Cancel before detaching so a late worker cannot publish an orphan file.
    pub fn remove(&mut self, id: u64) -> Result<(), String> {
        let Some(index) = self.jobs.iter().position(|j| j.id == id) else {
            return Ok(());
        };
        let job = &self.jobs[index];
        if job.status.active() && !job.control.request(DISCARD) {
            return Err("The download is finishing. Try again in a moment.".into());
        }
        job.control.request(DISCARD);
        self.pending.retain(|t| t.id != id);
        let job = self.jobs.remove(index);
        // File cleanup can touch a network-mounted download directory.
        self.workers
            .push((Mode::Download, thread::spawn(move || drop(job))));
        Ok(())
    }
    pub fn poll(&mut self) -> Vec<Completed> {
        let mut complete = Vec::new();
        for (id, update) in self.rx.try_iter() {
            let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) else {
                continue;
            };
            match update {
                Update::Status(status) if job.status != Status::Cancelled => job.status = status,
                Update::Progress(bytes, total, seconds) => {
                    job.bytes = bytes;
                    job.total = total;
                    job.seconds = seconds;
                    if !matches!(job.status, Status::Cancelled | Status::Finishing) {
                        job.status = Status::Running;
                    }
                }
                Update::Complete(Ok(path)) => {
                    job.running = false;
                    job.path = Some(path.clone());
                    job.status = Status::Finished;
                    complete.push(Completed {
                        id,
                        track: job.track.clone(),
                        mode: job.mode,
                        path,
                    });
                }
                Update::Complete(Err(error)) => {
                    job.running = false;
                    job.status = if matches!(job.control.value(), CANCEL | DISCARD) {
                        Status::Cancelled
                    } else {
                        Status::Failed(error)
                    }
                }
                _ => {}
            }
        }
        self.launch();
        complete
    }
    pub fn shutdown(&mut self) {
        for job in &self.jobs {
            job.control.request(DISCARD);
        }
        self.pending.clear();
        for (_, worker) in self.workers.drain(..) {
            let _ = worker.join();
        }
        for job in &self.jobs {
            job.partial.lock().unwrap().take();
        }
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Progress<'a> {
    id: u64,
    tx: &'a mpsc::Sender<(u64, Update)>,
    last: Instant,
}
impl Progress<'_> {
    fn status(&self, status: Status) {
        let _ = self.tx.send((self.id, Update::Status(status)));
    }
    fn send(&mut self, bytes: u64, total: Option<u64>, seconds: f64) {
        if self.last.elapsed() >= Duration::from_millis(150) {
            self.last = Instant::now();
            let _ = self
                .tx
                .send((self.id, Update::Progress(bytes, total, seconds)));
        }
    }
}

fn execute(mut task: Task, tx: &mpsc::Sender<(u64, Update)>) -> Result<PathBuf, String> {
    if task.control.interrupted() {
        return Err("Cancelled".into());
    }
    std::fs::create_dir_all(&task.directory)
        .map_err(|e| format!("Could not open the download folder: {e}"))?;
    let mut progress = Progress {
        id: task.id,
        tx,
        last: Instant::now() - Duration::from_secs(1),
    };
    let (file, extension) = if let Some(pcm) = task.pcm.take() {
        (
            record(&task, None, Some(pcm), &mut progress)?,
            "flac".to_string(),
        )
    } else {
        let resolved = if task.track.kind == Kind::YoutubeMusic {
            youtube_music::resolve_cancellable(task.account.as_ref(), &task.track.uri, || {
                task.control.value() != RUN
            })?
        } else {
            crate::services::http_url(&task.track.uri)?;
            youtube_music::Resolved {
                uri: task.track.uri.clone(),
                headers: HashMap::new(),
            }
        };
        if task.mode == Mode::Record {
            (
                record(&task, Some(&resolved), None, &mut progress)?,
                "flac".to_string(),
            )
        } else {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            match runtime.block_on(http::download(&task, &resolved, &mut progress))? {
                Some(audio) => audio,
                None => (
                    record(&task, Some(&resolved), None, &mut progress)?,
                    "flac".to_string(),
                ),
            }
        }
    };
    progress.status(Status::Finishing);
    let mut command = std::process::Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(file.path());
    let audio = crate::process::output_with_input_cancellable(
        &mut command,
        &[],
        Duration::from_secs(12),
        4096,
        || task.control.interrupted(),
    )?;
    if !String::from_utf8_lossy(&audio).contains("audio") {
        return Err("The source did not return a playable audio file.".into());
    }
    file.as_file()
        .sync_all()
        .map_err(|e| format!("Could not save audio: {e}"))?;
    task.control
        .0
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
            matches!(old, RUN | STOP).then_some(COMMIT)
        })
        .map_err(|_| "Cancelled".to_string())?;
    publish(file, &task.directory, &task.track, task.mode, &extension)
}

async fn interrupted(control: &Control) {
    while !control.interrupted() {
        tokio::time::sleep(Duration::from_millis(75)).await;
    }
}

fn extension<'a>(mime: &str, path: &'a str, bytes: &[u8]) -> &'a str {
    if bytes.starts_with(b"fLaC") {
        return "flac";
    }
    if bytes.starts_with(b"OggS") {
        return "ogg";
    }
    if bytes.starts_with(b"RIFF") {
        return "wav";
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        return "m4a";
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return "webm";
    }
    if bytes.starts_with(b"ID3") || mime == "audio/mpeg" {
        return "mp3";
    }
    match mime {
        "audio/mp4" | "video/mp4" | "audio/x-m4a" => "m4a",
        "audio/aac" | "audio/aacp" => "aac",
        "audio/ogg" | "application/ogg" => "ogg",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/webm" => "webm",
        _ => path
            .rsplit('.')
            .next()
            .filter(|s| ["mp3", "m4a", "aac", "ogg", "opus", "flac", "wav", "webm"].contains(s))
            .unwrap_or("mka"),
    }
}

struct Pipeline(gst::Element);
impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.0.set_state(gst::State::Null);
    }
}

fn record(
    task: &Task,
    resolved: Option<&youtube_music::Resolved>,
    pcm: Option<mpsc::Receiver<Vec<u8>>>,
    progress: &mut Progress<'_>,
) -> Result<NamedTempFile, String> {
    gst::init().map_err(|e| e.to_string())?;
    let file = tempfile::Builder::new()
        .prefix(".open-audio-")
        .suffix(".part")
        .tempfile_in(&task.directory)
        .map_err(|e| e.to_string())?;
    let caps = gst::Caps::builder("audio/x-raw")
        .field("format", "F32LE")
        .field("rate", 44100i32)
        .field("channels", 2i32)
        .field("layout", "interleaved")
        .build();
    // Pace decoded radio audio before encoding. Separating the input and writer
    // lets Stop discard network read-ahead and finalize exactly the captured PCM.
    let input = if let Some(resolved) = resolved {
        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|e| e.to_string())?;
        let sink = gst_app::AppSink::builder()
            .caps(&caps)
            .max_buffers(4)
            .sync(task.mode == Mode::Record)
            .build();
        pipeline.set_property_from_str("flags", "audio+buffering");
        pipeline.set_property("uri", &resolved.uri);
        pipeline.set_property("audio-sink", &sink);
        let headers = resolved.headers.clone();
        pipeline.connect("source-setup", false, move |values| {
            let source = values[1].get::<gst::Element>().ok()?;
            let mut extra = gst::Structure::new_empty("headers");
            for (name, value) in &headers {
                if name.eq_ignore_ascii_case("user-agent")
                    && source.find_property("user-agent").is_some()
                {
                    source.set_property("user-agent", value);
                } else {
                    extra.set(name, value);
                }
            }
            if source.find_property("extra-headers").is_some() {
                source.set_property("extra-headers", &extra);
            }
            if source
                .find_property("timeout")
                .is_some_and(|p| p.value_type() == u32::static_type())
            {
                source.set_property("timeout", 5u32);
            }
            None
        });
        let pipeline = Pipeline(pipeline);
        pipeline
            .0
            .set_state(gst::State::Playing)
            .map_err(|_| "Could not open the recording source.")?;
        Some((pipeline, sink))
    } else {
        None
    };
    let pipeline = gst::parse_launch("appsrc name=pcm format=time block=false max-bytes=1048576 ! audioconvert ! audioresample ! flacenc ! filesink name=save sync=false")
        .map_err(|_| "Install GStreamer FLAC recording support.")?.downcast::<gst::Pipeline>().map_err(|_| "Could not initialize recording.")?;
    let save = pipeline.by_name("save").unwrap();
    save.set_property(
        "location",
        file.path()
            .to_str()
            .ok_or("Choose a download folder with a UTF-8 path.")?,
    );
    let source = pipeline
        .by_name("pcm")
        .unwrap()
        .downcast::<gst_app::AppSrc>()
        .unwrap();
    source.set_caps(Some(&caps));
    let pipeline = Pipeline(pipeline.upcast());
    let bus = pipeline.0.bus().ok_or("Could not initialize recording.")?;
    pipeline
        .0
        .set_state(gst::State::Playing)
        .map_err(|_| "Could not start recording.")?;
    progress.status(Status::Running);
    let mut frames = 0u64;
    let mut stopping = None;
    let mut last_data = Instant::now();
    loop {
        match task.control.value() {
            CANCEL | DISCARD => return Err("Cancelled".into()),
            OVERFLOW => {
                return Err(
                    "Recording could not keep up with playback. Choose a faster download folder."
                        .into(),
                );
            }
            _ => {}
        }
        if stopping.is_some_and(|t: Instant| t.elapsed() > Duration::from_secs(6)) {
            return Err("The recording could not be finalized. Try another source.".into());
        }
        // Backpressure finite downloads instead of letting decoded PCM accumulate.
        // Playback's separate bounded tap still fails promptly if disk falls behind.
        let writer_full = source.current_level_bytes() >= 1_048_576;
        if stopping.is_none() && !writer_full {
            let bytes = if let Some(pcm) = &pcm {
                // Drain every frame accepted before Stop, then send EOS.
                match pcm.recv_timeout(Duration::from_millis(25)) {
                    Ok(bytes) => Some(bytes),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        task.control.finish();
                        None
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                }
            } else if task.control.value() == STOP {
                None
            } else if let Some((decoder, sink)) = &input {
                if decoder
                    .0
                    .bus()
                    .unwrap()
                    .iter()
                    .any(|m| matches!(m.view(), gst::MessageView::Error(_)))
                {
                    return Err(
                        "Recording failed. Check the source, audio plugins and free space.".into(),
                    );
                }
                let sample = sink.try_pull_sample(gst::ClockTime::from_mseconds(50));
                if sample.is_none() && sink.is_eos() {
                    task.control.finish();
                }
                sample.and_then(|s| {
                    s.buffer()
                        .and_then(|b| b.map_readable().ok().map(|m| m.as_slice().to_vec()))
                })
            } else {
                None
            };
            if let Some(bytes) = bytes {
                let count = bytes.len() as u64 / 8;
                let mut buffer = gst::Buffer::from_mut_slice(bytes);
                let b = buffer.get_mut().unwrap();
                b.set_pts(gst::ClockTime::from_nseconds(
                    frames * 1_000_000_000 / 44100,
                ));
                b.set_duration(gst::ClockTime::from_nseconds(count * 1_000_000_000 / 44100));
                frames += count;
                source
                    .push_buffer(buffer)
                    .map_err(|_| "Could not write the recording.")?;
                last_data = Instant::now();
            } else if task.control.value() == STOP {
                source
                    .end_of_stream()
                    .map_err(|_| "Could not finalize the recording.")?;
                stopping = Some(Instant::now());
                progress.status(Status::Finishing);
            }
        }
        for message in bus.iter() {
            match message.view() {
                gst::MessageView::Eos(_) => {
                    drop(pipeline);
                    if frames == 0 {
                        return Err("No audio was recorded.".into());
                    }
                    return Ok(file);
                }
                gst::MessageView::Error(_) => {
                    return Err(
                        "Recording failed. Check the source, audio plugins and free space.".into(),
                    );
                }
                _ => {}
            }
        }
        let bytes = file.as_file().metadata().map(|m| m.len()).unwrap_or(0);
        progress.send(bytes, None, frames as f64 / 44100.0);
        if (input.is_some() || writer_full)
            && stopping.is_none()
            && last_data.elapsed() > Duration::from_secs(30)
        {
            return Err("The source did not send audio. Try again.".into());
        }
        if stopping.is_some() || writer_full {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn publish(
    mut file: NamedTempFile,
    directory: &Path,
    track: &Track,
    mode: Mode,
    extension: &str,
) -> Result<PathBuf, String> {
    let label = if track.artist.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", track.artist, track.title)
    };
    let mut label: String = label
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    let mut end = label.len().min(180);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    label.truncate(end);
    let label = label.trim().trim_matches('.').trim();
    let label = if label.is_empty() { "Audio" } else { label };
    let label = if mode == Mode::Download {
        label.to_string()
    } else {
        format!("{label} - recording")
    };
    for index in 0..10_000 {
        let name = if index == 0 {
            format!("{label}.{extension}")
        } else {
            format!("{label} ({index}).{extension}")
        };
        let path = directory.join(name);
        match file.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                file = error.file
            }
            Err(error) => return Err(format!("Could not save audio: {}", error.error)),
        }
    }
    Err("Too many files with this name. Choose another download folder.".into())
}

#[cfg(test)]
pub(crate) mod tests;
