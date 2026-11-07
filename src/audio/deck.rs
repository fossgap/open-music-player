use super::{Event, NextTrack, gain::Gain, visualization};
use crate::model::ReplayGain;
use gst::prelude::*;
use gst_controller::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Queue {
    next: VecDeque<NextTrack>,
    committed: VecDeque<NextTrack>,
    announced: usize,
    started: bool,
}

/// One playbin with native gapless handoff, independent gain and fade envelopes.
pub(super) struct Deck {
    pub pipeline: gst::Element,
    pub(super) output: gst::Element,
    bus: gst::Bus,
    waveform: gst_app::AppSink,
    headers: Arc<Mutex<HashMap<String, String>>>,
    queue: Arc<Mutex<Queue>>,
    pub current: Option<NextTrack>,
    pending_seek: Option<f64>,
    initial_seek: bool,
    deferred_next: Option<NextTrack>,
    seek_target: Option<f64>,
    pub ready: bool,
    pub buffering: bool,
    pub ended: bool,
    rate: f64,
    gain: Gain,
    fade: gst::Element,
    envelope: gst_controller::InterpolationControlSource,
}

impl Deck {
    pub fn details(&self) -> crate::audio_info::Details {
        let index = self.pipeline.property::<i32>("current-audio");
        if index < 0 {
            return Default::default();
        }
        let mut details = crate::audio_info::Details::default();
        if let Some(tags) = self
            .pipeline
            .emit_by_name::<Option<gst::TagList>>("get-audio-tags", &[&index])
        {
            details.codec = tags
                .get::<gst::tags::AudioCodec>()
                .map(|t| t.get().to_string());
            details.bitrate = tags
                .get::<gst::tags::Bitrate>()
                .map(|t| u64::from(t.get()))
                .or_else(|| {
                    tags.get::<gst::tags::NominalBitrate>()
                        .map(|t| u64::from(t.get()))
                })
                .filter(|n| *n > 0);
        }
        if let Some(pad) = self
            .pipeline
            .emit_by_name::<Option<gst::Pad>>("get-audio-pad", &[&index])
        {
            details.sample_rate = pad.current_caps().and_then(|caps| {
                caps.structure(0)?
                    .get::<i32>("rate")
                    .ok()
                    .filter(|n| *n > 0)
                    .map(|n| n as u32)
            });
        }
        details
    }

    pub fn new() -> Result<Self, String> {
        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|_| "Install the GStreamer playback plugins")?;
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
        let queue = Arc::new(Mutex::new(Queue::default()));
        let next_queue = queue.clone();
        let next_headers = headers.clone();
        pipeline.connect("about-to-finish", false, move |values| {
            let pipeline = values[0].get::<gst::Element>().ok()?;
            let mut queue = next_queue.lock().ok()?;
            let next = queue.next.pop_front()?;
            *next_headers.lock().ok()? = next.headers.clone();
            queue.committed.push_back(next.clone());
            pipeline.set_property("uri", &next.uri);
            None
        });
        // Bound both queues so visualization and decoding cannot run far ahead of audio.
        let filter = gst::parse_bin_from_description(
            &format!(
                "audioconvert ! scaletempo ! volume name=normalization ! tee name=t \
             t. ! queue max-size-time=100000000 ! identity name=out \
             t. ! queue leaky=downstream max-size-buffers=2 ! audioconvert ! \
             audio/x-raw,format=F32LE,channels=1 ! {} ! appsink name=wave \
             max-buffers=1 drop=true sync=false async=false",
                visualization::filter()
            ),
            true,
        )
        .map_err(|e| e.to_string())?;
        let waveform = filter
            .by_name("wave")
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap();
        let gain = Gain::new(filter.by_name("normalization").unwrap());
        let fade = gst::ElementFactory::make("volume")
            .build()
            .map_err(|e| e.to_string())?;
        let envelope = gst_controller::InterpolationControlSource::new();
        envelope.set_mode(gst_controller::InterpolationMode::Linear);
        let binding =
            gst_controller::DirectControlBinding::new_absolute(&fade, "volume", &envelope);
        fade.add_control_binding(&binding)
            .map_err(|e| e.to_string())?;
        envelope.set(gst::ClockTime::ZERO, 1.0);
        pipeline.set_property("audio-filter", &filter);
        let output = if cfg!(test) || std::env::var_os("OPEN_AUDIO_FAKE_SINK").is_some() {
            gst::ElementFactory::make("fakesink")
                .property("sync", true)
                .property("signal-handoffs", cfg!(test))
                .build()
                .map_err(|e| e.to_string())?
        } else {
            gst::ElementFactory::make("autoaudiosink")
                .build()
                .map_err(|e| e.to_string())?
        };
        let audible_queue = queue.clone();
        let weak_pipeline = pipeline.downgrade();
        output.static_pad("sink").unwrap().add_probe(
            gst::PadProbeType::EVENT_DOWNSTREAM,
            move |_, info| {
                if info
                    .event()
                    .is_some_and(|e| matches!(e.view(), gst::EventView::StreamStart(_)))
                {
                    let mut queue = audible_queue.lock().unwrap();
                    if !queue.started {
                        queue.started = true;
                    } else if queue.announced < queue.committed.len() {
                        queue.announced += 1;
                        if let Some(pipeline) = weak_pipeline.upgrade() {
                            // Observe the audible branch directly. The visualization sink can
                            // run ahead and prevent playbin from aggregating stream-start messages.
                            let _ = pipeline.post_message(
                                gst::message::Application::builder(gst::Structure::new_empty(
                                    "oap-started",
                                ))
                                .src(&pipeline)
                                .build(),
                            );
                        }
                    }
                }
                gst::PadProbeReturn::Ok
            },
        );
        // Place the fade after playbin's queues. Otherwise decoded buffers could
        // already have their full volume baked in when the fade is scheduled.
        let sink = gst::Bin::new();
        sink.add_many([&fade, &output]).map_err(|e| e.to_string())?;
        fade.link(&output).map_err(|e| e.to_string())?;
        let pad = gst::GhostPad::with_target(&fade.static_pad("sink").unwrap())
            .map_err(|e| e.to_string())?;
        sink.add_pad(&pad).map_err(|e| e.to_string())?;
        pipeline.set_property("audio-sink", &sink);
        let bus = pipeline
            .bus()
            .ok_or("GStreamer did not create a message bus")?;
        Ok(Self {
            pipeline,
            output,
            bus,
            waveform,
            headers,
            queue,
            current: None,
            pending_seek: None,
            initial_seek: false,
            deferred_next: None,
            seek_target: None,
            ready: false,
            buffering: false,
            ended: false,
            rate: 1.0,
            gain,
            fade,
            envelope,
        })
    }

    pub fn load(
        &mut self,
        track: NextTrack,
        position: f64,
        playing: bool,
        next: Option<NextTrack>,
        muted: bool,
    ) -> Result<(), String> {
        self.stop();
        *self.headers.lock().unwrap() = track.headers.clone();
        if position > 0.0 || self.rate != 1.0 {
            self.deferred_next = next;
        } else {
            self.queue.lock().unwrap().next =
                next.as_ref().map_or_else(VecDeque::new, NextTrack::chain);
        }
        self.pipeline.set_property("uri", &track.uri);
        self.current = Some(track);
        self.pending_seek = (position > 0.0 || self.rate != 1.0).then_some(position);
        self.initial_seek = self.pending_seek.is_some();
        if muted {
            self.fade(0.0, 1.0, 0.0, 0.0);
        }
        let result = self
            .pipeline
            .set_state(if playing && !self.initial_seek {
                gst::State::Playing
            } else {
                gst::State::Paused
            })
            .map_err(|e| e.to_string())?;
        if result == gst::StateChangeSuccess::NoPreroll {
            // Truly live inputs cannot preroll or accept the saved rate seek.
            self.pending_seek = None;
            self.initial_seek = false;
            self.rate = 1.0;
            self.ready = true;
            self.set_playing(playing)?;
        }
        Ok(())
    }

    pub fn set_next(&mut self, next: Option<NextTrack>, playing: bool) -> Result<(), String> {
        if self.pending_seek.is_some() {
            self.deferred_next = next;
            return Ok(());
        }
        let restart = {
            let mut queue = self.queue.lock().unwrap();
            let mut proposed = next.as_ref().map_or_else(VecDeque::new, NextTrack::chain);
            let mut changed = false;
            for committed in &queue.committed {
                if !proposed
                    .pop_front()
                    .is_some_and(|n| n.same_source(committed))
                {
                    changed = true;
                    break;
                }
            }
            if !changed {
                queue.next = proposed;
            }
            changed
        };
        // A queue edit can arrive after about-to-finish has committed its URI.
        // Reload at the current position to cancel it instead of playing a removed track.
        if restart {
            if let Some(current) = self.current.clone() {
                self.load(current, self.position(), playing, next, false)?;
            }
        }
        Ok(())
    }

    pub fn set_playing(&self, playing: bool) -> Result<(), String> {
        self.pipeline
            .set_state(if playing && !self.buffering && !self.initial_seek {
                gst::State::Playing
            } else {
                gst::State::Paused
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn gain(&self, mode: ReplayGain) {
        self.gain.mode(mode);
    }
    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn can_change_rate(&self) -> bool {
        let mut query = gst::query::Seeking::new(gst::Format::Time);
        self.pipeline.query(&mut query) && query.result().0
    }

    pub fn set_rate(&mut self, rate: f64, playing: bool) -> Result<(), String> {
        if self.rate == rate {
            return Ok(());
        }
        if self.current.is_none() || !self.ready {
            self.rate = rate;
            if self.current.is_some() {
                self.pending_seek = Some(self.position());
                self.initial_seek = true;
            }
            return Ok(());
        }
        if !self.can_change_rate() {
            return Err("Playback speed is unavailable for this source.".into());
        }
        let old = self.rate;
        self.rate = rate;
        if let Err(error) = self.seek(self.position(), playing) {
            self.rate = old;
            return Err(error);
        }
        Ok(())
    }

    fn rate_seek(&self, seconds: f64) -> Result<(), gst::glib::BoolError> {
        self.pipeline.seek(
            self.rate,
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::SeekType::Set,
            clock_time(seconds),
            gst::SeekType::None,
            gst::ClockTime::NONE,
        )
    }
    pub fn volume(&self, value: f64) {
        self.pipeline.set_property("volume", value.clamp(0.0, 1.0));
    }
    pub fn fade(&self, start: f64, seconds: f64, from: f64, to: f64) {
        self.envelope.unset_all();
        self.envelope.set(gst::ClockTime::ZERO, from);
        self.envelope.set(clock_time(start), from);
        self.envelope
            .set(clock_time(start + seconds.max(0.001)), to);
        self.fade.set_property("volume", from);
    }
    pub fn clear_fade(&self) {
        self.envelope.unset_all();
        self.envelope.set(gst::ClockTime::ZERO, 1.0);
        self.fade.set_property("volume", 1.0f64);
    }
    pub fn position(&self) -> f64 {
        self.pending_seek.or(self.seek_target).unwrap_or_else(|| {
            self.output
                .query_position::<gst::ClockTime>()
                .map_or(0.0, |t| t.nseconds() as f64 / 1e9)
        })
    }
    pub fn duration(&self) -> f64 {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map_or(0.0, |t| t.nseconds() as f64 / 1e9)
    }
    pub fn seek(&mut self, seconds: f64, playing: bool) -> Result<(), String> {
        if !self.queue.lock().unwrap().committed.is_empty() {
            if let Some(current) = self.current.clone() {
                return self.load(current, seconds, playing, None, false);
            }
        }
        self.rate_seek(seconds)
            .map_err(|_| "This stream cannot seek".to_string())?;
        self.seek_target = Some(seconds);
        Ok(())
    }

    pub fn stop(&mut self) {
        // Do not hold the queue lock during set_state: the streaming callback may need it.
        *self.queue.lock().unwrap() = Queue::default();
        let _ = self.pipeline.set_state(gst::State::Null);
        while self.bus.pop().is_some() {}
        self.current = None;
        self.pending_seek = None;
        self.initial_seek = false;
        self.deferred_next = None;
        self.seek_target = None;
        self.ready = false;
        self.buffering = false;
        self.ended = false;
        self.clear_fade();
    }

    pub fn poll(&mut self, playing: bool) -> Vec<Event> {
        let mut events = Vec::new();
        let mut spectrum = None;
        for message in self.bus.iter().take(100).collect::<Vec<_>>() {
            if let Some(levels) = visualization::from_message(&message) {
                spectrum = Some(levels);
            }
            match message.view() {
                gst::MessageView::Application(m)
                    if m.structure().is_some_and(|s| s.name() == "oap-started") =>
                {
                    let next = {
                        let mut queue = self.queue.lock().unwrap();
                        queue.announced = queue.announced.saturating_sub(1);
                        queue.committed.pop_front()
                    };
                    if let Some(next) = next {
                        self.seek_target = None;
                        self.current = Some(next.clone());
                        self.ended = false;
                        events.push(Event::Advanced(next.id));
                        events.push(Event::Ready);
                    }
                }
                gst::MessageView::Eos(_) => {
                    self.ended = true;
                    events.push(Event::End);
                }
                gst::MessageView::Error(e) => events.push(Event::Error(e.error().to_string())),
                gst::MessageView::AsyncDone(_) => {
                    self.seek_target = None;
                    if let Some(position) = self.pending_seek.take() {
                        self.queue.lock().unwrap().next = self
                            .deferred_next
                            .take()
                            .as_ref()
                            .map_or_else(VecDeque::new, NextTrack::chain);
                        if self.can_change_rate() && self.rate_seek(position).is_ok() {
                            self.seek_target = Some(position);
                            continue;
                        }
                        // Live/unseekable sources continue at their natural speed.
                        self.rate = 1.0;
                    }
                    self.ready = true;
                    if self.initial_seek {
                        self.initial_seek = false;
                        let _ = self.set_playing(playing);
                    }
                    events.push(Event::Ready);
                }
                gst::MessageView::Buffering(b) => {
                    self.buffering = b.percent() < 100;
                    let _ = self.set_playing(playing);
                    events.push(Event::Buffering(b.percent()));
                }
                _ => {}
            }
        }
        if playing && !self.buffering {
            if let Some(levels) = spectrum {
                events.push(Event::Spectrum(levels));
            }
        }
        if let Some(sample) = self.waveform.try_pull_sample(gst::ClockTime::ZERO) {
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

impl Drop for Deck {
    fn drop(&mut self) {
        self.stop();
    }
}

fn clock_time(seconds: f64) -> gst::ClockTime {
    gst::ClockTime::from_nseconds((seconds.max(0.0) * 1e9) as u64)
}
