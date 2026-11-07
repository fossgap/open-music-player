use super::*;
use crate::{audio::visualization as analysis, model::Visualization};

pub(super) struct Controls {
    pub waveform: Vec<f64>,
    pub target: Vec<f64>,
    pub bars: Vec<f64>,
    wave_at: Instant,
    spectrum_at: Instant,
    frame_at: i64,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            waveform: vec![0.0; 180],
            target: vec![0.0; analysis::BANDS],
            bars: vec![0.0; analysis::BANDS],
            wave_at: Instant::now(),
            spectrum_at: Instant::now(),
            frame_at: 0,
        }
    }
}

impl Controls {
    fn advance(&mut self, now: i64, playing: bool, animated: bool) -> bool {
        let dt = if self.frame_at == 0 {
            1.0 / 60.0
        } else {
            ((now - self.frame_at) as f64 / 1_000_000.0).clamp(0.0, 0.1)
        };
        self.frame_at = now;
        let mut changed = false;
        let fresh = playing && self.spectrum_at.elapsed() < Duration::from_millis(250);
        for (bar, target) in self.bars.iter_mut().zip(&self.target) {
            let target = if fresh { *target } else { 0.0 };
            let blend = if animated {
                1.0 - (-dt / if target > *bar { 0.035 } else { 0.18 }).exp()
            } else {
                1.0
            };
            let next = *bar + (target - *bar) * blend;
            let next = if next < 0.002 { 0.0 } else { next };
            changed |= (*bar - next).abs() > 0.0001;
            *bar = next;
        }
        if !playing || self.wave_at.elapsed() >= Duration::from_millis(250) {
            let decay = if animated { (-dt / 0.15).exp() } else { 0.0 };
            for value in &mut self.waveform {
                let next = *value * decay;
                let next = if next.abs() < 0.002 { 0.0 } else { next };
                changed |= *value != next;
                *value = next;
            }
        }
        changed
    }
}

impl App {
    pub(super) fn setup_visualization(self: &Rc<Self>) {
        if !analysis::supported() {
            self.state.borrow_mut().visualization = Visualization::Waveform;
        }
        self.action("toggle-visualization", |app| {
            let current = app.state.borrow().visualization;
            if current == Visualization::Waveform && !analysis::supported() {
                app.notice(
                    &app.tr(
                        "Install the GStreamer spectrum plugin to use the spectrum visualizer.",
                    ),
                    true,
                    false,
                );
                return;
            }
            app.state.borrow_mut().visualization = match current {
                Visualization::Waveform => Visualization::Spectrum,
                Visualization::Spectrum => Visualization::Waveform,
            };
            app.update_visualization();
            object::<gtk::DrawingArea>(&app.ui, "spectrum").queue_draw();
            app.persist();
        });
        self.button("visualization_toggle")
            .set_cursor_from_name(Some("pointer"));
        let area: gtk::DrawingArea = object(&self.ui, "spectrum");
        let weak = Rc::downgrade(self);
        area.set_draw_func(move |_, cr, width, height| {
            let Some(app) = weak.upgrade() else { return };
            let data = app.visualization.borrow();
            let (w, h) = (f64::from(width), f64::from(height));
            match app.state.borrow().visualization {
                Visualization::Waveform => {
                    let step = w / data.waveform.len().saturating_sub(1).max(1) as f64;
                    cr.set_source_rgba(0.76, 0.87, 0.64, 0.8);
                    cr.set_line_width(1.8);
                    for (i, value) in data.waveform.iter().enumerate() {
                        let y = h * (0.5 - value * 0.45);
                        if i == 0 {
                            cr.move_to(0.0, y);
                        } else {
                            cr.line_to(i as f64 * step, y);
                        }
                    }
                    let _ = cr.stroke();
                }
                Visualization::Spectrum => {
                    let bottom = (h - 4.0).max(0.0);
                    cr.set_source_rgba(0.76, 0.87, 0.64, 0.18);
                    cr.set_line_width(1.0);
                    cr.move_to(0.0, bottom);
                    cr.line_to(w, bottom);
                    let _ = cr.stroke();
                    cr.set_source_rgba(0.76, 0.87, 0.64, 0.85);
                    let step = w / analysis::BANDS as f64;
                    for (i, level) in data.bars.iter().enumerate() {
                        let height = level * (h - 8.0).max(0.0);
                        if height > 0.1 {
                            cr.rectangle(
                                i as f64 * step + 1.0,
                                bottom - height,
                                (step - 2.0).max(1.0),
                                height,
                            );
                        }
                    }
                    let _ = cr.fill();
                }
            }
        });
        let weak = Rc::downgrade(self);
        area.add_tick_callback(move |area, clock| {
            let Some(app) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if area.is_mapped() {
                let animated = area.settings().is_gtk_enable_animations();
                let changed = app.visualization.borrow_mut().advance(
                    clock.frame_time(),
                    app.audio.borrow().playing,
                    animated,
                );
                if changed {
                    area.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        });
        self.update_visualization();
    }

    pub(super) fn update_visualization(&self) {
        let text = self.tr(match self.state.borrow().visualization {
            Visualization::Waveform => "Live audio waveform — click to show spectrum",
            Visualization::Spectrum => "Audio spectrum — click to show waveform",
        });
        let button = self.button("visualization_toggle");
        button.set_tooltip_text(Some(&text));
        button.update_property(&[gtk::accessible::Property::Label(&text)]);
        object::<gtk::DrawingArea>(&self.ui, "spectrum")
            .set_content_height(if self.window.height() < 700 { 48 } else { 64 });
    }

    pub(super) fn visualization_data(&self, waveform: bool, values: Vec<f64>) {
        let mut data = self.visualization.borrow_mut();
        if waveform {
            data.waveform = values;
            data.wave_at = Instant::now();
        } else if values.len() == analysis::BANDS {
            data.target = values;
            data.spectrum_at = Instant::now();
        }
        if object::<gtk::DrawingArea>(&self.ui, "spectrum").is_mapped() {
            object::<gtk::DrawingArea>(&self.ui, "spectrum").queue_draw();
        }
    }

    pub(super) fn clear_visualization(&self) {
        *self.visualization.borrow_mut() = Controls::default();
        object::<gtk::DrawingArea>(&self.ui, "spectrum").queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_or_stale_audio_settles_without_inventing_activity() {
        let mut data = Controls::default();
        data.target.fill(0.8);
        data.waveform.fill(0.4);
        data.advance(1_000_000, true, false);
        assert!(data.bars.iter().all(|n| *n == 0.8));
        data.advance(1_016_667, false, false);
        assert!(data.bars.iter().chain(&data.waveform).all(|n| *n == 0.0));
        data.spectrum_at = Instant::now() - Duration::from_secs(1);
        data.advance(1_033_334, true, false);
        assert!(data.bars.iter().all(|n| *n == 0.0));
        assert!(!data.advance(1_050_001, false, true));
    }
}
