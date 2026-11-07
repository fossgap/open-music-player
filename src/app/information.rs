use super::*;
use enrichment::{Field, Information, Query, Status};

#[derive(Default)]
pub(super) struct Controls {
    key: RefCell<Option<String>>,
    rendered: RefCell<Option<(String, Option<Information>)>>,
}

impl App {
    pub(super) fn schedule_information(&self) {
        let state = self.state.borrow();
        let query = state
            .current_track()
            .filter(|_| state.fetch_information)
            .and_then(|t| Query::for_track(t, Field::Information));
        let key = query.as_ref().map(|q| q.key.clone());
        let mut service = self.metadata.borrow_mut();
        if *self.information.key.borrow() != key {
            service.cancel(Field::Information);
            service.cancel(Field::Discography);
            *self.information.key.borrow_mut() = key;
        }
        if state
            .current_track()
            .is_some_and(|t| enrichment::needs_lookup(t, Field::Information))
        {
            if let Some(query) = query {
                service.request(query, false);
            }
        }
    }

    pub(super) fn update_information(&self) {
        let state = self.state.borrow();
        let track = state
            .current_track()
            .filter(|t| matches!(t.kind, Kind::Music | Kind::Spotify | Kind::YoutubeMusic));
        let service = self.metadata.borrow();
        let query = track.and_then(|t| Query::for_track(t, Field::Information));
        let status = query.as_ref().and_then(|q| service.status.get(&q.key));
        let info = track.and_then(|t| t.music_info.as_ref());
        let text = match status {
            _ if state.current_track().is_none() => {
                self.tr("Play a music track to explore its artist and album.")
            }
            _ if track.is_none() => {
                self.tr("Artist and album information is available for music tracks.")
            }
            Some(Status::Pending) => self.tr("Finding artist and album information…"),
            Some(Status::Failed(error)) => self.tr(&error.message),
            _ if info.is_some_and(|i| i.partial) => {
                self.tr("Some information is unavailable. Retry to fetch it again.")
            }
            _ if info.is_some() => String::new(),
            _ if !state.fetch_information => {
                self.tr("Online artist and album information is off. Enable it in Settings.")
            }
            _ if query.is_none() => self.tr("More tags are needed to identify this recording."),
            Some(Status::Missing) => self.tr("No unambiguous artist or album match found."),
            _ => self.tr("Waiting to find artist and album information…"),
        };
        self.label("information_status", &text);
        object::<gtk::Label>(&self.ui, "information_status").set_visible(!text.is_empty());
        let retry = self.button("information_retry");
        retry.set_visible(query.is_some() && state.fetch_information);
        retry.set_sensitive(
            !matches!(status, Some(Status::Pending))
                && !matches!(status, Some(Status::Failed(e)) if !e.retryable()),
        );
        object::<gtk::Box>(&self.ui, "information_content").set_visible(info.is_some());
        self.label(
            "info_artist_name",
            &info
                .and_then(|i| i.artist.as_ref())
                .map(|a| a.name.clone())
                .or_else(|| {
                    track
                        .filter(|t| !t.artist.is_empty())
                        .map(|t| t.artist.clone())
                })
                .unwrap_or_else(|| self.tr("Artist & album")),
        );
        self.set_art(
            &object::<gtk::Picture>(&self.ui, "info_album_art"),
            track.and_then(|t| t.artwork.as_deref()),
        );

        let key = track.map(|t| (t.id.clone(), t.music_info.clone()));
        if *self.information.rendered.borrow() != key {
            let changed_track = self.information.rendered.borrow().as_ref().map(|k| &k.0)
                != key.as_ref().map(|k| &k.0);
            *self.information.rendered.borrow_mut() = key;
            if changed_track {
                self.discography.selected.borrow_mut().take();
                for prefix in ["info_artist", "info_album"] {
                    object::<gtk::ToggleButton>(&self.ui, &format!("{prefix}_more"))
                        .set_active(false);
                }
                object::<gtk::ScrolledWindow>(&self.ui, "information_scroller")
                    .vadjustment()
                    .set_value(0.0);
            }
            for (prefix, entity) in [
                ("info_artist", info.and_then(|i| i.artist.as_ref())),
                ("info_album", info.and_then(|i| i.album.as_ref())),
            ] {
                object::<gtk::Box>(&self.ui, &format!("{prefix}_box"))
                    .set_visible(entity.is_some());
                if prefix == "info_album" {
                    self.label(
                        "info_album_name",
                        entity.map(|e| e.name.as_str()).unwrap_or(""),
                    );
                }
                self.render_information_summary(prefix);
                let genres = entity.map(|e| e.genres.join(", ")).unwrap_or_default();
                self.label(&format!("{prefix}_genres"), &genres);
                object::<gtk::Label>(&self.ui, &format!("{prefix}_genres"))
                    .set_visible(!genres.is_empty());
            }
            let date = info
                .and_then(|i| i.album.as_ref())
                .map(|a| a.date.as_str())
                .unwrap_or("");
            self.label(
                "info_album_date",
                &format!(
                    "{}: {}",
                    self.tr("First released"),
                    if date.is_empty() { "—" } else { date }
                ),
            );
            self.discography.rendering.set(true);
            let selected = self.discography.selected.borrow().clone();
            self.discography.rows.borrow_mut().clear();
            let list: gtk::ListBox = object(&self.ui, "discography_list");
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let page = info.map(|i| &i.discography);
            let mut releases: Vec<_> = page.into_iter().flat_map(|d| &d.entries).collect();
            releases.sort_by(|a, b| b.date.cmp(&a.date).then(a.title.cmp(&b.title)));
            for release in releases {
                if !enrichment::valid_mbid(&release.id) {
                    continue;
                }
                let item = gtk::ListBoxRow::builder()
                    .selectable(true)
                    .activatable(false)
                    .build();
                item.set_widget_name(&release.id);
                item.set_tooltip_text(Some(&release.title));
                item.update_property(&[gtk::accessible::Property::Label(&release.title)]);
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let year = gtk::Label::builder()
                    .label(release.date.get(..4).unwrap_or("—"))
                    .xalign(0.0)
                    .valign(gtk::Align::Start)
                    .width_chars(4)
                    .build();
                year.add_css_class("muted");
                year.add_css_class("small");
                let description = gtk::Box::new(gtk::Orientation::Vertical, 4);
                description.set_hexpand(true);
                let title = gtk::Label::builder()
                    .label(&release.title)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .width_chars(1)
                    .max_width_chars(24)
                    .hexpand(true)
                    .build();
                let detail = gtk::Label::builder()
                    .label(&release.kind)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(24)
                    .build();
                detail.add_css_class("small");
                detail.add_css_class("muted");
                description.append(&title);
                description.append(&detail);
                row.append(&year);
                row.append(&description);
                item.set_child(Some(&row));
                let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                let make_action = |action: &str, symbol: &str| {
                    let icon = gtk::Image::from_icon_name(symbol);
                    icon.add_css_class("muted");
                    let button = gtk::Button::builder()
                        .child(&icon)
                        .valign(gtk::Align::Center)
                        .build();
                    button.set_action_name(Some(action));
                    button.set_action_target_value(Some(&release.id.to_variant()));
                    button.add_css_class("discography-action");
                    controls.append(&button);
                    (button, icon)
                };
                let (play, play_icon) =
                    make_action("app.play-release", "media-playback-start-symbolic");
                let (queue, queue_icon) = make_action("app.queue-release", "list-add-symbolic");
                row.append(&controls);
                list.append(&item);
                self.discography.rows.borrow_mut().insert(
                    release.id.clone(),
                    super::discography::Row {
                        row: item,
                        play,
                        queue,
                        play_icon,
                        queue_icon,
                        label: detail,
                        kind: release.kind.clone(),
                        title: release.title.clone(),
                    },
                );
            }
            let selected_row = selected.as_ref().and_then(|id| {
                self.discography
                    .rows
                    .borrow()
                    .get(id)
                    .map(|r| r.row.clone())
            });
            list.select_row(selected_row.as_ref());
            *self.discography.selected.borrow_mut() =
                selected_row.map(|r| r.widget_name().to_string());
            self.discography.rendering.set(false);
            self.update_discography_buttons();
            let scroller: gtk::ScrolledWindow = object(&self.ui, "discography_scroller");
            // The outer viewport allocates minimum heights. Keep short discographies fully
            // readable; only longer lists need a separate scroll area.
            scroller.set_min_content_height(
                list.measure(gtk::Orientation::Vertical, -1)
                    .1
                    .clamp(64, 240),
            );
            scroller.set_visible(page.is_some_and(|d| !d.entries.is_empty()));
            self.label(
                "discography_count",
                &page
                    .map(|d| format!("{} / {}", d.entries.len(), d.total))
                    .unwrap_or_default(),
            );
            object::<gtk::Label>(&self.ui, "discography_count").set_tooltip_text(
                page.map(|d| format!("{} {}", d.total, self.tr("releases")))
                    .as_deref(),
            );
            object::<gtk::Box>(&self.ui, "discography_section")
                .set_visible(info.is_some_and(|i| i.artist.is_some()));
        }
        let more_query = track.and_then(|t| Query::for_track(t, Field::Discography));
        let more_status = more_query.as_ref().and_then(|q| service.status.get(&q.key));
        self.button("discography_more")
            .set_visible(more_query.is_some() && state.fetch_information);
        self.button("discography_more").set_sensitive(
            !matches!(more_status, Some(Status::Pending))
                && !matches!(more_status, Some(Status::Failed(e)) if !e.retryable()),
        );
        let page_status = match more_status {
            Some(Status::Pending) => self.tr("Loading more releases…"),
            Some(Status::Failed(e)) => self.tr(&e.message),
            _ if info.is_some_and(|i| i.discography.entries.is_empty()) => {
                self.tr("No releases found.")
            }
            _ => String::new(),
        };
        self.label("discography_status", &page_status);
        object::<gtk::Label>(&self.ui, "discography_status").set_visible(!page_status.is_empty());
    }

    pub(super) fn render_information_summary(&self, prefix: &str) {
        let state = self.state.borrow();
        let info = state.current_track().and_then(|t| t.music_info.as_ref());
        let artist = prefix == "info_artist";
        let summary = info
            .and_then(|i| {
                if artist {
                    i.artist.as_ref()
                } else {
                    i.album.as_ref()
                }
            })
            .and_then(|e| e.summary.as_ref());
        let text = summary.map(|s| s.text.clone()).unwrap_or_else(|| {
            self.tr(if artist {
                "No artist biography is available."
            } else {
                "No album description is available."
            })
        });
        let label: gtk::Label = object(&self.ui, &format!("{prefix}_summary"));
        let menu = gio::Menu::new();
        if let Some(source) = summary
            .map(|s| s.source.as_str())
            .filter(|uri| services::http_url(uri).is_ok())
        {
            for (title, uri) in [
                ("Wikipedia source", source),
                (
                    "CC BY-SA 4.0",
                    "https://creativecommons.org/licenses/by-sa/4.0/",
                ),
            ] {
                let item = gio::MenuItem::new(Some(&self.tr(title)), None);
                item.set_action_and_target_value(
                    Some("app.metadata-source"),
                    Some(&uri.to_variant()),
                );
                menu.append_item(&item);
            }
            label.set_tooltip_text(Some(&format!("Wikipedia · CC BY-SA 4.0\n{source}")));
        } else {
            label.set_tooltip_text(None);
        }
        label.set_extra_menu(Some(&menu));
        let preview = summary_preview(&text);
        let more: gtk::ToggleButton = object(&self.ui, &format!("{prefix}_more"));
        more.set_visible(preview != text);
        more.set_label(&self.tr(if more.is_active() {
            "Show less"
        } else {
            "Show more"
        }));
        more.set_tooltip_text(Some(&self.tr(match (artist, more.is_active()) {
            (true, false) => "Expand artist biography",
            (true, true) => "Collapse artist biography",
            (false, false) => "Expand album description",
            (false, true) => "Collapse album description",
        })));
        self.label(
            &format!("{prefix}_summary"),
            if more.is_active() { &text } else { &preview },
        );
    }
}

// Bound the preview by characters, then by words where possible (also safe for CJK text).
fn summary_preview(text: &str) -> String {
    let Some((end, _)) = text.char_indices().nth(180) else {
        return text.into();
    };
    let preview = &text[..end];
    let end = preview
        .rfind(char::is_whitespace)
        .filter(|i| *i > end / 2)
        .unwrap_or(end);
    format!(
        "{}…",
        text[..end].trim_end_matches([' ', '.', ',', ';', ':'])
    )
}
