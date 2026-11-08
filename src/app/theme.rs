//! System and music-driven semantic colors, shared by every application window.
use super::*;
use std::fmt::Write;

pub(super) type Color = [f64; 3];

// Saturation and lightness preserve the hierarchy of the existing dark skin.
const ROLES: &[(&str, f64, f64)] = &[
    ("omp_accent", 0.62, 0.77),
    ("omp_accent_ink", 0.4, 0.09),
    ("omp_window", 0.48, 0.14),
    ("omp_text", 0.18, 0.95),
    ("omp_rail", 0.54, 0.095),
    ("omp_raised", 0.4, 0.20),
    ("omp_muted", 0.2, 0.77),
    ("omp_header_line", 0.39, 0.205),
    ("omp_hover", 0.45, 0.25),
    ("omp_control", 0.18, 0.93),
    ("omp_active", 0.46, 0.29),
    ("omp_flat_hover", 0.44, 0.225),
    ("omp_accent_hover", 0.55, 0.85),
    ("omp_play_hover", 0.4, 0.87),
    ("omp_selected", 0.49, 0.24),
    ("omp_nav_hover", 0.44, 0.18),
    ("omp_nav_selected", 0.55, 0.225),
    ("omp_nav_text", 0.5, 0.84),
    ("omp_separator", 0.4, 0.235),
    ("omp_field", 0.42, 0.17),
    ("omp_outline", 0.34, 0.33),
    ("omp_selection", 0.5, 0.34),
    ("omp_notice", 0.4, 0.21),
    ("omp_divider", 0.39, 0.245),
    ("omp_row_hover", 0.44, 0.205),
    ("omp_placeholder", 0.44, 0.25),
    ("omp_placeholder_text", 0.4, 0.78),
    ("omp_album_placeholder", 0.4, 0.24),
    ("omp_album_placeholder_text", 0.45, 0.78),
    ("omp_thumb", 0.3, 0.91),
    ("omp_detail", 0.45, 0.165),
    ("omp_lyrics", 0.26, 0.86),
    ("omp_popover_border", 0.34, 0.36),
    ("omp_popover_hover", 0.45, 0.28),
    ("omp_scrollbar", 0.28, 0.47),
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
    fn system(widget: &gtk::Widget) -> Self {
        let fallback = Self::baseline();
        Self(std::array::from_fn(|i| {
            widget
                .style_context()
                .lookup_color(ROLES[i].0)
                .map(|c| {
                    [
                        f64::from(c.red()),
                        f64::from(c.green()),
                        f64::from(c.blue()),
                    ]
                })
                .unwrap_or(fallback.0[i])
        }))
    }
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
    styles: gtk::CssProvider,
    provider: gtk::CssProvider,
    display: gdk::Display,
    baseline: Palette,
    current: Palette,
    applied: Palette,
    last_tick: Instant,
    last_write: Instant,
    attached: bool,
    mode: model::Theme,
    probe: gtk::Box,
    settings: gtk::Settings,
    original_dark: bool,
    system_dark: Option<bool>,
    dirty: Rc<Cell<bool>>,
    settings_changed: Option<glib::SignalHandlerId>,
    pub portal: Option<gio::SignalSubscription>,
}

impl Theme {
    pub fn new(mode: model::Theme) -> Self {
        let display = gdk::Display::default().expect("GTK display");
        let styles = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &display,
            &styles,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let provider = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let baseline = Palette::baseline();
        let settings = gtk::Settings::for_display(&display);
        let original_dark = settings.is_gtk_application_prefer_dark_theme();
        let dirty = Rc::new(Cell::new(true));
        let changed = dirty.clone();
        let settings_changed = settings.connect_notify_local(None, move |_, _| changed.set(true));
        let mut theme = Self {
            styles,
            provider,
            display,
            current: baseline.clone(),
            applied: baseline.clone(),
            baseline,
            last_tick: Instant::now(),
            last_write: Instant::now(),
            attached: true,
            mode,
            probe: gtk::Box::new(gtk::Orientation::Vertical, 0),
            settings,
            original_dark,
            system_dark: None,
            dirty,
            settings_changed: Some(settings_changed),
            portal: None,
        };
        theme.select(mode);
        theme
    }

    pub fn select(&mut self, mode: model::Theme) {
        self.mode = mode;
        self.provider.load_from_data("");
        let stylesheet = include_str!("../../resources/style.css");
        if mode == model::Theme::Sage {
            self.styles.load_from_data(stylesheet);
        } else {
            // Share one layout, while leaving the GTK theme's own named colors intact.
            let layout = stylesheet
                .split("/* Keep native controls and auxiliary windows in the same semantic palette. */")
                .next().unwrap()
                .lines()
                .filter(|line| !line.starts_with("@define-color"))
                .collect::<Vec<_>>()
                .join("\n")
                .replace(
                    " font-family: \"Adwaita Sans\", \"Cantarell\", sans-serif; font-size: 14px;",
                    "",
                );
            self.styles.load_from_data(&format!(
                "{}\n{layout}",
                include_str!("../../resources/system.css")
            ));
        }
        self.apply_dark_preference();
        self.current = self.baseline.clone();
        self.applied = if mode == model::Theme::Sage {
            self.baseline.clone()
        } else {
            Palette::system(self.probe.upcast_ref())
        };
        self.dirty.set(true);
    }

    pub fn system_scheme(&mut self, value: u32) {
        if !self.attached {
            return;
        }
        self.system_dark = match value {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        };
        self.apply_dark_preference();
        self.dirty.set(true);
    }

    fn apply_dark_preference(&self) {
        // Detect the newer property at runtime while retaining GTK 4.8 compatibility.
        if let Some(property) = self.settings.find_property("gtk-interface-color-scheme") {
            let preference = if self.mode == model::Theme::Sage {
                Some(true)
            } else {
                self.system_dark
            };
            if let Some(dark) = preference {
                if let Some(value) = glib::EnumClass::with_type(property.value_type())
                    .and_then(|class| class.to_value_by_nick(if dark { "dark" } else { "light" }))
                {
                    self.settings
                        .set_property_from_value("gtk-interface-color-scheme", &value);
                }
            } else {
                self.settings.reset_property("gtk-interface-color-scheme");
            }
            return;
        }
        self.settings.set_gtk_application_prefer_dark_theme(
            self.mode == model::Theme::Sage || self.system_dark.unwrap_or(self.original_dark),
        );
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
        if self.mode == model::Theme::System {
            if !self.dirty.replace(false) {
                return false;
            }
            self.applied = Palette::system(self.probe.upcast_ref());
            return true;
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
            self.portal.take();
            if let Some(handler) = self.settings_changed.take() {
                self.settings.disconnect(handler);
            }
            if self
                .settings
                .find_property("gtk-interface-color-scheme")
                .is_some()
            {
                self.settings.reset_property("gtk-interface-color-scheme");
            } else {
                self.settings
                    .set_gtk_application_prefer_dark_theme(self.original_dark);
            }
            gtk::style_context_remove_provider_for_display(&self.display, &self.styles);
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
                        "omp_window",
                        "omp_rail",
                        "omp_detail",
                        "omp_field",
                        "omp_raised",
                        "omp_selected",
                        "omp_nav_selected",
                        "omp_row_hover",
                    ] {
                        for fg in ["omp_text", "omp_muted", "omp_lyrics", "omp_accent"] {
                            assert!(
                                contrast(palette.color(fg), palette.color(bg)) >= 4.5,
                                "{h}°, energy {energy}: {fg} on {bg}"
                            );
                        }
                    }
                    // A white cover is the worst case for light text, including
                    // the translucent sidebar, details, and transport surfaces.
                    let white_cover = palette
                        .color("omp_window")
                        .map(|c| c + (1.0 - c) * super::super::watermark::MAX_OPACITY);
                    for (role, opacity) in [
                        ("omp_window", 0.0),
                        ("omp_rail", 0.6),
                        ("omp_rail", 0.65),
                        ("omp_detail", 0.65),
                    ] {
                        let background = std::array::from_fn(|i| {
                            white_cover[i] * (1.0 - opacity) + palette.color(role)[i] * opacity
                        });
                        for fg in ["omp_text", "omp_muted", "omp_lyrics", "omp_accent"] {
                            assert!(
                                contrast(palette.color(fg), background) >= 4.5,
                                "White watermark: {h}°, energy {energy}: {fg} on {role}"
                            );
                        }
                    }
                    for bg in ["omp_accent", "omp_accent_hover", "omp_play_hover"] {
                        assert!(
                            contrast(palette.color("omp_accent_ink"), palette.color(bg)) >= 4.5
                        );
                    }
                    assert!(
                        contrast(palette.color("omp_thumb"), palette.color("omp_outline")) >= 3.0
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
