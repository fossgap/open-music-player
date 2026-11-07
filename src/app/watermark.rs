//! A quiet cover layer underneath the interface, shared by the main and mini windows.
use super::render_area::RenderArea;
use super::*;
use gtk::{gdk_pixbuf::Pixbuf, graphene::Rect};

pub(super) const MAX_OPACITY: f64 = 0.12;

struct Layer {
    art: Pixbuf,
    texture: gdk::Texture,
    weight: f64,
}

struct Artwork {
    target: Option<Pixbuf>,
    layers: Vec<Layer>,
    opacity: f64,
    updated: Instant,
}

impl Default for Artwork {
    fn default() -> Self {
        Self {
            target: None,
            layers: Vec::new(),
            opacity: 0.09,
            updated: Instant::now(),
        }
    }
}

impl Artwork {
    fn advance(&mut self, dt: f64, enabled: bool, energy: f64, animated: bool) -> bool {
        let target = self.target.as_ref().filter(|_| enabled);
        if let Some(art) = target {
            if !self.layers.iter().any(|layer| &layer.art == art) {
                // Bound history even when a user rapidly skips through many covers.
                if self.layers.len() == 8 {
                    let oldest = self
                        .layers
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| a.weight.total_cmp(&b.weight))
                        .map(|(i, _)| i)
                        .unwrap();
                    self.layers.remove(oldest);
                }
                self.layers.push(Layer {
                    art: art.clone(),
                    texture: gdk::Texture::for_pixbuf(art),
                    weight: 0.0,
                });
            }
        }
        let blend = 1.0 - (-dt / if animated { 0.35 } else { 0.7 }).exp();
        let mut changed = false;
        for layer in &mut self.layers {
            let weight = f64::from(target == Some(&layer.art));
            let next = layer.weight + (weight - layer.weight) * blend;
            let next = if (weight - next).abs() < 0.005 {
                weight
            } else {
                next
            };
            changed |= next != layer.weight;
            layer.weight = next;
        }
        self.layers.retain(|layer| layer.weight > 0.0);
        // Use slow energy, not individual beats. Reduced motion keeps intensity steady.
        let strength = if animated {
            energy.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let opacity = 0.09 + (MAX_OPACITY - 0.09) * strength;
        let next = self.opacity + (opacity - self.opacity) * (1.0 - (-dt / 1.6).exp());
        let next = if (opacity - next).abs() < 0.0001 {
            opacity
        } else {
            next
        };
        changed |= next != self.opacity && !self.layers.is_empty();
        self.opacity = next;
        changed
    }

    fn snapshot(&self, snapshot: &gtk::Snapshot, width: i32, height: i32) {
        let (w, h) = (width as f32, height as f32);
        snapshot.push_clip(&Rect::new(0.0, 0.0, w, h));
        for layer in &self.layers {
            let (iw, ih) = (layer.texture.width() as f32, layer.texture.height() as f32);
            let scale = (w / iw).max(h / ih);
            snapshot.push_opacity(self.opacity * layer.weight);
            snapshot.append_texture(
                &layer.texture,
                &Rect::new(
                    (w - iw * scale) / 2.0,
                    (h - ih * scale) / 2.0,
                    iw * scale,
                    ih * scale,
                ),
            );
            snapshot.pop();
        }
        snapshot.pop();
    }
}

#[derive(Default)]
pub(super) struct Controls {
    pub areas: [RenderArea; 2],
    artwork: RefCell<Artwork>,
}

impl Controls {
    pub fn set_artwork(&self, art: Option<Pixbuf>) {
        self.artwork.borrow_mut().target = art;
    }

    pub fn is_visible(&self) -> bool {
        !self.artwork.borrow().layers.is_empty()
    }

    pub fn update(&self, enabled: bool, energy: f64, animated: bool) {
        let mut art = self.artwork.borrow_mut();
        let now = Instant::now();
        let dt = now.duration_since(art.updated).as_secs_f64().min(0.25);
        art.updated = now;
        if !self.areas.iter().any(area_visible) {
            return;
        }
        if art.advance(dt, enabled, energy, animated) {
            for area in &self.areas {
                if area_visible(area) {
                    area.queue_draw();
                }
            }
        }
    }
}

fn area_visible(area: &RenderArea) -> bool {
    area.is_mapped()
        && area
            .root()
            .and_then(|root| root.dynamic_cast::<gtk::Window>().ok())
            .is_some_and(|window| super::visualization::window_visible(&window))
}

impl App {
    pub(super) fn setup_watermark(self: &Rc<Self>) {
        let mini: gtk::Window = object(&self.mini_ui, "mini_window");
        for (window, area) in [self.window.upcast_ref(), &mini]
            .into_iter()
            .zip(&self.watermark.areas)
        {
            let content = window.child().expect("Window content");
            window.set_child(None::<&gtk::Widget>);
            let overlay = gtk::Overlay::new();
            area.set_can_target(false);
            area.set_focusable(false);
            area.set_accessible_role(gtk::AccessibleRole::Presentation);
            overlay.set_child(Some(area));
            overlay.add_overlay(&content);
            // The real interface retains its minimum size, focus, and event handling.
            overlay.set_measure_overlay(&content, true);
            window.set_child(Some(&overlay));
            let weak = Rc::downgrade(self);
            area.set_snapshot_func(move |snapshot, w, h| {
                if let Some(app) = weak.upgrade() {
                    app.watermark.artwork.borrow().snapshot(snapshot, w, h);
                }
            });
        }
    }

    pub(super) fn update_watermark(&self, energy: f64, animated: bool) {
        let state = self.state.borrow();
        let enabled = state.visualization == model::Visualization::AlbumArt
            && state.current_track().is_some();
        self.watermark.update(enabled, energy, animated);
        let visible = self.watermark.is_visible();
        if visible != self.window.has_css_class("artwork-watermark") {
            if visible {
                self.window.add_css_class("artwork-watermark");
            } else {
                self.window.remove_css_class("artwork-watermark");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover(color: u32) -> Pixbuf {
        let art = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, 64, 32).unwrap();
        art.fill(color);
        art
    }

    #[test]
    fn cover_changes_crossfade_and_missing_artwork_clears_the_layer() {
        let first = cover(0xff9900ff);
        let second = cover(0x0066ffff);
        let mut art = Artwork {
            target: Some(first.clone()),
            ..Default::default()
        };
        for _ in 0..30 {
            art.advance(0.1, true, 0.7, true);
        }
        assert_eq!(art.layers.len(), 1);
        assert_eq!(art.layers[0].weight, 1.0);
        art.target = Some(second.clone());
        art.advance(0.1, true, 0.7, true);
        assert_eq!(art.layers.len(), 2);
        assert_eq!(art.layers[0].art, first);
        assert!(
            art.layers
                .iter()
                .all(|layer| layer.weight > 0.0 && layer.weight < 1.0)
        );
        for _ in 0..30 {
            art.advance(0.1, true, 0.7, true);
        }
        assert_eq!(art.layers.len(), 1);
        assert_eq!(art.layers[0].art, second);
        art.target = None;
        for _ in 0..30 {
            art.advance(0.1, true, 0.0, true);
        }
        assert!(art.layers.is_empty());
        assert!(!art.advance(0.1, true, 0.0, true));
    }

    #[test]
    fn rapid_skips_stay_bounded_and_reduced_motion_settles() {
        let mut art = Artwork::default();
        for _ in 0..30 {
            art.target = Some(cover(0xffffffff));
            art.advance(0.02, true, 1.0, true);
            assert!(art.layers.len() <= 8);
            assert!(art.opacity <= MAX_OPACITY);
        }
        for _ in 0..120 {
            art.advance(0.1, true, 1.0, false);
        }
        assert_eq!(art.opacity, 0.09);
        assert!(!art.advance(0.1, true, 1.0, false));
        for _ in 0..60 {
            art.advance(0.1, false, 1.0, false);
        }
        assert!(art.layers.is_empty());
    }
}
