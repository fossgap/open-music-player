use super::*;
use std::{io::Read, net::TcpListener, sync::atomic::AtomicBool};

pub(crate) struct Server {
    pub uri: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
#[derive(Clone, Copy, PartialEq)]
enum Ranges {
    Ignore,
    Honor,
    Changed,
    Invalid,
    Unverified,
}
impl Server {
    pub fn new(bytes: Vec<u8>, mime: &str, known_length: bool, delay: Duration) -> Self {
        Self::routes(
            HashMap::from([("/audio".into(), (bytes, mime.into()))]),
            known_length,
            delay,
        )
    }
    fn routes(
        routes: HashMap<String, (Vec<u8>, String)>,
        known_length: bool,
        delay: Duration,
    ) -> Self {
        Self::serve(routes, known_length, delay, Ranges::Ignore)
    }
    fn serve(
        routes: HashMap<String, (Vec<u8>, String)>,
        known_length: bool,
        delay: Duration,
        ranges: Ranges,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let uri = format!("http://{}/audio", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let worker = thread::spawn(move || {
            let mut changed = false;
            while !done.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = [0; 4096];
                let count = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..count]);
                received.lock().unwrap().push(request.to_string());
                let route = request.split_whitespace().nth(1).unwrap_or("");
                let Some((bytes, mime)) = routes.get(route) else {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let range = request.lines().find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .and_then(|v| v.trim().strip_suffix('-'))
                        .and_then(|v| v.parse::<usize>().ok())
                });
                if range.is_some() && ranges == Ranges::Changed {
                    changed = true;
                }
                let changed_bytes: Vec<_> = if changed {
                    bytes
                        .iter()
                        .enumerate()
                        .map(|(i, b)| if i >= 44 { b ^ 0x11 } else { *b })
                        .collect()
                } else {
                    Vec::new()
                };
                let bytes = if changed { &changed_bytes } else { bytes };
                let offset = if ranges == Ranges::Ignore {
                    0
                } else {
                    range.unwrap_or(0)
                };
                let status = if range.is_some() && ranges != Ranges::Ignore {
                    "206 Partial Content"
                } else {
                    "200 OK"
                };
                let range_header = if status.starts_with("206") {
                    format!(
                        "Content-Range: bytes {}-{}/{}\r\n",
                        offset + if ranges == Ranges::Invalid { 4 } else { 0 },
                        bytes.len() - 1,
                        bytes.len()
                    )
                } else {
                    String::new()
                };
                let body = &bytes[offset..];
                let length = if known_length {
                    format!("Content-Length: {}\r\n", body.len())
                } else {
                    String::new()
                };
                let etag = if ranges == Ranges::Unverified {
                    "W/\"fixture-v1\""
                } else if changed {
                    "\"fixture-v2\""
                } else {
                    "\"fixture-v1\""
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\n{length}{range_header}ETag: {etag}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
                );
                for chunk in body.chunks(8192) {
                    if done.load(Ordering::Relaxed) || stream.write_all(chunk).is_err() {
                        break;
                    }
                    thread::sleep(delay);
                }
            }
        });
        Self {
            uri,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

#[test]
fn resume_uses_verified_ranges_and_restarts_changed_or_uncooperative_sources() {
    for ranges in [
        Ranges::Honor,
        Ranges::Ignore,
        Ranges::Changed,
        Ranges::Invalid,
        Ranges::Unverified,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let bytes = wav(3);
        let server = Server::serve(
            HashMap::from([("/audio".into(), (bytes.clone(), "audio/wav".into()))]),
            true,
            Duration::from_millis(20),
            ranges,
        );
        let mut manager = Manager::default();
        let (id, _) = manager
            .start(
                Track::new(&server.uri, "Resume me", Kind::Podcast),
                Mode::Download,
                dir.path().into(),
                None,
            )
            .unwrap();
        wait(&mut manager, |m| m.jobs[0].bytes > 0);
        manager.cancel(id);
        assert!(
            manager.resume(id, None).is_err(),
            "Do not race a worker still cancelling"
        );
        wait(&mut manager, |m| m.jobs[0].can_resume());
        let kept = manager.jobs[0].bytes;
        assert!(kept > 0 && kept < bytes.len() as u64);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        manager.resume(id, None).unwrap();
        assert_eq!(manager.jobs[0].bytes, kept);
        let completed = wait(&mut manager, |m| !m.jobs[0].status.active());
        assert_eq!(completed.len(), 1, "{:?}", manager.jobs[0].status);
        assert_eq!(completed[0].id, id);
        assert_eq!(manager.jobs.len(), 1, "Resume updates the existing row");
        let expected: Vec<_> = bytes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if ranges == Ranges::Changed && i >= 44 {
                    b ^ 0x11
                } else {
                    *b
                }
            })
            .collect();
        assert_eq!(std::fs::read(&completed[0].path).unwrap(), expected);
        let requests = server.requests.lock().unwrap();
        if ranges == Ranges::Unverified {
            assert!(
                requests
                    .iter()
                    .all(|r| !r.to_ascii_lowercase().contains("\r\nrange:"))
            );
        } else {
            assert!(requests.iter().any(|r| {
                r.to_ascii_lowercase()
                    .contains(&format!("range: bytes={kept}-"))
            }));
            assert!(
                requests
                    .iter()
                    .any(|r| r.to_ascii_lowercase().contains("if-range: \"fixture-v1\""))
            );
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn deleting_active_or_cancelled_downloads_discards_partial_files() {
    for cancel_first in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(wav(10), "audio/wav", true, Duration::from_millis(30));
        let mut manager = Manager::default();
        let (id, _) = manager
            .start(
                Track::new(&server.uri, "Delete me", Kind::Podcast),
                Mode::Download,
                dir.path().into(),
                None,
            )
            .unwrap();
        wait(&mut manager, |m| m.jobs[0].bytes > 0);
        if cancel_first {
            manager.cancel(id);
            wait(&mut manager, |m| m.jobs[0].can_resume());
        }
        manager.remove(id).unwrap();
        manager.shutdown();
        assert!(manager.poll().is_empty());
        assert!(manager.jobs.is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn adaptive_stream_downloads_produce_complete_playable_audio() {
    for format in ["hls", "dash"] {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let manifest = source.path().join(if format == "hls" {
            "index.m3u8"
        } else {
            "index.mpd"
        });
        let mut command = std::process::Command::new("ffmpeg");
        command.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6",
            "-c:a",
            "aac",
            "-f",
            format,
        ]);
        if format == "hls" {
            command.args(["-hls_time", "2", "-hls_playlist_type", "vod"]);
        } else {
            command.args(["-seg_duration", "2"]);
        }
        assert!(command.arg(&manifest).status().unwrap().success());
        let mut routes = HashMap::new();
        for entry in std::fs::read_dir(source.path()).unwrap() {
            let path = entry.unwrap().path();
            let key = if path == manifest {
                "/audio".into()
            } else {
                format!("/{}", path.file_name().unwrap().to_str().unwrap())
            };
            let mime = if path == manifest {
                if format == "hls" {
                    "application/vnd.apple.mpegurl"
                } else {
                    "application/dash+xml"
                }
            } else if format == "hls" {
                "video/mp2t"
            } else {
                "audio/mp4"
            };
            routes.insert(key, (std::fs::read(path).unwrap(), mime.into()));
        }
        let server = Server::routes(routes, true, Duration::ZERO);
        let mut manager = Manager::default();
        manager
            .start(
                Track::new(&server.uri, format, Kind::Stream),
                Mode::Download,
                destination.path().into(),
                None,
            )
            .unwrap();
        let completed = wait(&mut manager, |m| !m.jobs[0].status.active());
        assert_eq!(completed.len(), 1, "{format}: {:?}", manager.jobs[0].status);
        let output = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
            ])
            .arg(&completed[0].path)
            .output()
            .unwrap();
        let duration: f64 = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!((duration - 6.0).abs() < 0.2, "{format}: {duration}s");
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

pub(crate) fn wav(seconds: u32) -> Vec<u8> {
    let frames = 44100 * seconds;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + frames * 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(44100u32.to_le_bytes());
    bytes.extend(88200u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((frames * 2).to_le_bytes());
    for i in 0..frames {
        bytes.extend(
            (((f64::from(i) * 440.0 * std::f64::consts::TAU / 44100.0).sin() * 10000.0) as i16)
                .to_le_bytes(),
        );
    }
    bytes
}

fn wait(manager: &mut Manager, condition: impl Fn(&Manager) -> bool) -> Vec<Completed> {
    let start = Instant::now();
    let mut completed = Vec::new();
    loop {
        completed.extend(manager.poll());
        if condition(manager) {
            return completed;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "Jobs: {:?}",
            manager.jobs.iter().map(|j| &j.status).collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn downloads_preserve_bytes_and_handle_unknown_lengths_without_overwrites() {
    for known in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let bytes = wav(1);
        let server = Server::new(bytes.clone(), "audio/wav", known, Duration::from_millis(25));
        let track = Track::new(&server.uri, "../An episode", Kind::Podcast);
        let mut manager = Manager::default();
        for n in 0..2 {
            let (id, _) = manager
                .start(track.clone(), Mode::Download, dir.path().into(), None)
                .unwrap();
            wait(&mut manager, |m| {
                m.jobs.iter().any(|j| j.id == id && j.bytes > 0)
            });
            let job = manager.jobs.last().unwrap();
            assert_eq!(job.total, known.then_some(bytes.len() as u64));
            let complete = wait(&mut manager, |m| !m.jobs.last().unwrap().status.active());
            assert_eq!(complete.len(), 1);
            assert_eq!(std::fs::read(&complete[0].path).unwrap(), bytes);
            assert_eq!(complete[0].path.parent(), Some(dir.path()));
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), n + 1);
        }
    }
}

#[test]
fn cancellation_removes_partial_files_and_rejects_error_documents() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(wav(30), "audio/wav", true, Duration::from_millis(75));
    let mut manager = Manager::default();
    let (id, _) = manager
        .start(
            Track::new(&server.uri, "Slow", Kind::Podcast),
            Mode::Download,
            dir.path().into(),
            None,
        )
        .unwrap();
    wait(&mut manager, |m| m.jobs[0].bytes > 0);
    let start = Instant::now();
    manager.cancel(id);
    manager.shutdown();
    manager.poll();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(manager.jobs[0].status, Status::Cancelled);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    for mime in ["text/html", "audio/mpeg"] {
        let server = Server::new(
            b"<html>Access denied</html>".to_vec(),
            mime,
            true,
            Duration::ZERO,
        );
        manager
            .start(
                Track::new(&server.uri, "Error", Kind::Podcast),
                Mode::Download,
                dir.path().into(),
                None,
            )
            .unwrap();
        wait(&mut manager, |m| !m.jobs.last().unwrap().status.active());
        assert!(matches!(
            manager.jobs.last().unwrap().status,
            Status::Failed(_)
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn live_recording_stop_finalizes_audio_and_cancel_discards_it() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(wav(20), "audio/wav", true, Duration::ZERO);
    let mut manager = Manager::default();
    for cancel in [false, true] {
        let (id, _) = manager
            .start(
                Track::new(&server.uri, "Live radio", Kind::Radio),
                Mode::Record,
                dir.path().into(),
                None,
            )
            .unwrap();
        wait(&mut manager, |m| m.jobs.last().unwrap().seconds > 0.5);
        if cancel {
            manager.cancel(id);
            manager.shutdown();
            manager.poll();
        } else {
            manager.stop(id);
            let completed = wait(&mut manager, |m| !m.jobs.last().unwrap().status.active());
            assert_eq!(
                completed.len(),
                1,
                "{:?}",
                manager.jobs.last().unwrap().status
            );
            assert_eq!(&std::fs::read(&completed[0].path).unwrap()[..4], b"fLaC");
            let output = std::process::Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&completed[0].path)
                .output()
                .unwrap();
            let duration: f64 = String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!((0.4..5.0).contains(&duration), "recorded {duration}s");
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn pcm_recording_drains_buffered_audio_and_bounds_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = Manager::default();
    let track = Track::new("spotify:track:fixture", &"🎵".repeat(100), Kind::Spotify);
    let (id, capture) = manager
        .start(track, Mode::Playback, dir.path().into(), None)
        .unwrap();
    let capture = capture.unwrap();
    let samples: Vec<u8> = (0..44100 * 2)
        .flat_map(|i| {
            ((f64::from(i / 2) * 440.0 * std::f64::consts::TAU / 44100.0).sin() as f32)
                .to_le_bytes()
        })
        .collect();
    for bytes in samples.chunks(8192) {
        capture.write(bytes);
    }
    manager.stop(id);
    let complete = wait(&mut manager, |m| !m.jobs[0].status.active());
    assert_eq!(complete.len(), 1);
    let output = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&complete[0].path)
        .args(["-f", "f32le", "-"])
        .output()
        .unwrap();
    assert_eq!(
        output.stdout.len(),
        samples.len(),
        "Stop must drain every accepted PCM frame"
    );
    let (tx, _rx) = mpsc::sync_channel(1);
    let control = Control::default();
    let capture = Capture {
        tx,
        control: control.clone(),
    };
    capture.write(&samples[..64]);
    capture.write(&samples[..64]);
    assert_eq!(control.value(), OVERFLOW);
}
