use gst::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub enum Event {
    End,
    Error(String),
    Buffering(i32),
    Waveform(Vec<f64>),
    Ready,
}

pub struct Audio {
    pipeline: gst::Element,
    bus: gst::Bus,
    pub playing: bool,
    pub loaded: bool,
    pending_seek: Option<f64>,
    buffering: bool,
    waveform: Option<gst_app::AppSink>,
    seek_target: std::cell::Cell<Option<f64>>,
    headers: Arc<Mutex<HashMap<String, String>>>,
    spotify: Option<crate::spotify::playback::Playback>,
    spotify_active: bool,
}

impl Audio {
    pub fn new() -> Result<Self, String> {
        gst::init().map_err(|e| e.to_string())?;
        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|_| "Install the GStreamer playback plugins")?;
        // Disable video and subtitles, including for video URLs with audio tracks.
        pipeline.set_property_from_str("flags", "audio+soft-volume+buffering");
        let headers = Arc::new(Mutex::new(HashMap::<String, String>::new()));
        let source_headers = headers.clone();
        pipeline.connect("source-setup", false, move |values| {
            let source = values[1].get::<gst::Element>().ok()?;
            let headers = source_headers.lock().ok()?;
            let mut extra = gst::Structure::new_empty("headers");
            for (name, value) in headers.iter() {
                if name.eq_ignore_ascii_case("user-agent") {
                    if source.find_property("user-agent").is_some() {
                        source.set_property("user-agent", value);
                    }
                } else {
                    extra.set(name, value);
                }
            }
            if source.find_property("extra-headers").is_some() {
                source.set_property("extra-headers", &extra);
            }
            None
        });
        // This bounded, leaky visualization branch cannot stall audible playback.
        let waveform = gst::parse_bin_from_description(
            "audioconvert ! tee name=t t. ! queue ! identity name=out \
             t. ! queue leaky=downstream max-size-buffers=2 ! audioconvert ! \
             audio/x-raw,format=F32LE,channels=1 ! appsink name=wave \
             max-buffers=1 drop=true sync=false async=false",
            true,
        )
        .ok()
        .and_then(|filter| {
            let sink = filter
                .by_name("wave")?
                .downcast::<gst_app::AppSink>()
                .ok()?;
            pipeline.set_property("audio-filter", &filter);
            Some(sink)
        });
        if std::env::var_os("OPEN_AUDIO_FAKE_SINK").is_some() {
            let sink = gst::ElementFactory::make("fakesink")
                .property("sync", true)
                .build()
                .map_err(|e| e.to_string())?;
            pipeline.set_property("audio-sink", sink);
        }
        let bus = pipeline
            .bus()
            .ok_or("GStreamer did not create a message bus")?;
        Ok(Self {
            pipeline,
            bus,
            playing: false,
            loaded: false,
            pending_seek: None,
            buffering: false,
            waveform,
            seek_target: std::cell::Cell::new(None),
            headers,
            spotify: None,
            spotify_active: false,
        })
    }

    pub fn spotify_ready(&self) -> bool {
        self.spotify.is_some()
    }
    pub fn attach_spotify(&mut self, playback: crate::spotify::playback::Playback) {
        if self.spotify_active {
            self.stop();
        }
        self.spotify = Some(playback);
    }
    pub fn detach_spotify(&mut self) {
        if self.spotify_active {
            self.stop();
        }
        self.spotify.take();
    }
    pub fn load_spotify(
        &mut self,
        uri: &str,
        position: f64,
        duration: f64,
        autoplay: bool,
    ) -> Result<(), String> {
        self.stop();
        self.spotify
            .as_mut()
            .ok_or("Connect your Spotify account first.")?
            .load(uri, position, duration, autoplay)?;
        self.spotify_active = true;
        self.loaded = true;
        self.playing = autoplay;
        Ok(())
    }

    pub fn load(&mut self, uri: &str, position: f64, autoplay: bool) -> Result<(), String> {
        self.load_with_headers(uri, position, autoplay, HashMap::new())
    }

    pub fn load_with_headers(
        &mut self,
        uri: &str,
        position: f64,
        autoplay: bool,
        headers: HashMap<String, String>,
    ) -> Result<(), String> {
        self.stop();
        *self
            .headers
            .lock()
            .map_err(|_| "Could not prepare media request")? = headers;
        while self.bus.pop().is_some() {}
        self.pipeline.set_property("uri", uri);
        self.pending_seek = (position > 0.0).then_some(position);
        self.loaded = true;
        self.playing = autoplay;
        self.pipeline
            .set_state(if autoplay {
                gst::State::Playing
            } else {
                gst::State::Paused
            })
            .map_err(|e| {
                self.playing = false;
                self.loaded = false;
                e.to_string()
            })?;
        Ok(())
    }

    pub fn set_playing(&mut self, playing: bool) -> Result<(), String> {
        if !self.loaded {
            return Ok(());
        }
        if self.spotify_active {
            self.spotify.as_mut().unwrap().set_playing(playing);
            self.playing = playing;
            return Ok(());
        }
        self.pipeline
            .set_state(if playing && !self.buffering {
                gst::State::Playing
            } else {
                gst::State::Paused
            })
            .map_err(|e| e.to_string())?;
        self.playing = playing;
        Ok(())
    }

    pub fn stop(&mut self) {
        if self.spotify_active {
            if let Some(spotify) = self.spotify.as_mut() {
                spotify.stop();
            }
        }
        self.spotify_active = false;
        let _ = self.pipeline.set_state(gst::State::Null);
        self.playing = false;
        self.loaded = false;
        self.pending_seek = None;
        self.buffering = false;
        self.seek_target.set(None);
    }
    pub fn volume(&self, volume: f64) {
        if let Some(spotify) = self.spotify.as_ref() {
            spotify.volume(volume);
        }
        self.pipeline.set_property("volume", volume.clamp(0.0, 1.0));
    }
    pub fn position(&self) -> f64 {
        if self.spotify_active {
            return self.spotify.as_ref().unwrap().position();
        }
        if let Some(target) = self.pending_seek.or(self.seek_target.get()) {
            return target;
        }
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1e9)
            .unwrap_or(0.0)
    }
    pub fn duration(&self) -> f64 {
        if self.spotify_active {
            return self.spotify.as_ref().unwrap().duration();
        }
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1e9)
            .unwrap_or(0.0)
    }
    pub fn seek(&self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid seek position".into());
        }
        if self.spotify_active {
            self.spotify.as_ref().unwrap().seek(seconds);
            return Ok(());
        }
        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                gst::ClockTime::from_nseconds((seconds * 1e9) as u64),
            )
            .map_err(|_| "This stream cannot seek".to_string())?;
        self.seek_target.set(Some(seconds));
        Ok(())
    }

    pub fn poll(&mut self) -> Vec<Event> {
        if self.spotify_active {
            let spotify = self.spotify.as_mut().unwrap();
            let events = spotify.poll();
            self.playing = spotify.playing;
            if events.iter().any(|e| matches!(e, Event::Error(_))) {
                self.loaded = false;
            }
            return events;
        }
        let mut events = Vec::new();
        for message in self.bus.iter().take(100) {
            match message.view() {
                gst::MessageView::Eos(..) => {
                    self.playing = false;
                    events.push(Event::End);
                }
                gst::MessageView::Error(error) => {
                    self.playing = false;
                    let _ = self.pipeline.set_state(gst::State::Null);
                    self.loaded = false;
                    events.push(Event::Error(error.error().to_string()));
                }
                gst::MessageView::AsyncDone(..) => {
                    self.seek_target.set(None);
                    if let Some(position) = self.pending_seek.take() {
                        let _ = self.seek(position);
                    }
                    events.push(Event::Ready);
                }
                gst::MessageView::Buffering(b) => {
                    self.buffering = b.percent() < 100;
                    if self.playing {
                        let _ = self.pipeline.set_state(if self.buffering {
                            gst::State::Paused
                        } else {
                            gst::State::Playing
                        });
                    }
                    events.push(Event::Buffering(b.percent()));
                }
                _ => {}
            }
        }
        if let Some(sample) = self
            .waveform
            .as_ref()
            .and_then(|sink| sink.try_pull_sample(gst::ClockTime::ZERO))
        {
            if let Some(buffer) = sample.buffer() {
                if let Ok(map) = buffer.map_readable() {
                    let samples: Vec<f64> = map
                        .as_slice()
                        .chunks_exact(4)
                        .take(2048)
                        .map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
                        .filter(|s| s.is_finite())
                        .collect();
                    if !samples.is_empty() {
                        let step = (samples.len() / 180).max(1);
                        events.push(Event::Waveform(
                            samples
                                .into_iter()
                                .step_by(step)
                                .map(|s| (s * 4.0).clamp(-1.0, 1.0))
                                .collect(),
                        ));
                    }
                }
            }
        }
        events
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
    }
}
