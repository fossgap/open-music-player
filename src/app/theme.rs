//! Music-driven semantic colors, shared by every application window.
use super::*;
use std::fmt::Write;

pub(super) type Color = [f64; 3];

// Saturation and lightness preserve the hierarchy of the existing dark skin.
const ROLES: &[(&str, f64, f64)] = &[
    ("oap_accent", 0.62, 0.77),
    ("oap_accent_ink", 0.4, 0.09),
    ("oap_window", 0.48, 0.14),
    ("oap_text", 0.18, 0.95),
    ("oap_rail", 0.54, 0.095),
    ("oap_raised", 0.4, 0.20),
    ("oap_muted", 0.2, 0.77),
    ("oap_header_line", 0.39, 0.205),
    ("oap_hover", 0.45, 0.25),
    ("oap_control", 0.18, 0.93),
    ("oap_active", 0.46, 0.29),
    ("oap_flat_hover", 0.44, 0.225),
    ("oap_accent_hover", 0.55, 0.85),
    ("oap_play_hover", 0.4, 0.87),
    ("oap_selected", 0.49, 0.24),
    ("oap_nav_hover", 0.44, 0.18),
    ("oap_nav_selected", 0.55, 0.225),
    ("oap_nav_text", 0.5, 0.84),
    ("oap_separator", 0.4, 0.235),
    ("oap_field", 0.42, 0.17),
    ("oap_outline", 0.34, 0.33),
    ("oap_selection", 0.5, 0.34),
    ("oap_notice", 0.4, 0.21),
    ("oap_divider", 0.39, 0.245),
    ("oap_row_hover", 0.44, 0.205),
    ("oap_placeholder", 0.44, 0.25),
    ("oap_placeholder_text", 0.4, 0.78),
    ("oap_album_placeholder", 0.4, 0.24),
    ("oap_album_placeholder_text", 0.45, 0.78),
    ("oap_thumb", 0.3, 0.91),
    ("oap_detail", 0.45, 0.165),
    ("oap_lyrics", 0.26, 0.86),
    ("oap_popover_border", 0.34, 0.36),
    ("oap_popover_hover", 0.45, 0.28),
    ("oap_scrollbar", 0.28, 0.47),
];

fn hue(color: Color) -> f64 {
    let [r, g, b] = color;
    let high = r.max(g).max(b);
    let low = r.min(g).min(b);
    let span = high - low;
    if span < 0.02 {
        return 100.0;
    }
    (60.0
        * if high == r {
            (g - b) / span
        } else if high == g {
            2.0 + (b - r) / span
        } else {
            4.0 + (r - g) / span
        })
    .rem_euclid(360.0)
}

fn hsl(hue: f64, saturation: f64, lightness: f64) -> Color {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = hue / 60.0;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let rgb = match h as u32 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    rgb.map(|channel| channel + lightness - chroma / 2.0)
}

#[derive(Clone, Debug, PartialEq)]
struct Palette([Color; ROLES.len()]);

impl Palette {
    fn baseline() -> Self {
        // The stylesheet remains the single source for the original skin.
        let css = include_str!("../../resources/style.css");
        Self(std::array::from_fn(|i| {
            let prefix = format!("@define-color {} #", ROLES[i].0);
            let hex = css
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .expect("Missing default theme role")
                .trim_end_matches(';');
            let value = u32::from_str_radix(hex, 16).expect("Invalid default theme color");
            [16, 8, 0].map(|shift| f64::from((value >> shift) & 255) / 255.0)
        }))
    }

    fn adaptive(color: Color, energy: f64) -> Self {
        let hue = hue(color);
        let strength = 0.65 + energy.clamp(0.0, 1.0) * 0.35;
        Self(std::array::from_fn(|i| {
            let (_, saturation, lightness) = ROLES[i];
            hsl(hue, saturation * strength, lightness)
        }))
    }

    fn color(&self, name: &str) -> Color {
        self.0[ROLES
            .iter()
            .position(|r| r.0 == name)
            .expect("Unknown theme role")]
    }

    fn approach(&mut self, target: &Self, blend: f64) {
        for (color, target) in self.0.iter_mut().zip(target.0) {
            for (value, target) in color.iter_mut().zip(target) {
                *value = if (*value - target).abs() < 0.5 / 255.0 {
                    target
                } else {
                    *value + (target - *value) * blend
                };
            }
        }
    }

    fn quantized(&self) -> Self {
        Self(
            self.0
                .map(|color| color.map(|n| (n.clamp(0.0, 1.0) * 255.0).round() / 255.0)),
        )
    }

    fn should_publish(&self, applied: &Self, baseline: &Self, elapsed: Duration) -> bool {
        if elapsed < Duration::from_millis(500) || self == applied {
            return false;
        }
        // A one-byte color fluctuation need not invalidate styles across every window.
        // Still publish the final small step, and always restore the exact original skin.
        self == baseline
            || elapsed >= Duration::from_secs(2)
            || self
                .0
                .iter()
                .flatten()
                .zip(applied.0.iter().flatten())
                .any(|(a, b)| (a - b).abs() >= 2.0 / 255.0 - f64::EPSILON)
    }

    fn css(&self) -> String {
        let mut css = String::with_capacity(1800);
        for ((name, _, _), color) in ROLES.iter().zip(self.0) {
            let [r, g, b] = color.map(|n| (n.clamp(0.0, 1.0) * 255.0).round() as u8);
            let _ = writeln!(css, "@define-color {name} #{r:02x}{g:02x}{b:02x};");
        }
        css
    }
}

pub(super) struct Theme {
    provider: gtk::CssProvider,
    display: gdk::Display,
    baseline: Palette,
    current: Palette,
    applied: Palette,
    last_tick: Instant,
    last_write: Instant,
    attached: bool,
}

impl Theme {
    pub fn new() -> Self {
        let display = gdk::Display::default().expect("GTK display");
        let provider = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let baseline = Palette::baseline();
        Self {
            provider,
            display,
            current: baseline.clone(),
            applied: baseline.clone(),
            baseline,
            last_tick: Instant::now(),
            last_write: Instant::now(),
            attached: true,
        }
    }

    pub fn color(&self, name: &str) -> Color {
        self.applied.color(name)
    }

    #[cfg(test)]
    pub fn is_original(&self) -> bool {
        self.applied == self.baseline
    }

    pub fn update(&mut self, tone: Option<(Color, f64)>, visible: bool, animated: bool) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        if !visible || !self.attached {
            return false;
        }
        let target = tone
            .map(|(color, energy)| Palette::adaptive(color, energy))
            .unwrap_or_else(|| self.baseline.clone());
        // Whole-window color changes deliberately run more slowly than audio pulses.
        self.current.approach(
            &target,
            1.0 - (-dt / if animated { 1.2 } else { 2.5 }).exp(),
        );
        let next = self.current.quantized();
        if !next.should_publish(
            &self.applied,
            &self.baseline,
            now.duration_since(self.last_write),
        ) {
            return false;
        }
        if next == self.baseline {
            self.provider.load_from_data("");
        } else {
            self.provider.load_from_data(&next.css());
        }
        self.applied = next;
        self.last_write = now;
        true
    }

    pub fn detach(&mut self) {
        if self.attached {
            gtk::style_context_remove_provider_for_display(&self.display, &self.provider);
            self.attached = false;
        }
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        self.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f64 {
        let linear = color.map(|c| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn adaptive_palettes_and_transitions_preserve_readability() {
        for h in (0..360).step_by(15) {
            for energy in [0.0, 0.5, 1.0] {
                let target = Palette::adaptive(hsl(f64::from(h), 0.7, 0.5), energy);
                let mut palette = Palette::baseline();
                for _ in 0..24 {
                    palette.approach(&target, 0.25);
                    let palette = palette.quantized();
                    for bg in [
                        "oap_window",
                        "oap_rail",
                        "oap_detail",
                        "oap_field",
                        "oap_raised",
                        "oap_selected",
                        "oap_nav_selected",
                        "oap_row_hover",
                    ] {
                        for fg in ["oap_text", "oap_muted", "oap_lyrics", "oap_accent"] {
                            assert!(
                                contrast(palette.color(fg), palette.color(bg)) >= 4.5,
                                "{h}°, energy {energy}: {fg} on {bg}"
                            );
                        }
                    }
                    // A white cover is the worst case for light text, including
                    // the translucent sidebar, details, and transport surfaces.
                    let white_cover = palette
                        .color("oap_window")
                        .map(|c| c + (1.0 - c) * super::super::watermark::MAX_OPACITY);
                    for (role, opacity) in [
                        ("oap_window", 0.0),
                        ("oap_rail", 0.6),
                        ("oap_rail", 0.65),
                        ("oap_detail", 0.65),
                    ] {
                        let background = std::array::from_fn(|i| {
                            white_cover[i] * (1.0 - opacity) + palette.color(role)[i] * opacity
                        });
                        for fg in ["oap_text", "oap_muted", "oap_lyrics", "oap_accent"] {
                            assert!(
                                contrast(palette.color(fg), background) >= 4.5,
                                "White watermark: {h}°, energy {energy}: {fg} on {role}"
                            );
                        }
                    }
                    for bg in ["oap_accent", "oap_accent_hover", "oap_play_hover"] {
                        assert!(
                            contrast(palette.color("oap_accent_ink"), palette.color(bg)) >= 4.5
                        );
                    }
                    assert!(
                        contrast(palette.color("oap_thumb"), palette.color("oap_outline")) >= 3.0
                    );
                }
            }
        }
    }

    #[test]
    fn palette_returns_exactly_to_the_original_skin() {
        let baseline = Palette::baseline();
        let mut palette = Palette::adaptive([0.9, 0.3, 0.1], 1.0);
        assert_ne!(palette, baseline);
        for _ in 0..100 {
            palette.approach(&baseline, 0.2);
        }
        assert!(palette.quantized() == baseline);
    }

    #[test]
    fn tiny_music_fluctuations_do_not_restyle_the_whole_application() {
        let baseline = Palette::baseline();
        let applied = Palette::adaptive([0.9, 0.3, 0.1], 0.5).quantized();
        let mut jitter = applied.clone();
        jitter.0[0][0] += 1.0 / 255.0;
        for ms in [200, 500, 1000, 1900] {
            assert!(!jitter.should_publish(&applied, &baseline, Duration::from_millis(ms)));
        }
        assert!(jitter.should_publish(&applied, &baseline, Duration::from_secs(2)));
        let changed_cover = Palette::adaptive([0.1, 0.3, 0.9], 0.5).quantized();
        assert!(!changed_cover.should_publish(&applied, &baseline, Duration::from_millis(200)));
        assert!(changed_cover.should_publish(&applied, &baseline, Duration::from_millis(500)));
        let mut nearly_original = baseline.clone();
        nearly_original.0[0][0] += 1.0 / 255.0;
        assert!(baseline.should_publish(&nearly_original, &baseline, Duration::from_millis(500)));
    }
}
