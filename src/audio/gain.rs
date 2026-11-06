//! Apply tagged ReplayGain before buffers reach the output; never rewrite files.
use crate::model::ReplayGain;
use gst::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Tags {
    track_gain: Option<f64>,
    album_gain: Option<f64>,
    track_peak: Option<f64>,
    album_peak: Option<f64>,
}

impl Tags {
    fn update(&mut self, tags: &gst::TagListRef) {
        macro_rules! read {
            ($field:ident, $tag:ty) => {
                if let Some(value) = tags
                    .get::<$tag>()
                    .map(|v| v.get())
                    .filter(|v| v.is_finite())
                {
                    self.$field = Some(value);
                }
            };
        }
        read!(track_gain, gst::tags::TrackGain);
        read!(album_gain, gst::tags::AlbumGain);
        read!(track_peak, gst::tags::TrackPeak);
        read!(album_peak, gst::tags::AlbumPeak);
    }

    fn volume(&self, mode: ReplayGain) -> f64 {
        let (gain, peak) = match mode {
            ReplayGain::Off => return 1.0,
            ReplayGain::Album if self.album_gain.is_some() => (self.album_gain, self.album_peak),
            _ if self.track_gain.is_some() => (self.track_gain, self.track_peak),
            _ => (self.album_gain, self.album_peak),
        };
        let Some(gain) = gain.filter(|g| g.is_finite()) else {
            return 1.0;
        };
        let volume = 10f64.powf(gain / 20.0).clamp(0.0, 10.0);
        // Known peaks cap positive gain to prevent clipping. Untagged audio is untouched.
        peak.filter(|p| p.is_finite() && *p > 0.0)
            .map_or(volume, |peak| volume.min(1.0 / peak))
    }
}

pub(super) struct Gain {
    element: gst::Element,
    state: Arc<Mutex<(ReplayGain, Tags)>>,
}

impl Gain {
    pub fn new(element: gst::Element) -> Self {
        let state = Arc::new(Mutex::new((ReplayGain::Off, Tags::default())));
        let shared = state.clone();
        let weak = element.downgrade();
        element.static_pad("sink").unwrap().add_probe(
            gst::PadProbeType::EVENT_DOWNSTREAM,
            move |_, info| {
                if let (Some(event), Some(element)) = (info.event(), weak.upgrade()) {
                    let mut state = shared.lock().unwrap();
                    match event.view() {
                        gst::EventView::StreamStart(_) => state.1 = Tags::default(),
                        gst::EventView::Tag(tag) => state.1.update(tag.tag()),
                        _ => return gst::PadProbeReturn::Ok,
                    }
                    element.set_property("volume", state.1.volume(state.0));
                }
                gst::PadProbeReturn::Ok
            },
        );
        Self { element, state }
    }

    pub fn mode(&self, mode: ReplayGain) {
        let mut state = self.state.lock().unwrap();
        state.0 = mode;
        self.element.set_property("volume", state.1.volume(mode));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_gain_modes_fallback_and_peak_protection() {
        let tags = Tags {
            track_gain: Some(-6.0),
            album_gain: Some(-3.0),
            ..Tags::default()
        };
        assert_eq!(tags.volume(ReplayGain::Off), 1.0);
        assert!((tags.volume(ReplayGain::Track) - 0.501187).abs() < 0.000001);
        assert!((tags.volume(ReplayGain::Album) - 0.707946).abs() < 0.000001);
        let tags = Tags {
            track_gain: Some(6.0),
            track_peak: Some(0.8),
            ..Tags::default()
        };
        assert_eq!(tags.volume(ReplayGain::Track), 1.25);
        assert_eq!(tags.volume(ReplayGain::Album), 1.25);
        assert_eq!(Tags::default().volume(ReplayGain::Album), 1.0);
        assert_eq!(
            Tags {
                track_gain: Some(f64::NAN),
                ..Tags::default()
            }
            .volume(ReplayGain::Track),
            1.0
        );
    }
}
