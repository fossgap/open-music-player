//! Local, audio-driven atmospheres. These are visual heuristics, not emotion labels.
use crate::model::Visualization;
use gtk::{
    cairo, gdk,
    gdk_pixbuf::Pixbuf,
    graphene::{Point, Rect},
    gsk,
    prelude::*,
};
use std::f64::consts::{PI, TAU};

type Color = [f64; 3];
const SAGE: Color = [0.76, 0.87, 0.64];
const FALLBACK: [Color; 2] = [SAGE, [0.43, 0.65, 0.60]];

fn mix(a: Color, b: Color, weight: f64) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * weight)
}

fn rgba(color: Color, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(color[0] as f32, color[1] as f32, color[2] as f32, alpha)
}

fn ink(cr: &cairo::Context, color: Color, alpha: f64) {
    cr.set_source_rgba(color[0], color[1], color[2], alpha);
}

fn ease(value: &mut f64, target: f64, blend: f64) -> bool {
    let next = if blend >= 1.0 || (*value - target).abs() < 0.0005 {
        target
    } else {
        *value + (target - *value) * blend
    };
    let changed = (*value - next).abs() > 0.00001;
    *value = next;
    changed
}

/// Extract two colors from the already decoded 16px cover, including alpha.
/// Bounded work on cache changes; no decoding, network or per-frame image analysis.
fn palette(art: Option<&Pixbuf>) -> [Color; 2] {
    let Some(art) = art else { return FALLBACK };
    let bytes = art.read_pixel_bytes();
    let pixels = bytes.as_ref();
    let stride = art.rowstride() as usize;
    let channels = art.n_channels() as usize;
    let mut bins = [[0.0; 4]; 64];
    for y in 0..art.height() as usize {
        for x in 0..art.width() as usize {
            let at = y * stride + x * channels;
            let rgb = &pixels[at..at + 3];
            let alpha = if channels == 4 {
                f64::from(pixels[at + 3]) / 255.0
            } else {
                1.0
            };
            let index = usize::from((rgb[0] >> 6) * 16 + (rgb[1] >> 6) * 4 + (rgb[2] >> 6));
            for (sum, byte) in bins[index][..3].iter_mut().zip(rgb) {
                *sum += f64::from(*byte) / 255.0 * alpha;
            }
            bins[index][3] += alpha;
        }
    }
    let colors: Vec<(Color, f64)> = bins
        .iter()
        .filter(|bin| bin[3] > 0.0)
        .map(|bin| {
            let color: Color = std::array::from_fn(|i| bin[i] / bin[3]);
            let max = color.iter().copied().fold(0.0, f64::max);
            let min = color.iter().copied().fold(1.0, f64::min);
            (color, bin[3] * (0.2 + max - min) * (0.2 + max))
        })
        .collect();
    let Some(&(primary, _)) = colors.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        return FALLBACK;
    };
    let contrast = |c: Color| {
        c.iter()
            .zip(primary)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
    };
    let secondary = colors
        .iter()
        .max_by(|a, b| (a.1 * contrast(a.0)).total_cmp(&(b.1 * contrast(b.0))))
        .map(|c| c.0)
        .filter(|c| contrast(*c) > 0.08)
        .unwrap_or(SAGE);
    [mix(primary, SAGE, 0.28), mix(secondary, SAGE, 0.35)]
}

#[derive(Default)]
struct Response {
    energy: f64,
    bass: f64,
    brightness: f64,
}

impl Response {
    fn measure(wave: &[f64], bands: &[f64]) -> Self {
        let rms = (wave.iter().map(|n| n * n).sum::<f64>() / wave.len().max(1) as f64).sqrt();
        let mut power = 0.0;
        let mut bass = 0.0;
        let mut brightness = 0.0;
        for (i, level) in bands.iter().enumerate() {
            // Convert the logarithmic display levels back to relative signal power.
            let value = if *level > 0.001 {
                10_f64.powf((level * 72.0 - 72.0) / 10.0)
            } else {
                0.0
            };
            power += value;
            if i < 11 {
                bass += value;
            }
            brightness += value * i as f64 / bands.len().saturating_sub(1).max(1) as f64;
        }
        Self {
            energy: (if wave.is_empty() { power.sqrt() } else { rms } * 1.5).clamp(0.0, 1.0),
            bass: if power > 1e-8 { bass / power } else { 0.0 },
            brightness: if power > 1e-8 {
                brightness / power
            } else {
                0.0
            },
        }
    }
}

pub(super) struct Scene {
    energy: f64,
    mood: f64,
    bass: f64,
    brightness: f64,
    phase: f64,
    animated: bool,
    active: bool,
    has_artwork: bool,
    artwork: Option<gdk::Texture>,
    artwork_source: Option<Pixbuf>,
    colors: [Color; 2],
    target_colors: [Color; 2],
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            energy: 0.0,
            mood: 0.0,
            bass: 0.0,
            brightness: 0.0,
            phase: 0.0,
            animated: true,
            active: false,
            has_artwork: false,
            artwork: None,
            artwork_source: None,
            colors: FALLBACK,
            target_colors: FALLBACK,
        }
    }
}

impl Scene {
    pub(super) fn set_artwork(&mut self, art: Option<&Pixbuf>) {
        if self.artwork_source.as_ref() == art {
            return;
        }
        self.artwork_source = art.cloned();
        self.target_colors = palette(art);
        self.has_artwork = art.is_some();
        self.artwork = art.map(gdk::Texture::for_pixbuf);
    }

    pub(super) fn reset_audio(&mut self) {
        self.energy = 0.0;
        self.mood = 0.0;
        self.bass = 0.0;
        self.brightness = 0.0;
        self.phase = 0.0;
        self.active = false;
    }

    pub(super) fn advance(&mut self, dt: f64, wave: &[f64], bands: &[f64], animated: bool) -> bool {
        let response = Response::measure(wave, bands);
        let blend = |seconds: f64| {
            if animated {
                1.0 - (-dt / seconds).exp()
            } else {
                1.0
            }
        };
        let mut changed = self.animated != animated;
        self.animated = animated;
        self.active = response.energy > 0.005;
        changed |= ease(&mut self.energy, response.energy, blend(0.16));
        // Color and character adapt more slowly than the beat envelope, avoiding flashes.
        changed |= ease(&mut self.mood, response.energy, blend(1.6));
        changed |= ease(&mut self.bass, response.bass, blend(0.8));
        changed |= ease(&mut self.brightness, response.brightness, blend(0.8));
        for (color, target) in self.colors.iter_mut().zip(self.target_colors) {
            for (channel, target) in color.iter_mut().zip(target) {
                changed |= ease(channel, target, blend(0.65));
            }
        }
        changed
    }

    pub(super) fn animate(&mut self, dt: f64) -> bool {
        if self.animated && self.active {
            self.phase = (self.phase + dt * self.energy * (0.4 + self.mood)) % TAU;
            true
        } else {
            false
        }
    }

    pub(super) fn theme_color(&self, mode: Visualization) -> Option<(Color, f64)> {
        match mode {
            Visualization::Mood => Some((self.mood_colors()[0], self.mood)),
            Visualization::AlbumArt if self.has_artwork => Some((self.colors[0], self.mood)),
            _ => None,
        }
    }

    fn mood_colors(&self) -> [Color; 2] {
        let calm = [0.43, 0.65, 0.66];
        let warm = [0.78, 0.58, 0.39];
        let bright = [0.77, 0.80, 0.49];
        let lively = [0.81, 0.48, 0.37];
        let tone = mix(warm, bright, self.brightness);
        let energy = mix(calm, mix(tone, lively, self.bass * self.mood), self.mood);
        [
            mix(energy, self.colors[0], 0.25),
            mix(SAGE, self.colors[1], 0.4),
        ]
    }

    fn wash(&self, snapshot: &gtk::Snapshot, w: f64, h: f64, colors: [Color; 2], skin: Color) {
        let stops: Vec<_> = colors
            .into_iter()
            .enumerate()
            .map(|(i, color)| {
                let color = mix(skin, color, 0.12 + self.mood * 0.10);
                gsk::ColorStop::new(i as f32, rgba(color, 1.0))
            })
            .collect();
        snapshot.append_linear_gradient(
            &Rect::new(0.0, 0.0, w as f32, h as f32),
            &Point::new(0.0, h as f32),
            &Point::new(w as f32, 0.0),
            &stops,
        );
    }

    pub(super) fn snapshot_mood(
        &self,
        snapshot: &gtk::Snapshot,
        w: f64,
        h: f64,
        bars: &[f64],
        skin: Color,
    ) {
        let colors = self.mood_colors();
        self.wash(snapshot, w, h, colors, skin);
        // GTK 4.8 has no native path nodes. Keep only the curves in a Cairo layer;
        // full-area gradients and artwork are handled by the active GTK renderer.
        let cr = snapshot.append_cairo(&Rect::new(0.0, 0.0, w as f32, h as f32));
        self.mood_curves(&cr, w, h, bars, colors);
        drop(cr);
        self.fade_edges(snapshot, w, h, skin);
    }

    fn mood_curves(&self, cr: &cairo::Context, w: f64, h: f64, bars: &[f64], colors: [Color; 2]) {
        let smoothed: Vec<f64> = (0..=bars.len())
            .map(|at| {
                let start = at.saturating_sub(2);
                let end = (at + 3).min(bars.len());
                bars.get(start..end)
                    .filter(|values| !values.is_empty())
                    .map(|values| values.iter().sum::<f64>() / values.len() as f64)
                    .unwrap_or(0.0)
            })
            .collect();
        let amplitude = if self.animated { self.energy } else { 0.0 };
        for layer in (0..3).rev() {
            let layer = f64::from(layer);
            let color = mix(colors[0], colors[1], layer / 2.0);
            let path = |cr: &cairo::Context| {
                for i in 0..=96 {
                    let t = f64::from(i) / 96.0;
                    let position = (1.0 - t) * bars.len().saturating_sub(1) as f64;
                    let index = position as usize;
                    let a = smoothed.get(index).copied().unwrap_or(0.0);
                    let b = smoothed.get(index + 1).copied().unwrap_or(0.0);
                    let band = a + (b - a) * position.fract();
                    let envelope = (PI * t).sin().max(0.0).powf(0.7);
                    let shape =
                        (t * TAU * (1.1 + self.brightness) + self.phase + layer * 0.8).sin();
                    let y = h * (0.52 + layer * 0.06)
                        + envelope
                            * amplitude
                            * h
                            * (shape * (0.16 + layer * 0.025) + band * 0.035);
                    if i == 0 {
                        cr.move_to(t * w, y);
                    } else {
                        cr.line_to(t * w, y);
                    }
                }
            };
            path(cr);
            let stroke = cr.copy_path().ok();
            cr.line_to(w, h);
            cr.line_to(0.0, h);
            cr.close_path();
            let gradient = cairo::LinearGradient::new(0.0, h * 0.25, 0.0, h);
            gradient.add_color_stop_rgba(0.0, color[0], color[1], color[2], 0.30);
            gradient.add_color_stop_rgba(1.0, color[0], color[1], color[2], 0.0);
            let _ = cr.set_source(&gradient);
            let _ = cr.fill();
            if let Some(path) = stroke {
                cr.append_path(&path);
            }
            ink(cr, color, 0.42 + self.mood * 0.30);
            cr.set_line_width(1.0 + (2.0 - layer) * 0.35);
            let _ = cr.stroke();
        }
    }

    pub(super) fn snapshot_artwork(&self, snapshot: &gtk::Snapshot, w: f64, h: f64, skin: Color) {
        self.wash(snapshot, w, h, self.colors, skin);
        if let Some(art) = &self.artwork {
            snapshot.push_opacity(0.13 + self.mood * 0.10);
            snapshot.append_texture(art, &Rect::new(0.0, 0.0, w as f32, h as f32));
            snapshot.pop();
        }
        let cr = snapshot.append_cairo(&Rect::new(0.0, 0.0, w as f32, h as f32));
        self.artwork_curves(&cr, w, h);
        drop(cr);
        self.fade_edges(snapshot, w, h, skin);
    }

    fn artwork_curves(&self, cr: &cairo::Context, w: f64, h: f64) {
        let cover = if h < 150.0 { 80.0 } else { 136.0 };
        let reach = ((w - cover) / 2.0 - 12.0).max(0.0);
        let energy = if self.animated { self.energy } else { 0.0 };
        for side in 0..2 {
            for strand in 0..5 {
                let band = f64::from(strand) - 2.0;
                for i in 0..=40 {
                    let t = f64::from(i) / 40.0;
                    let x = if side == 0 { t * reach } else { w - t * reach };
                    let curve = (t * PI + self.phase + band * 0.4).sin();
                    let response = if side == 0 {
                        self.bass
                    } else {
                        self.brightness
                    };
                    let y = h / 2.0
                        + band * h * 0.075
                        + curve * (PI * t).sin() * energy * h * (0.08 + response * 0.09);
                    if i == 0 {
                        cr.move_to(x, y);
                    } else {
                        cr.line_to(x, y);
                    }
                }
                ink(cr, self.colors[side], 0.22 + self.energy * 0.40);
                cr.set_line_width(if strand == 2 { 1.8 } else { 1.0 });
                let _ = cr.stroke();
            }
        }
    }

    fn fade_edges(&self, snapshot: &gtk::Snapshot, w: f64, h: f64, skin: Color) {
        let stops = [(0.0, 1.0), (0.12, 0.0), (0.88, 0.0), (1.0, 1.0)]
            .map(|(at, alpha)| gsk::ColorStop::new(at, rgba(skin, alpha)));
        for end in [Point::new(w as f32, 0.0), Point::new(0.0, h as f32)] {
            snapshot.append_linear_gradient(
                &Rect::new(0.0, 0.0, w as f32, h as f32),
                &Point::new(0.0, 0.0),
                &end,
                &stops,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atmosphere_follows_energy_and_frequency_balance() {
        let quiet = vec![0.01; 180];
        let loud = vec![0.6; 180];
        let mut bass = vec![0.0; 32];
        bass[4] = 0.8;
        let mut treble = vec![0.0; 32];
        treble[27] = 0.8;
        let low = Response::measure(&quiet, &bass);
        let high = Response::measure(&loud, &treble);
        assert!(high.energy > low.energy * 10.0);
        assert!(low.bass > 0.9 && high.bass < 0.1);
        assert!(high.brightness > low.brightness + 0.5);
        let mut scene = Scene::default();
        scene.advance(0.1, &quiet, &bass, false);
        let calm = scene.mood_colors();
        scene.advance(0.1, &loud, &treble, false);
        assert_ne!(calm, scene.mood_colors());
        assert_eq!(scene.phase, 0.0, "Reduced motion must not move the scene");
        scene.advance(0.1, &loud, &bass, true);
        scene.animate(0.1);
        assert!(scene.phase > 0.0);
        let phase = scene.phase;
        scene.advance(0.1, &[], &[], false);
        assert_eq!(scene.phase, phase, "Silence must not produce movement");
        assert_eq!(scene.energy, 0.0);
        assert!(!scene.advance(0.1, &[], &[], false));
    }

    #[test]
    fn artwork_palette_changes_and_missing_art_restores_the_skin() {
        let cover = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, 16, 16).unwrap();
        cover.fill(0xdd5522ff);
        let warm = palette(Some(&cover));
        cover.fill(0x2255ddff);
        let cool = palette(Some(&cover));
        assert!(warm[0][0] > warm[0][2]);
        assert!(cool[0][2] > cool[0][0]);
        let mut scene = Scene::default();
        scene.set_artwork(Some(&cover));
        scene.advance(0.1, &[], &[], false);
        assert_eq!(scene.colors, cool);
        scene.set_artwork(None);
        scene.advance(0.1, &[], &[], false);
        assert_eq!(scene.colors, FALLBACK);
        cover.fill(0xff000000);
        assert_eq!(palette(Some(&cover)), FALLBACK, "Ignore transparent pixels");
    }
}
