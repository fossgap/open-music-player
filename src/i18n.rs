use gtk::prelude::*;
use std::collections::HashMap;

pub const LANGUAGES: &[(&str, &str)] = &[
    ("system", "System"),
    ("en", "English"),
    ("fr", "Français"),
    ("es", "Español"),
    ("de", "Deutsch"),
    ("it", "Italiano"),
    ("pt", "Português"),
    ("ru", "Русский"),
    ("zh", "中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

pub struct I18n {
    messages: HashMap<String, String>,
}

impl I18n {
    pub fn new(language: &str) -> Self {
        let locale = if language == "system" {
            ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .find_map(|key| std::env::var(key).ok().filter(|s| !s.is_empty()))
                .unwrap_or_else(|| "en".into())
        } else {
            language.into()
        };
        let language = locale.split(['_', '-', '.', ':']).next().unwrap_or("en");
        let json = match language {
            "fr" => include_str!("../locales/fr.json"),
            "es" => include_str!("../locales/es.json"),
            "de" => include_str!("../locales/de.json"),
            "it" => include_str!("../locales/it.json"),
            "pt" => include_str!("../locales/pt.json"),
            "ru" => include_str!("../locales/ru.json"),
            "zh" => include_str!("../locales/zh.json"),
            "ja" => include_str!("../locales/ja.json"),
            "ko" => include_str!("../locales/ko.json"),
            _ => include_str!("../locales/en.json"),
        };
        Self {
            messages: serde_json::from_str(json).expect("Invalid embedded translation catalogue"),
        }
    }
    pub fn text(&self, key: &str) -> String {
        self.messages
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.into())
    }
    pub fn builder(&self, builder: &gtk::Builder) {
        for object in builder.objects() {
            if let Some(window) = object.downcast_ref::<gtk::Window>() {
                if let Some(title) = window.title() {
                    window.set_title(Some(&self.text(&title)));
                }
            }
            if let Some(label) = object.downcast_ref::<gtk::Label>() {
                label.set_label(&self.text(&label.label()));
            }
            if let Some(button) = object.downcast_ref::<gtk::Button>() {
                if let Some(label) = button.label() {
                    button.set_label(&self.text(&label));
                }
            }
            if let Some(entry) = object.downcast_ref::<gtk::Entry>() {
                if let Some(text) = entry.placeholder_text() {
                    entry.set_placeholder_text(Some(&self.text(&text)));
                }
            }
            if let Some(entry) = object.downcast_ref::<gtk::SearchEntry>() {
                if let Some(text) = entry.placeholder_text() {
                    entry.set_placeholder_text(Some(&self.text(&text)));
                }
            }
            if let Some(widget) = object.downcast_ref::<gtk::Widget>() {
                if let Some(text) = widget.tooltip_text() {
                    widget.set_tooltip_text(Some(&self.text(&text)));
                }
            }
        }
    }
}
