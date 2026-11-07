//! Frequency analysis stays in GStreamer's streaming thread, away from GTK.
use gst::prelude::*;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const BANDS: usize = 32;
const FLOOR_DB: f64 = -72.0;

pub struct Sample<T> {
    pub values: T,
    pub at: Instant,
}

#[derive(Default)]
pub struct Frame {
    pub waveform: Option<Sample<Vec<f64>>>,
    pub spectrum: Option<Sample<Vec<f64>>>,
}

#[derive(Default)]
struct Latest {
    enabled: bool,
    waveform: Option<Sample<gst::Sample>>,
    spectrum: Option<Sample<Vec<f64>>>,
}

/// A bounded, latest-only mailbox, independent of the application's control-event poll.
/// Audio always uses a separate branch; dropping analysis buffers cannot mute playback.
#[derive(Clone)]
pub struct Tap(Arc<Mutex<Latest>>);

pub fn branch() -> String {
    format!(
        "queue leaky=downstream max-size-buffers=2 ! identity name=visualization_gate ! \
         audioconvert ! audio/x-raw,format=F32LE,channels=1 ! {} ! \
         appsink name=wave max-buffers=1 drop=true sync=false async=false",
        filter()
    )
}

impl Tap {
    pub fn new(bin: &gst::Bin, bus: &gst::Bus) -> Self {
        let latest = Arc::new(Mutex::new(Latest::default()));
        let gate = latest.clone();
        bin.by_name("visualization_gate")
            .unwrap()
            .static_pad("sink")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                if gate.lock().unwrap().enabled {
                    gst::PadProbeReturn::Ok
                } else {
                    // Forward events (including EOS/flush/caps), but avoid conversion and FFT.
                    gst::PadProbeReturn::Drop
                }
            });
        let wave = latest.clone();
        bin.by_name("wave")
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap()
            .set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let mut latest = wave.lock().unwrap();
                        if latest.enabled {
                            latest.waveform = Some(Sample {
                                values: sample,
                                at: Instant::now(),
                            });
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
        let spectrum = latest.clone();
        bus.set_sync_handler(move |_, message| {
            if message.structure().is_some_and(|s| s.name() == "spectrum") {
                let mut latest = spectrum.lock().unwrap();
                if latest.enabled {
                    latest.spectrum = from_message(message).map(|values| Sample {
                        values,
                        at: Instant::now(),
                    });
                }
                gst::BusSyncReply::Drop
            } else {
                // Playback errors, buffering, and gapless handoffs retain their normal route.
                gst::BusSyncReply::Pass
            }
        });
        Self(latest)
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut latest = self.0.lock().unwrap();
        if latest.enabled != enabled {
            *latest = Latest {
                enabled,
                ..Default::default()
            };
        }
    }

    pub fn clear(&self) {
        let mut latest = self.0.lock().unwrap();
        latest.waveform = None;
        latest.spectrum = None;
    }

    pub fn take(&self) -> Frame {
        let (waveform, spectrum) = {
            let mut latest = self.0.lock().unwrap();
            (latest.waveform.take(), latest.spectrum.take())
        };
        let waveform = waveform
            .filter(|s| s.at.elapsed() < Duration::from_millis(250))
            .and_then(|sample| {
                let map = sample.values.buffer()?.map_readable().ok()?;
                let count = (map.len() / 4).min(2048);
                let step = (count / 180).max(1);
                let values: Vec<_> = map
                    .as_slice()
                    .chunks_exact(4)
                    .take(count)
                    .step_by(step)
                    .map(|b| f64::from(f32::from_le_bytes(b.try_into().unwrap())))
                    .filter(|s| s.is_finite())
                    .map(|s| (s * 4.0).clamp(-1.0, 1.0))
                    .collect();
                (!values.is_empty()).then_some(Sample {
                    values,
                    at: sample.at,
                })
            });
        Frame {
            waveform,
            spectrum: spectrum.filter(|s| s.at.elapsed() < Duration::from_millis(250)),
        }
    }

    #[cfg(test)]
    pub fn is_enabled(&self) -> bool {
        self.0.lock().unwrap().enabled
    }
}

pub fn supported() -> bool {
    gst::ElementFactory::find("spectrum").is_some()
}

pub fn filter() -> &'static str {
    if supported() {
        "spectrum bands=512 interval=50000000 threshold=-72 post-messages=true message-magnitude=true message-phase=false"
    } else {
        // Missing optional visualization support must not prevent audio playback.
        "identity"
    }
}

pub fn from_message(message: &gst::MessageRef) -> Option<Vec<f64>> {
    let gst::MessageView::Element(message) = message.view() else {
        return None;
    };
    let structure = message.structure().filter(|s| s.name() == "spectrum")?;
    let magnitudes = structure.get::<gst::List>("magnitude").ok()?;
    let element = message.src()?.downcast_ref::<gst::Element>()?;
    let caps = element.static_pad("sink")?.current_caps()?;
    let rate = caps.structure(0)?.get::<i32>("rate").ok()?;
    if rate <= 0 || magnitudes.as_slice().is_empty() {
        return None;
    }
    let db: Vec<_> = magnitudes
        .as_slice()
        .iter()
        .map(|v| v.get::<f32>().map(f64::from).unwrap_or(FLOOR_DB))
        .collect();
    let nyquist = f64::from(rate) / 2.0;
    let high = nyquist.min(16_000.0);
    let low = 40.0_f64.min(high / 2.0);
    let bin = |hz: f64| {
        ((hz / nyquist * db.len() as f64 - 0.5).ceil().max(0.0) as usize).min(db.len() - 1)
    };
    // Logarithmic ranges keep bass, midrange and treble readable at panel widths.
    Some(
        (0..BANDS)
            .map(|i| {
                let start = bin(low * (high / low).powf(i as f64 / BANDS as f64));
                let end = bin(low * (high / low).powf((i + 1) as f64 / BANDS as f64))
                    .max(start + 1)
                    .min(db.len());
                let peak = db[start..end]
                    .iter()
                    .copied()
                    .filter(|n| n.is_finite())
                    .fold(FLOOR_DB, f64::max);
                ((peak - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_analysis_stops_buffers_without_stopping_audio_or_eos() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        gst::init().unwrap();
        let pipeline = gst::parse_launch(&format!(
            "audiotestsrc is-live=true num-buffers=150 samplesperbuffer=480 freq=440 ! \
             audio/x-raw,rate=48000 ! tee name=t \
             t. ! queue max-size-time=100000000 ! fakesink name=audible sync=true \
             t. ! {}",
            branch()
        ))
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let bus = pipeline.bus().unwrap();
        let tap = Tap::new(pipeline.upcast_ref(), &bus);
        assert!(!tap.is_enabled());
        let count = |name: &str| {
            let count = Arc::new(AtomicUsize::new(0));
            let copy = count.clone();
            pipeline
                .by_name(name)
                .unwrap()
                .static_pad("sink")
                .unwrap()
                .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                    copy.fetch_add(1, Ordering::Relaxed);
                    gst::PadProbeReturn::Ok
                });
            count
        };
        let audible = count("audible");
        let analyzed = count("wave");
        pipeline.set_state(gst::State::Playing).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(audible.load(Ordering::Relaxed) > 0);
        assert_eq!(analyzed.load(Ordering::Relaxed), 0);
        let receive = || {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                let frame = tap.take();
                if let Some(spectrum) = frame.spectrum {
                    assert!(spectrum.values.iter().any(|n| *n > 0.4));
                    assert!(spectrum.at.elapsed() < Duration::from_millis(100));
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "Analysis must arrive without polling control events"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        tap.set_enabled(true);
        receive();
        // A slow UI receives the newest result, never a backlog of old frames.
        std::thread::sleep(Duration::from_millis(180));
        let latest = tap.take();
        assert!(latest.waveform.unwrap().at.elapsed() < Duration::from_millis(100));
        assert!(latest.spectrum.unwrap().at.elapsed() < Duration::from_millis(100));
        tap.set_enabled(false);
        std::thread::sleep(Duration::from_millis(40)); // allow an in-flight buffer to finish
        let before = analyzed.load(Ordering::Relaxed);
        let playing = audible.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(analyzed.load(Ordering::Relaxed), before);
        assert!(audible.load(Ordering::Relaxed) > playing);
        assert!(tap.take().spectrum.is_none());
        tap.set_enabled(true);
        receive();
        tap.set_enabled(false);
        let end = bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(3),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        pipeline.set_state(gst::State::Null).unwrap();
        assert!(
            end.is_some_and(|m| matches!(m.view(), gst::MessageView::Eos(_))),
            "A disabled analysis branch must still forward EOS"
        );
    }

    fn measured(frequency: u32, rate: u32, volume: f64) -> Vec<f64> {
        gst::init().unwrap();
        assert!(
            supported(),
            "Install the GStreamer good plugins for audio tests"
        );
        let pipeline = gst::parse_launch(&format!(
            "audiotestsrc num-buffers=20 freq={frequency} volume={volume} ! audio/x-raw,rate={rate} ! {} ! fakesink sync=false", filter()
        )).unwrap().downcast::<gst::Pipeline>().unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();
        let bus = pipeline.bus().unwrap();
        let result = loop {
            let message = bus
                .timed_pop_filtered(
                    gst::ClockTime::from_seconds(2),
                    &[
                        gst::MessageType::Element,
                        gst::MessageType::Error,
                        gst::MessageType::Eos,
                    ],
                )
                .expect("Spectrum timeout");
            if let Some(levels) = from_message(&message) {
                break levels;
            }
            if let gst::MessageView::Error(error) = message.view() {
                panic!("{}", error.error());
            }
            assert!(
                !matches!(message.view(), gst::MessageView::Eos(_)),
                "No spectrum data"
            );
        };
        pipeline.set_state(gst::State::Null).unwrap();
        result
    }

    #[test]
    fn spectrum_tracks_actual_frequency_across_sample_rates() {
        for (frequency, range) in [(100, 3..8), (1000, 15..20), (6000, 25..29)] {
            let mut peaks = Vec::new();
            for rate in [32_000, 44_100, 48_000] {
                let levels = measured(frequency, rate, 0.5);
                assert_eq!(levels.len(), BANDS);
                assert!(
                    levels
                        .iter()
                        .all(|n| n.is_finite() && (0.0..=1.0).contains(n))
                );
                let (index, peak) = levels
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .unwrap();
                assert!(
                    range.contains(&index),
                    "{frequency} Hz at {rate}: peak band {index}"
                );
                assert!(*peak > 0.4);
                peaks.push(index);
            }
            assert!(peaks.iter().max().unwrap() - peaks.iter().min().unwrap() <= 2);
        }
    }

    #[test]
    fn silence_has_no_spectrum_energy() {
        assert!(measured(440, 44_100, 0.0).iter().all(|n| *n == 0.0));
    }
}
