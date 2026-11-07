use super::*;
use enrichment::{Field, Query, Status};

impl App {
    pub(super) fn setup_metadata(self: &Rc<Self>) {
        let stack: gtk::Stack = object(&self.ui, "detail_stack");
        for (name, title) in [
            ("now-playing", "Now playing"),
            ("information", "Artist & album"),
        ] {
            stack
                .page(&stack.child_by_name(name).unwrap())
                .set_title(&self.tr(title));
        }
        // Native tab semantics, with wrapping labels that also fit the compact panel.
        let tabs: gtk::StackSwitcher = object(&self.ui, "detail_tabs");
        let mut child = tabs.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some(label) = widget
                .downcast_ref::<gtk::ToggleButton>()
                .and_then(|button| button.child())
                .and_downcast::<gtk::Label>()
            {
                widget.set_tooltip_text(Some(&label.text()));
                label.set_wrap(true);
                label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                label.set_width_chars(1);
                label.set_max_width_chars(10);
                label.set_justify(gtk::Justification::Center);
            }
        }
        self.string_action("play-release", |app, id| {
            app.activate_discography(id, super::discography::Action::Play)
        });
        self.string_action("queue-release", |app, id| {
            app.activate_discography(id, super::discography::Action::Queue)
        });
        let weak = Rc::downgrade(self);
        object::<gtk::ListBox>(&self.ui, "discography_list").connect_row_selected(move |_, row| {
            if let Some(app) = weak.upgrade() {
                if !app.discography.rendering.get() {
                    *app.discography.selected.borrow_mut() =
                        row.map(|r| r.widget_name().to_string());
                }
            }
        });
        self.string_action("metadata-source", |app, uri| {
            if services::http_url(uri).is_ok() {
                if let Err(error) =
                    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
                {
                    app.notice(&error.to_string(), true, false);
                }
            }
        });
        for prefix in ["info_artist", "info_album"] {
            let weak = Rc::downgrade(self);
            object::<gtk::ToggleButton>(&self.ui, &format!("{prefix}_more")).connect_toggled(
                move |_| {
                    if let Some(app) = weak.upgrade() {
                        app.render_information_summary(prefix);
                    }
                },
            );
        }
        self.on("cover_retry", |a| a.retry_metadata(Field::Cover));
        self.on("lyrics_retry", |a| a.retry_metadata(Field::Lyrics));
        self.on("information_retry", |a| {
            a.retry_metadata(Field::Information)
        });
        self.on("discography_more", |a| a.retry_metadata(Field::Discography));
    }

    fn retry_metadata(&self, field: Field) {
        let state = self.state.borrow();
        if !enabled(&state, field) {
            return;
        }
        let query = state
            .current_track()
            .filter(|t| field == Field::Information || enrichment::missing(t, field))
            .and_then(|t| Query::for_track(t, field));
        drop(state);
        if let Some(query) = query {
            self.metadata.borrow_mut().request(query, true);
        }
        self.update_metadata();
    }

    pub(super) fn schedule_metadata(&self) {
        self.schedule_information();
        let state = self.state.borrow();
        if let Some(track) = state
            .current_track()
            .filter(|t| t.audio_details.codec.is_none())
        {
            if let Some(path) = library::local_path(&track.uri) {
                if self
                    .details_requested
                    .borrow_mut()
                    .insert(track.uri.clone())
                {
                    let tx = self.tx.clone();
                    let id = track.id.clone();
                    let uri = track.uri.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(Message::AudioDetails(
                            id,
                            uri,
                            library::audio_details(&path),
                        ));
                    });
                }
            }
        }
        let mut service = self.metadata.borrow_mut();
        let lyrics = state
            .current_track()
            .filter(|t| state.fetch_lyrics && enrichment::needs_lookup(t, Field::Lyrics))
            .and_then(|t| Query::for_track(t, Field::Lyrics));
        let key = lyrics.as_ref().map(|q| q.key.clone());
        if *self.lyrics_key.borrow() != key {
            service.cancel(Field::Lyrics);
            *self.lyrics_key.borrow_mut() = key;
        }
        if let Some(query) = lyrics {
            service.request(query, false);
        }
        if !state.fetch_covers {
            return;
        }
        let mut ids: Vec<String> = state.current.iter().cloned().collect();
        // GTK binds a small window around the viewport. Only visible rows/cards need covers.
        for items in [&self.track_rows, &self.album_cards, &self.playlist_cards] {
            for (item, _) in items.borrow().values() {
                let Some(item) = item.upgrade() else {
                    continue;
                };
                let Some(child) = item.child().filter(|child| child.is_mapped()) else {
                    continue;
                };
                let Some(bounds) = child.compute_bounds(&self.window) else {
                    continue;
                };
                if bounds.y() + bounds.height() < 0.0 || bounds.y() > self.window.height() as f32 {
                    continue;
                }
                if let Some(value) = item.item().and_downcast::<gtk::StringObject>() {
                    let id = value.string().to_string();
                    if let Some(playlist) = state.playlist(&id) {
                        if !playlist
                            .tracks
                            .iter()
                            .filter_map(|id| state.track(id))
                            .any(|t| !enrichment::missing(t, Field::Cover))
                        {
                            ids.extend(playlist.tracks.first().cloned());
                        }
                    } else {
                        ids.push(id);
                    }
                }
            }
        }
        for id in ids {
            if let Some(query) = state
                .track(&id)
                .filter(|t| enrichment::missing(t, Field::Cover))
                .and_then(|t| Query::for_track(t, Field::Cover))
            {
                service.request(query, false);
            }
        }
    }

    pub(super) fn poll_metadata(&self) {
        let replies = self.metadata.borrow_mut().poll();
        let has_replies = !replies.is_empty();
        let mut changed = false;
        {
            let mut state = self.state.borrow_mut();
            for reply in replies {
                if !enabled(&state, reply.query.field) {
                    continue;
                }
                if let Ok(Some(found)) = reply.result {
                    for track in &mut state.tracks {
                        changed |= enrichment::apply(track, &reply.query.key, &found);
                    }
                }
            }
        }
        if changed {
            self.refresh();
            self.update_player();
            self.persist();
        }
        // Retry controls become available when a provider's backoff ends. Also pick up scrolling.
        if has_replies || self.metadata_last.get().elapsed() >= Duration::from_secs(1) {
            self.metadata_last.set(Instant::now());
            self.schedule_metadata();
            self.update_metadata();
        }
    }

    pub(super) fn update_metadata(&self) {
        let state = self.state.borrow();
        let track = state.current_track();
        let service = self.metadata.borrow();
        let compact = self.window.height() < 700;
        let narrow = self.narrow_details();
        let content: gtk::Box = object(&self.ui, "content");
        content.set_margin_start(if narrow { 20 } else { 32 });
        content.set_margin_end(if narrow { 20 } else { 32 });
        object::<gtk::Label>(&self.ui, "album_header").set_visible(!narrow);
        for (_, ui) in self.track_rows.borrow().values() {
            object::<gtk::Label>(ui, "album").set_visible(!narrow);
        }
        object::<gtk::DrawingArea>(&self.ui, "detail_backdrop").set_content_height(if compact {
            120
        } else {
            200
        });
        self.update_visualization();
        for (field, prefix) in [(Field::Cover, "cover"), (Field::Lyrics, "lyrics")] {
            let query = track.and_then(|t| Query::for_track(t, field));
            let missing = track.is_some_and(|t| enrichment::missing(t, field));
            let status = query.as_ref().and_then(|q| service.status.get(&q.key));
            let active = enabled(&state, field);
            let button = self.button(&format!("{prefix}_retry"));
            button.set_visible(missing && query.is_some() && active);
            button.set_sensitive(
                !matches!(status, Some(Status::Pending))
                    && !matches!(status, Some(Status::Failed(e)) if !e.retryable()),
            );
            if field == Field::Cover {
                let source = track
                    .and_then(|t| t.artwork_source.as_deref())
                    .filter(|url| services::http_url(url).is_ok());
                let link: gtk::LinkButton = object(&self.ui, "cover_source");
                link.set_visible(source.is_some() && !missing);
                link.set_uri(source.unwrap_or("https://coverartarchive.org/"));
            }
            let text = if field == Field::Lyrics && track.is_some_and(|t| t.instrumental) {
                self.tr("Instrumental — no lyrics.")
            } else if field == Field::Lyrics && !missing && track.is_some() {
                track.and_then(|t| t.lyrics.clone()).unwrap_or_default()
            } else if track.is_none() {
                if field == Field::Lyrics {
                    self.tr("Select a track to see its lyrics.")
                } else {
                    String::new()
                }
            } else if !missing {
                String::new()
            } else if query.is_none() {
                if field == Field::Lyrics {
                    self.tr("Lyrics lookup needs a music track with a title and artist.")
                } else {
                    self.tr("Cover lookup needs an album and artist.")
                }
            } else if !active {
                if field == Field::Lyrics {
                    self.tr("Online lyrics are off. Enable them in Settings or add a local .lrc or .txt file.")
                } else {
                    self.tr("Online covers are off. Enable them in Settings.")
                }
            } else {
                match status {
                    Some(Status::Pending) => self.tr(if field == Field::Cover {
                        "Finding cover…"
                    } else {
                        "Finding lyrics…"
                    }),
                    Some(Status::Missing) => self.tr(if field == Field::Cover {
                        "No matching cover found."
                    } else {
                        "No matching lyrics found. You can add a local .lrc or .txt file."
                    }),
                    Some(Status::Failed(e)) => self.tr(&e.message),
                    _ => self.tr(if field == Field::Cover {
                        "Waiting to find cover…"
                    } else {
                        "Waiting to find lyrics…"
                    }),
                }
            };
            if field == Field::Lyrics {
                self.render_lyrics(track, &text);
            } else {
                self.label("cover_status", &text);
            }
            if field == Field::Cover {
                object::<gtk::Box>(&self.ui, "cover_lookup").set_visible(missing);
            }
        }
        self.update_audio_details();
        self.update_information();
    }

    fn update_audio_details(&self) {
        let state = self.state.borrow();
        let track = state.current_track();
        let mut details = track.map(|t| t.audio_details.clone()).unwrap_or_default();
        if track.is_some() {
            details.fill_missing(&self.audio.borrow().details());
        }
        let values = [
            ("Codec", details.codec.map(|s| s.to_uppercase())),
            (
                "Bitrate",
                details.bitrate.map(|n| {
                    format!(
                        "{}{:.0} kbps",
                        if details.bitrate_estimated {
                            "≈ "
                        } else {
                            ""
                        },
                        n as f64 / 1000.0
                    )
                }),
            ),
            (
                "Sample rate",
                details
                    .sample_rate
                    .map(|n| format!("{} kHz", f64::from(n) / 1000.0)),
            ),
            (
                "Bit depth",
                details.bit_depth.map(|n| format!("{n} {}", self.tr("bit"))),
            ),
        ];
        let describe = |fields: &[(&str, Option<String>)]| {
            fields
                .iter()
                .map(|(name, value)| {
                    format!("{}: {}", self.tr(name), value.as_deref().unwrap_or("-"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        for (id, fields) in [
            ("player_encoding", &values[..2]),
            ("player_resolution", &values[2..]),
        ] {
            let text = fields
                .iter()
                .filter_map(|(_, value)| value.as_deref())
                .collect::<Vec<_>>()
                .join(" · ");
            let label: gtk::Label = object(&self.ui, id);
            label.set_label(&text);
            label.set_visible(!text.is_empty());
            label.update_property(&[gtk::accessible::Property::Label(&describe(fields))]);
        }
        let summary: gtk::Box = object(&self.ui, "player_audio");
        let available = values.iter().any(|(_, value)| value.is_some());
        summary.set_visible(available);
        summary.set_tooltip_text(available.then(|| describe(&values)).as_deref());
    }

    pub(super) fn narrow_details(&self) -> bool {
        self.window.width() < 1100
            && object::<gtk::Revealer>(&self.ui, "detail_revealer").is_visible()
    }

    pub(super) fn metadata_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        for (id, field) in [
            ("fetch_covers", Field::Cover),
            ("fetch_lyrics", Field::Lyrics),
            ("fetch_information", Field::Information),
        ] {
            let switch: gtk::Switch = object(ui, id);
            switch.set_active(enabled(&self.state.borrow(), field));
            let weak = Rc::downgrade(self);
            switch.connect_active_notify(move |switch| {
                if let Some(app) = weak.upgrade() {
                    {
                        let mut state = app.state.borrow_mut();
                        match field {
                            Field::Cover => state.fetch_covers = switch.is_active(),
                            Field::Lyrics => state.fetch_lyrics = switch.is_active(),
                            Field::Information | Field::Discography => {
                                state.fetch_information = switch.is_active()
                            }
                        }
                    }
                    if !switch.is_active() {
                        app.metadata.borrow_mut().cancel(field);
                        if field == Field::Information {
                            app.metadata.borrow_mut().cancel(Field::Discography);
                        }
                    }
                    app.schedule_metadata();
                    app.update_metadata();
                    app.persist();
                }
            });
        }
    }
}

fn enabled(state: &State, field: Field) -> bool {
    match field {
        Field::Cover => state.fetch_covers,
        Field::Lyrics => state.fetch_lyrics,
        Field::Information | Field::Discography => state.fetch_information,
    }
}
