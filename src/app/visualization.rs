use super::*;
use crate::{audio::visualization as analysis, model::Visualization};
mod scene;

pub(super) struct Controls {
    pub waveform: Vec<f64>,
    pub target: Vec<f64>,
    pub bars: Vec<f64>,
    scene: scene::Scene,
    wave_at: Instant,
    spectrum_at: Instant,
    frame_at: i64,
    response_at: Instant,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            waveform: vec![0.0; 180],
            target: vec![0.0; analysis::BANDS],
            bars: vec![0.0; analysis::BANDS],
            scene: scene::Scene::default(),
            wave_at: Instant::now(),
            spectrum_at: Instant::now(),
            frame_at: 0,
            response_at: Instant::now(),
        }
    }
}

impl Controls {
    fn advance(&mut self, now: i64, playing: bool, animated: bool, scene_active: bool) -> bool {
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
        if scene_active {
            changed |= self.respond(playing, animated);
            changed |= self.scene.animate(dt);
        }
        changed
    }

    fn respond(&mut self, playing: bool, animated: bool) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.response_at).as_secs_f64().min(0.25);
        self.response_at = now;
        self.scene.advance(
            dt,
            if playing && self.wave_at.elapsed() < Duration::from_millis(250) {
                &self.waveform
            } else {
                &[]
            },
            if playing && self.spectrum_at.elapsed() < Duration::from_millis(250) {
                &self.target
            } else {
                &[]
            },
            animated,
        )
    }
}

impl App {
    pub(super) fn setup_visualization(self: &Rc<Self>) {
        if !analysis::supported() && self.state.borrow().visualization == Visualization::Spectrum {
            self.state.borrow_mut().visualization = Visualization::Waveform;
        }
        self.action("toggle-visualization", |app| {
            let current = app.state.borrow().visualization;
            app.state.borrow_mut().visualization = current.next(analysis::supported());
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
            let theme = app.theme.borrow();
            let [r, g, b] = theme.color("oap_accent");
            let skin = theme.color("oap_detail");
            match app.state.borrow().visualization {
                Visualization::Waveform => {
                    let step = w / data.waveform.len().saturating_sub(1).max(1) as f64;
                    cr.set_source_rgba(r, g, b, 0.8);
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
                    cr.set_source_rgba(r, g, b, 0.18);
                    cr.set_line_width(1.0);
                    cr.move_to(0.0, bottom);
                    cr.line_to(w, bottom);
                    let _ = cr.stroke();
                    cr.set_source_rgba(r, g, b, 0.85);
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
                Visualization::Mood => data.scene.draw_mood(cr, w, h, &data.bars, skin),
                Visualization::AlbumArt => {
                    data.scene
                        .draw_artwork(cr, w, h, app.backdrop_art.borrow().as_ref(), skin);
                }
            }
        });
        let weak = Rc::downgrade(self);
        area.add_tick_callback(move |area, clock| {
            let Some(app) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if area.is_mapped() {
                let viewport: gtk::ScrolledWindow = object(&app.ui, "detail_scroller");
                let on_screen = area.compute_bounds(&viewport).is_some_and(|bounds| {
                    bounds.y() + bounds.height() > 0.0
                        && bounds.y() < viewport.height() as f32
                        && bounds.x() + bounds.width() > 0.0
                        && bounds.x() < viewport.width() as f32
                });
                if !on_screen {
                    return glib::ControlFlow::Continue;
                }
                let animated = area.settings().is_gtk_enable_animations();
                let changed = app.visualization.borrow_mut().advance(
                    clock.frame_time(),
                    app.audio.borrow().playing,
                    animated,
                    matches!(
                        app.state.borrow().visualization,
                        Visualization::Mood | Visualization::AlbumArt
                    ),
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
        let mode = self.state.borrow().visualization;
        let text = self.tr(match mode {
            Visualization::Waveform if analysis::supported() => {
                "Live audio waveform — click to show spectrum"
            }
            Visualization::Waveform => "Live audio waveform — click to show mood",
            Visualization::Spectrum => "Audio spectrum — click to show mood",
            Visualization::Mood => "Mood theme — click to show album art theme",
            Visualization::AlbumArt => "Album art theme — click to show waveform",
        });
        let button = self.button("visualization_toggle");
        button.set_tooltip_text(Some(&text));
        button.update_property(&[gtk::accessible::Property::Label(&text)]);
        let immersive = matches!(mode, Visualization::Mood | Visualization::AlbumArt);
        let content: gtk::Box = object(&self.ui, "detail_content");
        object::<gtk::Overlay>(&self.ui, "detail_artwork").set_visible(!immersive);
        // Reuse the cover's space instead of adding another tall section above lyrics.
        let after = (!immersive).then(|| object::<gtk::Widget>(&self.ui, "cover_source"));
        if button.prev_sibling() != after {
            content.reorder_child_after(&button, after.as_ref());
        }
        let compact = self.window.height() < 700;
        object::<gtk::DrawingArea>(&self.ui, "spectrum").set_content_height(
            match (immersive, compact) {
                (true, true) => 120,
                (true, false) => 200,
                (false, true) => 48,
                (false, false) => 64,
            },
        );
        let art: gtk::Picture = object(&self.ui, "visualization_art");
        let size = if compact { 80 } else { 136 };
        art.set_size_request(size, size);
        art.set_margin_top(if compact { 20 } else { 32 });
        art.set_margin_bottom(if compact { 20 } else { 32 });
        art.set_visible(mode == Visualization::AlbumArt);
        self.update_visualization_art();
    }

    pub(super) fn update_music_theme(&self) {
        let mode = self.state.borrow().visualization;
        let animated = self.window.settings().is_gtk_enable_animations();
        let tone = if matches!(mode, Visualization::Mood | Visualization::AlbumArt) {
            let mut data = self.visualization.borrow_mut();
            data.respond(self.audio.borrow().playing, animated);
            self.state
                .borrow()
                .current
                .as_ref()
                .and_then(|_| data.scene.theme_color(mode))
        } else {
            None
        };
        // The skin follows music even with the details pane closed or in the mini player.
        let visible = self
            .application
            .windows()
            .iter()
            .any(|window| window.is_mapped());
        let changed = self.theme.borrow_mut().update(tone, visible, animated);
        self.update_watermark(tone.map(|(_, energy)| energy).unwrap_or(0.0), animated);
        if changed {
            let [r, g, b] = self.theme.borrow().color("oap_accent");
            if let Some(tag) = object::<gtk::TextView>(&self.ui, "lyrics_timed")
                .buffer()
                .tag_table()
                .lookup("current")
            {
                tag.set_foreground_rgba(Some(&gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)));
            }
            for id in ["spectrum", "empty_art", "detail_backdrop"] {
                let area: gtk::DrawingArea = object(&self.ui, id);
                if area.is_mapped() {
                    area.queue_draw();
                }
            }
        }
    }

    pub(super) fn update_visualization_art(&self) {
        let mode = self.state.borrow().visualization;
        let art: gtk::Picture = object(&self.ui, "visualization_art");
        object::<gtk::Image>(&self.ui, "visualization_placeholder")
            .set_visible(mode == Visualization::AlbumArt && art.paintable().is_none());
        self.visualization
            .borrow_mut()
            .scene
            .set_artwork(self.backdrop_art.borrow().as_ref());
        object::<gtk::DrawingArea>(&self.ui, "spectrum").queue_draw();
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
        let mut data = self.visualization.borrow_mut();
        data.waveform.fill(0.0);
        data.bars.fill(0.0);
        data.target.fill(0.0);
        data.scene.reset_audio();
        drop(data);
        self.update_visualization_art();
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
        data.advance(1_000_000, true, false, true);
        assert!(data.bars.iter().all(|n| *n == 0.8));
        data.advance(1_016_667, false, false, true);
        assert!(data.bars.iter().chain(&data.waveform).all(|n| *n == 0.0));
        data.spectrum_at = Instant::now() - Duration::from_secs(1);
        data.advance(1_033_334, true, false, true);
        assert!(data.bars.iter().all(|n| *n == 0.0));
        data.advance(1_050_001, false, true, true);
        assert!(!data.advance(1_066_668, false, true, true));
    }
}
