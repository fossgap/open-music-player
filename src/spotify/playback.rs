//! librespot decodes; the application's GStreamer version owns the audio device.
use crate::{
    audio::{Event, NextTrack},
    model::{PlaybackSettings, ReplayGain},
};
use gst::prelude::*;
use librespot_core::{session::Session, spotify_uri::SpotifyUri};
use librespot_playback::{
    audio_backend::{Sink, SinkError, SinkResult},
    config::{NormalisationMethod, NormalisationType, PlayerConfig},
    convert::Converter,
    decoder::AudioPacket,
    mixer::NoOpVolume,
    player::{Player, PlayerEvent, PlayerEventChannel},
};
use std::{
    cell::Cell,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct PcmSink {
    pipeline: gst::Pipeline,
    source: gst_app::AppSrc,
    waveform: Arc<Mutex<Vec<f64>>>,
    frames: u64,
}
impl PcmSink {
    fn new() -> Result<(Self, gst::Element), String> {
        let output = if std::env::var_os("OPEN_AUDIO_FAKE_SINK").is_some() {
            "fakesink sync=true"
        } else {
            "autoaudiosink"
        };
        Self::with_output(output)
    }
    fn with_output(output: &str) -> Result<(Self, gst::Element), String> {
        let pipeline = gst::parse_launch(&format!("appsrc name=pcm format=time block=true max-bytes=16384 ! audioconvert ! audioresample ! volume name=volume ! {output}"))
            .map_err(|e| e.to_string())?.downcast::<gst::Pipeline>().map_err(|_| "Could not create Spotify audio output.")?;
        let source = pipeline
            .by_name("pcm")
            .unwrap()
            .downcast::<gst_app::AppSrc>()
            .unwrap();
        source.set_caps(Some(
            &gst::Caps::builder("audio/x-raw")
                .field("format", "F32LE")
                .field("rate", 44100i32)
                .field("channels", 2i32)
                .field("layout", "interleaved")
                .build(),
        ));
        let volume = pipeline.by_name("volume").unwrap();
        Ok((
            Self {
                pipeline,
                source,
                waveform: Arc::new(Mutex::new(Vec::new())),
                frames: 0,
            },
            volume,
        ))
    }
}
impl Sink for PcmSink {
    fn start(&mut self) -> SinkResult<()> {
        self.frames = 0;
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| SinkError::StateChange(e.to_string()))?;
        Ok(())
    }
    fn stop(&mut self) -> SinkResult<()> {
        self.pipeline
            .set_state(gst::State::Ready)
            .map_err(|e| SinkError::StateChange(e.to_string()))?;
        Ok(())
    }
    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(samples) = packet else {
            return Err(SinkError::InvalidParams(
                "Expected decoded Spotify PCM.".into(),
            ));
        };
        let frames = samples.len() as u64 / 2;
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|v| (*v as f32).to_le_bytes())
            .collect();
        if let Ok(mut wave) = self.waveform.lock() {
            *wave = samples
                .chunks_exact(2)
                .step_by((samples.len() / 360).max(1))
                .take(180)
                .map(|s| ((s[0] + s[1]) * 2.0).clamp(-1.0, 1.0))
                .collect();
        }
        let mut buffer = gst::Buffer::from_mut_slice(data);
        let b = buffer.get_mut().unwrap();
        b.set_pts(gst::ClockTime::from_nseconds(
            self.frames * 1_000_000_000 / 44100,
        ));
        b.set_duration(gst::ClockTime::from_nseconds(
            frames * 1_000_000_000 / 44100,
        ));
        self.frames += frames;
        self.source
            .push_buffer(buffer)
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        Ok(())
    }
}
impl Drop for PcmSink {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

pub struct Playback {
    player: Arc<Player>,
    events: tokio::sync::mpsc::UnboundedReceiver<Feedback>,
    monitor: tokio::task::JoinHandle<()>,
    transition: Arc<Mutex<Transition>>,
    session: Session,
    runtime: tokio::runtime::Handle,
    settings: PlaybackSettings,
    applied: PlaybackSettings,
    bus: gst::Bus,
    volume: gst::Element,
    waveform: Arc<Mutex<Vec<f64>>>,
    track: Option<SpotifyUri>,
    request: Option<u64>,
    anchor: Cell<(f64, Instant)>,
    running: bool,
    pub playing: bool,
    duration: f64,
    started: Instant,
    ready: bool,
}
impl Playback {
    /// Called while the service runtime is entered; librespot keeps it for its I/O.
    pub fn new(session: Session) -> Result<Self, String> {
        Self::with_settings(session, PlaybackSettings::default())
    }
    fn with_settings(session: Session, settings: PlaybackSettings) -> Result<Self, String> {
        let (sink, volume) = PcmSink::new()?;
        let waveform = sink.waveform.clone();
        let bus = sink.pipeline.bus().unwrap();
        let config = PlayerConfig {
            position_update_interval: Some(Duration::from_secs(1)),
            gapless: settings.gapless,
            normalisation: settings.replay_gain != ReplayGain::Off,
            normalisation_type: if settings.replay_gain == ReplayGain::Album {
                NormalisationType::Album
            } else {
                NormalisationType::Track
            },
            normalisation_method: NormalisationMethod::Basic,
            ..PlayerConfig::default()
        };
        let player = Player::new(config, session.clone(), Box::new(NoOpVolume), move || {
            Box::new(sink)
        });
        let transition = Arc::new(Mutex::new(Transition::default()));
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let monitor = tokio::spawn(forward_events(
            player.clone(),
            player.get_player_event_channel(),
            transition.clone(),
            tx,
        ));
        Ok(Self {
            player,
            events,
            monitor,
            transition,
            session,
            runtime: tokio::runtime::Handle::current(),
            settings,
            applied: settings,
            bus,
            volume,
            waveform,
            track: None,
            request: None,
            anchor: Cell::new((0.0, Instant::now())),
            running: false,
            playing: false,
            duration: 0.0,
            started: Instant::now(),
            ready: false,
        })
    }
    pub fn configure(&mut self, settings: PlaybackSettings) {
        self.settings = settings;
        if !self.can_chain() {
            self.transition.lock().unwrap().next = None;
        }
    }
    fn can_chain(&self) -> bool {
        self.settings.gapless
            && self.applied.gapless
            && self.settings.replay_gain == self.applied.replay_gain
    }
    pub fn prepare_next(&mut self, next: Option<NextTrack>) {
        let next = next.filter(|n| self.can_chain() && super::track_id(&n.uri).is_some());
        let mut transition = self.transition.lock().unwrap();
        if transition.next == next {
            return;
        }
        if let Some(next) = next.as_ref() {
            if let Ok(uri) = SpotifyUri::from_uri(&next.uri) {
                self.player.preload(uri);
            }
        }
        transition.next = next;
    }
    pub fn load(
        &mut self,
        uri: &str,
        position: f64,
        duration: f64,
        autoplay: bool,
    ) -> Result<(), String> {
        if super::track_id(uri).is_none() {
            return Err("Invalid Spotify track.".into());
        }
        let track = SpotifyUri::from_uri(uri).map_err(|_| "Invalid Spotify track.")?;
        if self.settings.replay_gain != self.applied.replay_gain
            || self.settings.gapless != self.applied.gapless
        {
            let runtime = self.runtime.clone();
            let _entered = runtime.enter();
            let replacement = Self::with_settings(self.session.clone(), self.settings)?;
            replacement.volume(self.volume.property("volume"));
            *self = replacement;
        }
        self.stop();
        while self.events.try_recv().is_ok() {}
        while self.bus.pop().is_some() {}
        self.track = Some(track.clone());
        self.request = None;
        self.ready = false;
        self.started = Instant::now();
        self.duration = duration;
        self.anchor.set((position, Instant::now()));
        self.playing = autoplay;
        {
            let mut transition = self.transition.lock().unwrap();
            transition.current = Some(track.clone());
            transition.playing = autoplay;
        }
        self.player.load(track, autoplay, millis(position));
        Ok(())
    }
    pub fn set_playing(&mut self, playing: bool) {
        self.transition.lock().unwrap().playing = playing;
        self.anchor.set((self.position(), Instant::now()));
        if !playing {
            self.running = false;
        }
        self.playing = playing;
        if playing {
            self.player.play();
        } else {
            self.player.pause();
        }
    }
    pub fn stop(&mut self) {
        *self.transition.lock().unwrap() = Transition::default();
        self.player.stop();
        self.track = None;
        self.request = None;
        self.playing = false;
        self.running = false;
        self.anchor.set((0.0, Instant::now()));
    }
    pub fn volume(&self, value: f64) {
        self.volume.set_property("volume", value.clamp(0.0, 1.0));
    }
    pub fn duration(&self) -> f64 {
        self.duration
    }
    pub fn position(&self) -> f64 {
        let (value, time) = self.anchor.get();
        let value = value
            + if self.running && self.playing {
                time.elapsed().as_secs_f64()
            } else {
                0.0
            };
        if self.duration > 0.0 {
            value.min(self.duration)
        } else {
            value
        }
    }
    pub fn seek(&self, seconds: f64) {
        self.anchor.set((seconds, Instant::now()));
        self.player.seek(millis(seconds));
    }
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        for _ in 0..200 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            let event = match event {
                Feedback::Advanced(next) => {
                    self.track = SpotifyUri::from_uri(&next.uri).ok();
                    self.request = None;
                    self.ready = false;
                    self.running = false;
                    self.started = Instant::now();
                    self.duration = next.duration;
                    self.anchor.set((0.0, Instant::now()));
                    out.push(Event::Advanced(next.id));
                    continue;
                }
                Feedback::Native(event) => event,
            };
            match event {
                PlayerEvent::PlayRequestIdChanged { play_request_id } if self.track.is_some() => {
                    self.request = Some(play_request_id);
                }
                PlayerEvent::Loading {
                    play_request_id,
                    track_id,
                    ..
                } if self.track.as_ref() == Some(&track_id) => {
                    self.request = Some(play_request_id);
                }
                PlayerEvent::Playing {
                    play_request_id,
                    track_id,
                    position_ms,
                } if self.accepts(play_request_id, &track_id) => {
                    self.anchor
                        .set((f64::from(position_ms) / 1000.0, Instant::now()));
                    self.running = true;
                    if !self.playing {
                        self.player.pause();
                    }
                    if !self.ready {
                        self.ready = true;
                        out.push(Event::Ready);
                    }
                }
                PlayerEvent::Paused {
                    play_request_id,
                    track_id,
                    position_ms,
                } if self.accepts(play_request_id, &track_id) => {
                    self.anchor
                        .set((f64::from(position_ms) / 1000.0, Instant::now()));
                    self.running = false;
                    if !self.ready {
                        self.ready = true;
                        out.push(Event::Ready);
                    }
                }
                PlayerEvent::PositionChanged {
                    play_request_id,
                    track_id,
                    position_ms,
                }
                | PlayerEvent::PositionCorrection {
                    play_request_id,
                    track_id,
                    position_ms,
                }
                | PlayerEvent::Seeked {
                    play_request_id,
                    track_id,
                    position_ms,
                } if self.accepts(play_request_id, &track_id) => self
                    .anchor
                    .set((f64::from(position_ms) / 1000.0, Instant::now())),
                PlayerEvent::EndOfTrack {
                    play_request_id,
                    track_id,
                } if self.accepts(play_request_id, &track_id) => {
                    self.playing = false;
                    self.running = false;
                    self.track = None;
                    out.push(Event::End);
                }
                PlayerEvent::Unavailable {
                    play_request_id,
                    track_id,
                } if self.accepts(play_request_id, &track_id) => {
                    self.stop();
                    out.push(Event::Error("Spotify could not play this track. Check your Premium account and connection, or choose another track.".into()));
                }
                _ => {}
            }
        }
        if self.track.is_some() && !self.ready && self.started.elapsed() > Duration::from_secs(45) {
            self.stop();
            out.push(Event::Error(
                "Spotify playback timed out. Reconnect your account and try again.".into(),
            ));
        }
        for message in self.bus.iter().take(50) {
            if let gst::MessageView::Error(error) = message.view() {
                out.push(Event::Error(error.error().to_string()));
            }
        }
        if out.iter().any(|e| matches!(e, Event::Error(_))) {
            self.stop();
        }
        if self.playing && self.running {
            if let Ok(mut wave) = self.waveform.lock() {
                if !wave.is_empty() {
                    out.push(Event::Waveform(std::mem::take(&mut *wave)));
                }
            }
        }
        out
    }
    fn accepts(&self, request: u64, track: &SpotifyUri) -> bool {
        self.request == Some(request) && self.track.as_ref() == Some(track)
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.monitor.abort();
        self.player.stop();
    }
}

enum Feedback {
    Native(PlayerEvent),
    Advanced(NextTrack),
}

#[derive(Default)]
struct Transition {
    playing: bool,
    current: Option<SpotifyUri>,
    request: Option<u64>,
    next: Option<NextTrack>,
}

impl Transition {
    fn accept(&mut self, event: &PlayerEvent) -> Option<NextTrack> {
        match event {
            PlayerEvent::PlayRequestIdChanged { play_request_id } if self.current.is_some() => {
                self.request = Some(*play_request_id)
            }
            PlayerEvent::Loading {
                play_request_id,
                track_id,
                ..
            } if self.current.as_ref() == Some(track_id) => self.request = Some(*play_request_id),
            PlayerEvent::EndOfTrack {
                play_request_id,
                track_id,
            } if self.current.as_ref() == Some(track_id)
                && self.request == Some(*play_request_id) =>
            {
                let mut next = self.next.take()?;
                let mut following = std::mem::take(&mut next.following).into_iter();
                self.next = following.next().map(|mut track| {
                    track.following = following.collect();
                    track
                });
                self.current = SpotifyUri::from_uri(&next.uri).ok();
                self.request = None;
                return Some(next);
            }
            _ => {}
        }
        None
    }
}

// Advance on the service runtime, not the GTK poll timer. Keep the PCM sink running
// so queued samples and the preloaded decoder join without reopening the device.
async fn forward_events(
    player: Arc<Player>,
    mut events: PlayerEventChannel,
    transition: Arc<Mutex<Transition>>,
    tx: tokio::sync::mpsc::UnboundedSender<Feedback>,
) {
    while let Some(event) = events.recv().await {
        let mut state = transition.lock().unwrap();
        if let Some(next) = state.accept(&event) {
            if let Ok(uri) = SpotifyUri::from_uri(&next.uri) {
                if tx.send(Feedback::Advanced(next)).is_err() {
                    break;
                }
                player.load(uri, state.playing, 0);
                if let Some(next) = state.next.as_ref() {
                    if let Ok(uri) = SpotifyUri::from_uri(&next.uri) {
                        player.preload(uri);
                    }
                }
            }
        } else if tx.send(Feedback::Native(event)).is_err() {
            break;
        }
    }
}
fn millis(seconds: f64) -> u32 {
    (seconds.max(0.0) * 1000.0).min(u32::MAX as f64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gapless_successor_accepts_preloaded_requests_without_loading_event() {
        let current = SpotifyUri::from_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").unwrap();
        let next = NextTrack {
            id: "next".into(),
            uri: "spotify:track:6rqhFgbbKwnb9MLmUQDhG6".into(),
            headers: Default::default(),
            duration: 120.0,
            following: Vec::new(),
        };
        let mut state = Transition {
            current: Some(current.clone()),
            request: Some(8),
            next: Some(next.clone()),
            playing: true,
        };
        assert!(
            state
                .accept(&PlayerEvent::EndOfTrack {
                    play_request_id: 7,
                    track_id: current.clone()
                })
                .is_none()
        );
        assert_eq!(
            state
                .accept(&PlayerEvent::EndOfTrack {
                    play_request_id: 8,
                    track_id: current
                })
                .unwrap(),
            next
        );
        assert!(state.next.is_none());
        assert!(state.request.is_none());
        // A ready preload skips Loading, but still emits PlayRequestIdChanged.
        state.accept(&PlayerEvent::PlayRequestIdChanged { play_request_id: 9 });
        assert_eq!(state.request, Some(9));
        assert_eq!(
            state.current,
            Some(SpotifyUri::from_uri(&next.uri).unwrap())
        );
        state.next = Some(next);
        state = Transition::default();
        assert!(
            state
                .accept(&PlayerEvent::EndOfTrack {
                    play_request_id: 9,
                    track_id: SpotifyUri::from_uri("spotify:track:6rqhFgbbKwnb9MLmUQDhG6").unwrap()
                })
                .is_none(),
            "Stop invalidates every prepared successor"
        );
    }
    #[test]
    fn pcm_output_can_pause_restart_and_produces_real_waveform() {
        gst::init().unwrap();
        let (mut sink, volume) = PcmSink::with_output("fakesink sync=true").unwrap();
        let mut converter = Converter::new(None);
        sink.start().unwrap();
        sink.write(AudioPacket::Samples(vec![0.1; 8820]), &mut converter)
            .unwrap();
        let (_, state, _) = sink.pipeline.state(gst::ClockTime::from_seconds(2));
        assert_eq!(state, gst::State::Playing);
        assert_eq!(sink.frames, 4410);
        assert!(
            sink.waveform
                .lock()
                .unwrap()
                .iter()
                .all(|s| (*s - 0.4).abs() < 0.001)
        );
        volume.set_property("volume", 0.25f64);
        assert_eq!(volume.property::<f64>("volume"), 0.25);
        sink.stop().unwrap();
        sink.start().unwrap();
        assert_eq!(sink.frames, 0);
        sink.write(AudioPacket::Samples(vec![0.0; 882]), &mut converter)
            .unwrap();
        sink.stop().unwrap();
        assert!(
            !sink
                .pipeline
                .bus()
                .unwrap()
                .iter()
                .any(|m| matches!(m.view(), gst::MessageView::Error(_)))
        );
    }
    #[test]
    fn obsolete_track_events_cannot_advance_or_restart_the_queue() {
        gst::init().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let mut playback = Playback::new(Session::new(Default::default(), None)).unwrap();
        let track = SpotifyUri::from_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").unwrap();
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        playback.events = events;
        playback.track = Some(track.clone());
        playback.duration = 180.0;
        tx.send(Feedback::Native(PlayerEvent::EndOfTrack {
            play_request_id: 1,
            track_id: track.clone(),
        }))
        .unwrap();
        assert!(playback.poll().is_empty());
        tx.send(Feedback::Native(PlayerEvent::Loading {
            play_request_id: 2,
            track_id: track.clone(),
            position_ms: 0,
        }))
        .unwrap();
        tx.send(Feedback::Native(PlayerEvent::Playing {
            play_request_id: 2,
            track_id: track.clone(),
            position_ms: 12000,
        }))
        .unwrap();
        assert!(playback.poll().iter().any(|e| matches!(e, Event::Ready)));
        assert!(
            !playback.playing,
            "A paused load must stay paused even if an earlier Playing event arrives"
        );
        assert_eq!(playback.position(), 12.0);
        tx.send(Feedback::Native(PlayerEvent::EndOfTrack {
            play_request_id: 1,
            track_id: track.clone(),
        }))
        .unwrap();
        assert!(playback.poll().is_empty());
        playback.stop();
        tx.send(Feedback::Native(PlayerEvent::Playing {
            play_request_id: 2,
            track_id: track.clone(),
            position_ms: 15000,
        }))
        .unwrap();
        tx.send(Feedback::Native(PlayerEvent::EndOfTrack {
            play_request_id: 2,
            track_id: track,
        }))
        .unwrap();
        assert!(playback.poll().is_empty());
        assert!(!playback.playing);
    }
}
