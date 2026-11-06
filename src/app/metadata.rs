use super::*;
use enrichment::{Field, Query, Status};

impl App {
    pub(super) fn setup_metadata(self: &Rc<Self>) {
        self.on("cover_retry", |a| a.retry_metadata(Field::Cover));
        self.on("lyrics_retry", |a| a.retry_metadata(Field::Lyrics));
    }

    fn retry_metadata(&self, field: Field) {
        let state = self.state.borrow();
        if !enabled(&state, field) {
            return;
        }
        let query = state
            .current_track()
            .filter(|t| enrichment::missing(t, field))
            .and_then(|t| Query::for_track(t, field));
        drop(state);
        if let Some(query) = query {
            self.metadata.borrow_mut().request(query, true);
        }
        self.update_metadata();
    }

    pub(super) fn schedule_metadata(&self) {
        let state = self.state.borrow();
        let mut service = self.metadata.borrow_mut();
        let lyrics = state
            .current_track()
            .filter(|t| state.fetch_lyrics && enrichment::missing(t, Field::Lyrics))
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
        object::<gtk::DrawingArea>(&self.ui, "spectrum").set_visible(!compact);
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
            let source = track.and_then(|t| {
                if field == Field::Cover {
                    t.artwork_source.as_deref()
                } else {
                    t.lyrics_source.as_deref()
                }
            });
            let link: gtk::LinkButton = object(&self.ui, &format!("{prefix}_source"));
            let source = source.filter(|url| services::http_url(url).is_ok());
            link.set_visible(source.is_some() && !missing);
            link.set_uri(source.unwrap_or("https://lrclib.net/"));
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
            self.label(
                if field == Field::Cover {
                    "cover_status"
                } else {
                    "lyrics_text"
                },
                &text,
            );
            if field == Field::Cover {
                object::<gtk::Box>(&self.ui, "cover_lookup").set_visible(missing);
            }
        }
    }

    pub(super) fn narrow_details(&self) -> bool {
        self.window.width() < 1100
            && object::<gtk::Revealer>(&self.ui, "detail_revealer").is_visible()
    }

    pub(super) fn metadata_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        for (id, field) in [
            ("fetch_covers", Field::Cover),
            ("fetch_lyrics", Field::Lyrics),
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
                        }
                    }
                    if !switch.is_active() {
                        app.metadata.borrow_mut().cancel(field);
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
    }
}
