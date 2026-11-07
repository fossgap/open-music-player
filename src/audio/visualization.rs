//! Frequency analysis stays in GStreamer's streaming thread, away from GTK.
use gst::prelude::*;

pub const BANDS: usize = 32;
const FLOOR_DB: f64 = -72.0;

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
