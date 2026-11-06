use super::*;

#[derive(Clone, Copy)]
enum Grid {
    Albums,
    Playlists,
    Podcasts,
}

/// Preserve item identity, selection and scroll position when only playback or
/// metadata changes. Structural changes touch only the changed middle range.
fn sync_model(model: &gtk::StringList, ids: &[String]) {
    let old: Vec<String> = (0..model.n_items())
        .filter_map(|i| model.string(i).map(|s| s.to_string()))
        .collect();
    if old == ids {
        return;
    }
    let prefix = old.iter().zip(ids).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(ids[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let additions: Vec<&str> = ids[prefix..ids.len() - suffix]
        .iter()
        .map(String::as_str)
        .collect();
    model.splice(
        prefix as u32,
        (old.len() - prefix - suffix) as u32,
        &additions,
    );
}

fn bound(items: &BoundItems) -> Vec<(gtk::ListItem, gtk::Builder)> {
    items
        .borrow()
        .values()
        .filter_map(|(item, builder)| item.upgrade().map(|i| (i, builder.clone())))
        .collect()
}

impl App {
    pub(super) fn setup_lists(self: &Rc<Self>) {
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(self);
        factory.connect_setup(move |_, item| {
            let Some(app) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let ui = gtk::Builder::from_string(include_str!("../../resources/track.ui"));
            app.i18n.builder(&ui);
            item.set_child(Some(&object::<gtk::Box>(&ui, "row")));
            let weak_item = item.downgrade();
            let weak_app = Rc::downgrade(&app);
            object::<gtk::Button>(&ui, "favorite").connect_clicked(move |_| {
                if let (Some(app), Some(item)) = (weak_app.upgrade(), weak_item.upgrade()) {
                    if let Some(value) = item.item().and_downcast::<gtk::StringObject>() {
                        app.favorite(&value.string());
                    }
                }
            });
            app.track_rows
                .borrow_mut()
                .insert(item.as_ptr() as usize, (item.downgrade(), ui));
        });
        let weak = Rc::downgrade(self);
        factory.connect_bind(move |_, item| {
            if let Some(app) = weak.upgrade() {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let ui = app
                    .track_rows
                    .borrow()
                    .get(&(item.as_ptr() as usize))
                    .map(|(_, b)| b.clone());
                if let Some(ui) = ui {
                    app.bind_track(item, &ui);
                }
            }
        });
        let weak = Rc::downgrade(self);
        factory.connect_teardown(move |_, item| {
            if let Some(app) = weak.upgrade() {
                app.track_rows
                    .borrow_mut()
                    .remove(&(item.as_ptr() as usize));
            }
        });
        let view: gtk::ListView = object(&self.ui, "track_list");
        let selection = gtk::SingleSelection::new(Some(self.tracks_model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        view.set_model(Some(&selection));
        view.set_factory(Some(&factory));
        let weak = Rc::downgrade(self);
        view.connect_activate(move |_, position| {
            if let Some(app) = weak.upgrade() {
                let id = app.tracks_model.string(position);
                if let Some(id) = id {
                    app.start_visible(&id);
                }
            }
        });
        self.setup_grid(Grid::Albums, "album_grid", &self.albums_model);
        self.setup_grid(Grid::Playlists, "playlist_grid", &self.playlists_model);
        self.setup_grid(Grid::Podcasts, "podcast_grid", &self.podcasts_model);
    }

    fn bind_track(&self, item: &gtk::ListItem, ui: &gtk::Builder) {
        let Some(id) = item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let state = self.state.borrow();
        let Some(track) = state.track(&id.string()) else {
            return;
        };
        object::<gtk::Label>(ui, "title").set_label(&track.title);
        object::<gtk::Label>(ui, "artist").set_label(&self.artist(track));
        object::<gtk::Label>(ui, "album").set_label(&track.album);
        object::<gtk::Label>(ui, "time").set_label(&if track.kind == Kind::Radio {
            self.tr("Live")
        } else {
            model::duration(track.duration)
        });
        self.set_favorite_button(&object::<gtk::ToggleButton>(ui, "favorite"), track.favorite);
        let row: gtk::Box = object(ui, "row");
        if state.current.as_deref() == Some(&track.id) {
            row.add_css_class("playing");
        } else {
            row.remove_css_class("playing");
        }
        row.set_tooltip_text(Some(&format!("{} — {}", track.title, self.artist(track))));
        self.set_art(&object::<gtk::Picture>(ui, "art"), track.artwork.as_deref());
        let menu = gio::Menu::new();
        for (label, action) in [
            ("Play", "app.play-track"),
            ("Add to queue", "app.enqueue"),
            ("Add to playlist", "app.playlist-add"),
            ("Remove", "app.remove"),
        ] {
            let entry = gio::MenuItem::new(Some(&self.tr(label)), None);
            entry.set_action_and_target_value(Some(action), Some(&track.id.to_variant()));
            menu.append_item(&entry);
        }
        object::<gtk::MenuButton>(ui, "menu").set_menu_model(Some(&menu));
    }

    fn cards(&self, grid: Grid) -> &BoundItems {
        match grid {
            Grid::Albums => &self.album_cards,
            Grid::Playlists => &self.playlist_cards,
            Grid::Podcasts => &self.podcast_cards,
        }
    }

    fn setup_grid(self: &Rc<Self>, kind: Grid, id: &str, model: &gtk::StringList) {
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(self);
        factory.connect_setup(move |_, item| {
            let Some(app) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let ui = gtk::Builder::from_string(if matches!(kind, Grid::Podcasts) {
                include_str!("../../resources/podcast.ui")
            } else {
                include_str!("../../resources/album.ui")
            });
            app.i18n.builder(&ui);
            item.set_child(Some(&object::<gtk::Box>(
                &ui,
                if matches!(kind, Grid::Podcasts) {
                    "podcast_card"
                } else {
                    "album_card"
                },
            )));
            if matches!(kind, Grid::Podcasts) {
                let weak_item = item.downgrade();
                let weak_app = Rc::downgrade(&app);
                object::<gtk::Button>(&ui, "subscribe").connect_clicked(move |_| {
                    if let (Some(app), Some(item)) = (weak_app.upgrade(), weak_item.upgrade()) {
                        if let Some(id) = item.item().and_downcast::<gtk::StringObject>() {
                            app.subscribe_show(&id.string());
                        }
                    }
                });
            }
            app.cards(kind)
                .borrow_mut()
                .insert(item.as_ptr() as usize, (item.downgrade(), ui));
        });
        let weak = Rc::downgrade(self);
        factory.connect_bind(move |_, item| {
            if let Some(app) = weak.upgrade() {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let ui = app
                    .cards(kind)
                    .borrow()
                    .get(&(item.as_ptr() as usize))
                    .map(|(_, b)| b.clone());
                if let Some(ui) = ui {
                    app.bind_card(kind, item, &ui);
                }
            }
        });
        let weak = Rc::downgrade(self);
        factory.connect_teardown(move |_, item| {
            if let Some(app) = weak.upgrade() {
                app.cards(kind)
                    .borrow_mut()
                    .remove(&(item.as_ptr() as usize));
            }
        });
        let grid: gtk::GridView = object(&self.ui, id);
        grid.set_factory(Some(&factory));
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        grid.set_model(Some(&selection));
        let weak = Rc::downgrade(self);
        let model = model.clone();
        grid.connect_activate(move |_, position| {
            if let (Some(app), Some(id)) = (weak.upgrade(), model.string(position)) {
                match kind {
                    Grid::Albums => app.open_album(&id),
                    Grid::Playlists => app.open_playlist(&id),
                    Grid::Podcasts => {}
                }
            }
        });
    }

    fn bind_card(&self, kind: Grid, item: &gtk::ListItem, ui: &gtk::Builder) {
        let Some(value) = item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let id = value.string();
        let state = self.state.borrow();
        if matches!(kind, Grid::Podcasts) {
            let compact = self.compact_browse.get();
            object::<gtk::Box>(ui, "podcast_content").set_orientation(if compact {
                gtk::Orientation::Horizontal
            } else {
                gtk::Orientation::Vertical
            });
            object::<gtk::Box>(ui, "podcast_actions").set_orientation(if compact {
                gtk::Orientation::Horizontal
            } else {
                gtk::Orientation::Vertical
            });
            let size = if compact { 72 } else { 160 };
            object::<gtk::Image>(ui, "placeholder").set_size_request(size, size);
            object::<gtk::Image>(ui, "placeholder").set_pixel_size(if compact { 32 } else { 56 });
            let results = self.podcast_results.borrow();
            let Some(show) = results.iter().find(|s| s.id == id) else {
                return;
            };
            object::<gtk::Label>(ui, "title").set_label(&show.title);
            object::<gtk::Box>(ui, "podcast_card")
                .set_tooltip_text(Some(&format!("{}\n{}", show.title, show.author)));
            object::<gtk::Label>(ui, "author").set_label(&show.author);
            self.set_art(&object::<gtk::Picture>(ui, "art"), show.artwork.as_deref());
            let subscribed = state.subscriptions.iter().any(|s| s.url == show.feed_url);
            let button: gtk::Button = object(ui, "subscribe");
            button.set_label(&self.tr(if subscribed {
                "Subscribed"
            } else {
                "Subscribe"
            }));
            button.set_sensitive(!subscribed);
            let website: gtk::LinkButton = object(ui, "website");
            website.set_visible(show.website.is_some());
            website.set_uri(
                show.website
                    .as_deref()
                    .unwrap_or("https://podcasts.apple.com/"),
            );
            return;
        }
        let (title, subtitle, art) = if matches!(kind, Grid::Playlists) {
            let Some(playlist) = state.playlist(&id) else {
                return;
            };
            object::<gtk::Image>(ui, "placeholder")
                .set_icon_name(Some("view-list-bullet-symbolic"));
            let menu = gio::Menu::new();
            for (label, action) in [
                ("Rename playlist", "app.rename-playlist"),
                ("Delete playlist", "app.delete-playlist"),
            ] {
                let entry = gio::MenuItem::new(Some(&self.tr(label)), None);
                entry.set_action_and_target_value(Some(action), Some(&playlist.id.to_variant()));
                menu.append_item(&entry);
            }
            let options: gtk::MenuButton = object(ui, "menu");
            options.set_visible(true);
            options.set_menu_model(Some(&menu));
            let art = playlist
                .tracks
                .iter()
                .filter_map(|id| state.track(id))
                .find_map(|t| t.artwork.clone());
            (
                playlist.name.clone(),
                format!(
                    "{} {}",
                    playlist.tracks.len(),
                    self.tr(if playlist.tracks.len() == 1 {
                        "track"
                    } else {
                        "tracks"
                    })
                ),
                art,
            )
        } else {
            let Some(track) = state.track(&id) else {
                return;
            };
            (
                if track.album.is_empty() {
                    self.tr("Unknown album")
                } else {
                    track.album.clone()
                },
                self.artist(track),
                track.artwork.clone(),
            )
        };
        object::<gtk::Label>(ui, "title").set_label(&title);
        object::<gtk::Label>(ui, "artist").set_label(&subtitle);
        object::<gtk::Box>(ui, "album_card")
            .set_tooltip_text(Some(&format!("{title}\n{subtitle}")));
        self.set_art(&object::<gtk::Picture>(ui, "art"), art.as_deref());
    }

    pub(super) fn setup_browse_controls(self: &Rc<Self>) {
        self.on("browse_back", |a| a.browse_back());
        self.on("playlist_import", |a| a.choose_playlist());
        self.on("playlist_delete", |a| {
            let id = a.playlist.borrow().clone();
            if let Some(id) = id {
                a.delete_playlist(&id);
            }
        });
        self.on("podcast_search", |a| a.discover_podcasts());
        object::<gtk::DropDown>(&self.ui, "podcast_provider")
            .set_model(Some(&gtk::StringList::new(&["Apple Podcasts", "gPodder"])));
        let weak = Rc::downgrade(self);
        object::<gtk::DropDown>(&self.ui, "podcast_provider").connect_selected_notify(move |_| {
            if let Some(a) = weak.upgrade() {
                if a.podcast_discovery.get() {
                    a.discover_podcasts();
                }
            }
        });
        let weak = Rc::downgrade(self);
        object::<gtk::ToggleButton>(&self.ui, "podcast_discover").connect_toggled(move |button| {
            if let Some(a) = weak.upgrade() {
                a.podcast_discovery.set(button.is_active());
                object::<gtk::SearchEntry>(&a.ui, "search").set_text("");
                a.refresh();
                if button.is_active() && a.podcast_results.borrow().is_empty() {
                    a.discover_podcasts();
                }
            }
        });
    }

    pub(super) fn refresh(&self) {
        let page = self.page.borrow().clone();
        let state = self.state.borrow();
        let query = object::<gtk::SearchEntry>(&self.ui, "search")
            .text()
            .to_string();
        let album = self.album.borrow().clone();
        let playlist_id = self.playlist.borrow().clone();
        let playlist = playlist_id.as_deref().and_then(|id| state.playlist(id));
        let playlist_overview = page == "playlists" && playlist.is_none();
        let podcast_browser = page == "podcasts" && self.podcast_discovery.get();
        let (title, action, empty_title, empty_description, empty_action) = match page.as_str() {
            "favorites" => (
                "Favorites",
                "Add music",
                "Keep the good ones close.",
                "Star a track to find it here.",
                "Add music",
            ),
            "podcasts" if podcast_browser => (
                "Discover podcasts",
                "Add RSS feed",
                "Find your next podcast.",
                "Search by title, topic, or creator.",
                "Search podcasts",
            ),
            "podcasts" => (
                "Podcasts",
                "Add podcast",
                "A new story is a play away.",
                "Discover a show or add its RSS feed.",
                "Discover podcasts",
            ),
            "radio" => (
                "Live radio",
                "Discover radio",
                "Find your frequency.",
                "Search stations around the world.",
                "Discover radio",
            ),
            "streams" => (
                "Streaming",
                "Add stream",
                "The world is on air.",
                "Play a YouTube video or an audio stream.",
                "Add stream",
            ),
            "queue" => (
                "Play queue",
                "Clear queue",
                "What’s next?",
                "Add tracks from your library.",
                "Your music",
            ),
            "playlists" if playlist.is_some() => (
                "Playlist",
                "Rename playlist",
                "This playlist is ready for music.",
                "Add tracks from their options menu.",
                "Your music",
            ),
            "playlists" => (
                "Playlists",
                "New playlist",
                "A soundtrack of your own.",
                "Create a playlist or import one.",
                "New playlist",
            ),
            _ => (
                "Your music",
                "Add music",
                "Your next favorite is already yours.",
                "Bring your music in. We’ll take it from here.",
                "Choose a music folder",
            ),
        };
        let heading = if let Some(p) = playlist {
            p.name.clone()
        } else if let Some((_, album)) = &album {
            if album.is_empty() {
                self.tr("Unknown album")
            } else {
                album.clone()
            }
        } else {
            self.tr(title)
        };
        self.label("page_title", &heading);
        object::<gtk::Label>(&self.ui, "page_title").set_tooltip_text(Some(&heading));
        self.button("page_action").set_label(&self.tr(action));
        let ordered = match page.as_str() {
            "queue" => Some(state.queue.clone()),
            "playlists" => Some(playlist.map(|p| p.tracks.clone()).unwrap_or_default()),
            "radio" if !self.radio_results.borrow().is_empty() => {
                Some(self.radio_results.borrow().clone())
            }
            _ => None,
        };
        let mut tracks: Vec<&Track> = if podcast_browser {
            vec![]
        } else if let Some(ids) = ordered {
            ids.iter()
                .filter_map(|id| state.track(id))
                .filter(|t| t.matches(&query))
                .collect()
        } else {
            state
                .tracks
                .iter()
                .filter(|t| match page.as_str() {
                    "favorites" => t.favorite,
                    "podcasts" => t.kind == Kind::Podcast,
                    "radio" => t.kind == Kind::Radio,
                    "streams" => t.kind == Kind::Stream,
                    _ => {
                        t.kind == Kind::Music
                            && album.as_ref().is_none_or(|(artist, name)| {
                                &t.artist == artist && &t.album == name
                            })
                    }
                })
                .filter(|t| t.matches(&query))
                .collect()
        };
        if page == "library" || page == "favorites" {
            tracks.sort_by_key(|t| {
                (
                    t.artist.to_lowercase(),
                    t.album.to_lowercase(),
                    t.number,
                    t.title.to_lowercase(),
                )
            });
        }
        let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        *self.visible.borrow_mut() = ids.clone();
        sync_model(&self.tracks_model, &ids);
        let mut album_count = 0;
        if page == "library" && album.is_none() {
            let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
            for track in &tracks {
                groups
                    .entry((track.artist.clone(), track.album.clone()))
                    .or_default()
                    .push(track.id.clone());
            }
            let albums: Vec<Vec<String>> = groups.into_values().collect();
            album_count = albums.len();
            let representatives = albums
                .iter()
                .filter_map(|a| a.first().cloned())
                .collect::<Vec<_>>();
            sync_model(&self.albums_model, &representatives);
            *self.albums.borrow_mut() = albums;
        }
        let playlist_ids: Vec<String> = state
            .playlists
            .iter()
            .filter(|p| {
                !playlist_overview
                    || query
                        .split_whitespace()
                        .all(|word| p.name.to_lowercase().contains(&word.to_lowercase()))
            })
            .map(|p| p.id.clone())
            .collect();
        if playlist_overview {
            sync_model(&self.playlists_model, &playlist_ids);
        }
        let podcast_ids: Vec<String> = self
            .podcast_results
            .borrow()
            .iter()
            .map(|s| s.id.clone())
            .collect();
        sync_model(&self.podcasts_model, &podcast_ids);
        let count = if playlist_overview {
            playlist_ids.len()
        } else if podcast_browser {
            podcast_ids.len()
        } else {
            tracks.len()
        };
        self.label(
            "page_subtitle",
            &if podcast_browser && self.podcast_failed.get() {
                self.tr("Search unavailable")
            } else if podcast_browser && !self.button("podcast_search").is_sensitive() {
                self.tr("Finding podcasts…")
            } else if playlist_overview {
                format!(
                    "{} {}",
                    count,
                    self.tr(if count == 1 { "playlist" } else { "playlists" })
                )
            } else if podcast_browser {
                format!(
                    "{} {}",
                    count,
                    self.tr(if count == 1 { "podcast" } else { "podcasts" })
                )
            } else {
                let summary = format!(
                    "{} {}  ·  {}",
                    count,
                    self.tr(if count == 1 { "track" } else { "tracks" }),
                    model::duration(tracks.iter().map(|t| t.duration).sum())
                );
                if let Some((artist, _)) = &album {
                    format!("{artist}  ·  {summary}")
                } else {
                    summary
                }
            },
        );
        self.label(
            "library_status",
            &format!(
                "{} {}",
                state
                    .tracks
                    .iter()
                    .filter(|t| t.kind == Kind::Music)
                    .count(),
                self.tr("local tracks")
            ),
        );
        self.label(
            "footer_status",
            &match page.as_str() {
                "library" if album.is_none() => format!(
                    "{} {}  ·  {} {}",
                    album_count,
                    self.tr("albums"),
                    state.folders.len(),
                    self.tr("folders")
                ),
                "radio" => self.tr("Station directory by Radio Browser"),
                "podcasts" if podcast_browser => format!(
                    "{}: {}",
                    self.tr("Podcast directory"),
                    if object::<gtk::DropDown>(&self.ui, "podcast_provider").selected() == 0 {
                        "Apple Podcasts"
                    } else {
                        "gPodder"
                    }
                ),
                "podcasts" => format!("{} {}", state.subscriptions.len(), self.tr("subscriptions")),
                "streams" => self.tr("YouTube uses yt-dlp. Spotify opens in its own player."),
                "playlists" if playlist_overview => self.tr("Select a playlist to see its tracks."),
                _ => self.tr("Double-click a track to play."),
            },
        );
        object::<gtk::Box>(&self.ui, "view_switch")
            .set_visible(page == "library" && album.is_none());
        object::<gtk::Box>(&self.ui, "stream_services").set_visible(page == "streams");
        object::<gtk::Box>(&self.ui, "podcast_tabs").set_visible(page == "podcasts");
        object::<gtk::DropDown>(&self.ui, "podcast_provider").set_visible(podcast_browser);
        self.button("podcast_search").set_visible(podcast_browser);
        let compact_playlist =
            page == "playlists" && playlist.is_some() && self.compact_browse.get();
        let options: gtk::MenuButton = object(&self.ui, "playlist_options");
        options.set_visible(compact_playlist);
        if let Some(playlist) = playlist {
            let menu = gio::Menu::new();
            for (label, action) in [
                ("Rename playlist", "app.rename-playlist"),
                ("Delete playlist", "app.delete-playlist"),
            ] {
                let item = gio::MenuItem::new(Some(&self.tr(label)), None);
                item.set_action_and_target_value(Some(action), Some(&playlist.id.to_variant()));
                menu.append_item(&item);
            }
            menu.append(
                Some(&self.tr("Import playlist")),
                Some("app.playlist-import"),
            );
            options.set_menu_model(Some(&menu));
        }
        self.button("page_action").set_visible(!compact_playlist);
        self.button("refresh").set_visible(!compact_playlist);
        self.button("playlist_import")
            .set_visible(page == "playlists" && !compact_playlist);
        self.button("playlist_delete")
            .set_visible(page == "playlists" && playlist.is_some() && !compact_playlist);
        let podcast_grid: gtk::GridView = object(&self.ui, "podcast_grid");
        if self.compact_browse.get() {
            podcast_grid.set_min_columns(1);
            podcast_grid.set_max_columns(1);
        } else {
            podcast_grid.set_max_columns(5);
            podcast_grid.set_min_columns(2);
        }
        self.button("browse_back")
            .set_visible(album.is_some() || playlist.is_some());
        self.button("play_all")
            .set_visible(!playlist_overview && !podcast_browser);
        self.button("play_all").set_sensitive(!tracks.is_empty());
        object::<gtk::SearchEntry>(&self.ui, "search").set_placeholder_text(Some(&self.tr(
            if podcast_browser {
                "Search podcasts, then press Enter"
            } else if playlist_overview {
                "Search playlists"
            } else if page == "radio" {
                "Search stations, then press Enter"
            } else {
                "Search your music"
            },
        )));
        let stack: gtk::Stack = object(&self.ui, "content_stack");
        if count == 0 {
            stack.set_visible_child_name("empty");
            let loading = podcast_browser && !self.button("podcast_search").is_sensitive();
            let failed = podcast_browser && self.podcast_failed.get();
            self.label(
                "empty_title",
                &self.tr(if loading {
                    "Finding podcasts…"
                } else if failed {
                    "Podcast directory unavailable"
                } else if query.is_empty() {
                    empty_title
                } else {
                    "No matches yet."
                }),
            );
            self.label(
                "empty_description",
                &self.tr(if loading {
                    ""
                } else if failed {
                    "Could not search podcasts. Check your connection or try another directory."
                } else if query.is_empty() {
                    empty_description
                } else {
                    "Try a different search."
                }),
            );
            self.button("empty_action").set_label(&self.tr(if failed {
                "Try again"
            } else {
                empty_action
            }));
            self.button("empty_action")
                .set_visible(!loading && (query.is_empty() || page == "radio" || podcast_browser));
        } else {
            stack.set_visible_child_name(if playlist_overview {
                "playlists"
            } else if podcast_browser {
                "podcasts"
            } else if page == "library"
                && album.is_none()
                && object::<gtk::ToggleButton>(&self.ui, "albums_view").is_active()
            {
                "albums"
            } else {
                "tracks"
            });
        }
        drop(state);
        for (item, ui) in bound(&self.track_rows) {
            self.bind_track(&item, &ui);
        }
        for kind in [Grid::Albums, Grid::Playlists, Grid::Podcasts] {
            for (item, ui) in bound(self.cards(kind)) {
                self.bind_card(kind, &item, &ui);
            }
        }
    }

    pub(super) fn open_album(self: &Rc<Self>, id: &str) {
        let key = self
            .state
            .borrow()
            .track(id)
            .map(|t| (t.artist.clone(), t.album.clone()));
        let Some(key) = key else { return };
        if *self.page.borrow() != "library" {
            self.navigate("library");
        }
        *self.parent_query.borrow_mut() = object::<gtk::SearchEntry>(&self.ui, "search")
            .text()
            .to_string();
        *self.album.borrow_mut() = Some(key);
        object::<gtk::SearchEntry>(&self.ui, "search").set_text("");
        self.refresh();
    }

    pub(super) fn open_playlist(self: &Rc<Self>, id: &str) {
        if self.state.borrow().playlist(id).is_none() {
            return;
        }
        if *self.page.borrow() != "playlists" {
            self.navigate("playlists");
        }
        if self.playlist.borrow().is_none() {
            *self.parent_query.borrow_mut() = object::<gtk::SearchEntry>(&self.ui, "search")
                .text()
                .to_string();
        }
        *self.playlist.borrow_mut() = Some(id.into());
        object::<gtk::SearchEntry>(&self.ui, "search").set_text("");
        self.refresh();
    }

    pub(super) fn browse_back(&self) {
        if self.album.borrow().is_none() && self.playlist.borrow().is_none() {
            return;
        }
        self.album.borrow_mut().take();
        self.playlist.borrow_mut().take();
        object::<gtk::SearchEntry>(&self.ui, "search").set_text(&self.parent_query.borrow());
        self.refresh();
    }

    pub(super) fn discover_podcasts(self: &Rc<Self>) {
        if *self.page.borrow() != "podcasts" {
            self.navigate("podcasts");
        }
        if !self.podcast_discovery.get() {
            object::<gtk::ToggleButton>(&self.ui, "podcast_discover").set_active(true);
            return;
        }
        let directory = if object::<gtk::DropDown>(&self.ui, "podcast_provider").selected() == 0 {
            services::PodcastDirectory::Apple
        } else {
            services::PodcastDirectory::Gpodder
        };
        let query = object::<gtk::SearchEntry>(&self.ui, "search")
            .text()
            .to_string();
        let generation = self.podcast_generation.get() + 1;
        self.podcast_generation.set(generation);
        self.podcast_failed.set(false);
        self.button("podcast_search").set_sensitive(false);
        self.podcast_results.borrow_mut().clear();
        self.refresh();
        self.notice(&self.tr("Finding podcasts…"), false, true);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Message::PodcastSearch(
                generation,
                services::podcast_search(directory, &query),
            ));
        });
    }

    pub(super) fn subscribe_show(&self, id: &str) {
        let show = self
            .podcast_results
            .borrow()
            .iter()
            .find(|s| s.id == id)
            .cloned();
        if let Some(show) = show {
            if !self
                .state
                .borrow()
                .subscriptions
                .iter()
                .any(|s| s.url == show.feed_url)
            {
                self.subscribe(&show.feed_url);
            }
        }
    }

    pub(super) fn choose_playlist(self: &Rc<Self>) {
        let dialog = gtk::FileChooserNative::new(
            Some(&self.tr("Import playlist")),
            Some(&self.window),
            gtk::FileChooserAction::Open,
            Some(&self.tr("Import")),
            Some(&self.tr("Cancel")),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("M3U / M3U8 / PLS / XSPF"));
        for extension in ["m3u", "m3u8", "pls", "xspf", "M3U", "M3U8", "PLS", "XSPF"] {
            filter.add_pattern(&format!("*.{extension}"));
        }
        dialog.add_filter(&filter);
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let (Some(app), Some(path)) =
                    (weak.upgrade(), dialog.file().and_then(|f| f.path()))
                {
                    app.import_playlist(path);
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }

    pub(super) fn import_playlist(&self, path: PathBuf) {
        if self.importing_playlist.replace(true) {
            return;
        }
        self.button("playlist_import").set_sensitive(false);
        self.notice(&self.tr("Importing playlist…"), false, true);
        let existing = self.state.borrow().tracks.clone();
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Message::PlaylistImport(playlists::import(
                &path, &existing, &cache,
            )));
        });
    }

    pub(super) fn playlist_dialog(
        self: &Rc<Self>,
        track_id: Option<String>,
        rename: Option<String>,
    ) {
        let ui = gtk::Builder::from_string(include_str!("../../resources/playlist.ui"));
        self.i18n.builder(&ui);
        let window: gtk::Window = object(&ui, "dialog");
        close_on_escape(&window);
        window.set_transient_for(Some(&self.window));
        window.set_application(Some(&self.application));
        let title = self.tr(if rename.is_some() {
            "Rename playlist"
        } else if track_id.is_some() {
            "Add to playlist"
        } else {
            "New playlist"
        });
        window.set_title(Some(&title));
        object::<gtk::Label>(&ui, "title").set_label(&title);
        let entry: gtk::Entry = object(&ui, "name");
        if let Some(id) = &rename {
            if let Some(p) = self.state.borrow().playlist(id) {
                entry.set_text(&p.name);
            }
        }
        let lists: Vec<(String, String)> = self
            .state
            .borrow()
            .playlists
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let picker: gtk::ListBox = object(&ui, "existing");
        for (_, name) in &lists {
            let label = gtk::Label::new(Some(name));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_margin_top(10);
            label.set_margin_bottom(10);
            label.set_margin_start(12);
            label.set_margin_end(12);
            picker.append(&label);
        }
        if let Some(row) = picker.row_at_index(0) {
            picker.select_row(Some(&row));
        }
        object::<gtk::ScrolledWindow>(&ui, "existing_scroll")
            .set_visible(track_id.is_some() && !lists.is_empty());
        let weak = window.downgrade();
        object::<gtk::Button>(&ui, "cancel").connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                w.close();
            }
        });
        let weak = Rc::downgrade(self);
        let weak_window = window.downgrade();
        let error: gtk::Label = object(&ui, "error");
        let save: gtk::Button = object(&ui, "save");
        save.connect_clicked(move |_| {
            let Some(app) = weak.upgrade() else { return };
            let mut state = app.state.borrow_mut();
            let name = entry.text().trim().to_string();
            let result = if let Some(id) = &rename {
                state.rename_playlist(id, &name).map(|()| id.clone())
            } else if name.is_empty() && track_id.is_some() {
                picker
                    .selected_row()
                    .and_then(|r| lists.get(r.index() as usize))
                    .map(|(id, _)| id.clone())
                    .ok_or("Enter a playlist name.")
            } else {
                state.create_playlist(&name, vec![])
            };
            let id = match result {
                Ok(id) => id,
                Err(message) => {
                    error.set_label(&app.tr(message));
                    error.set_visible(true);
                    return;
                }
            };
            if let Some(track) = &track_id {
                if let Some(list) = state.playlists.iter_mut().find(|p| p.id == id) {
                    if !list.tracks.contains(track) {
                        list.tracks.push(track.clone());
                    }
                }
            }
            drop(state);
            if rename.is_some() {
                app.refresh();
            } else {
                app.open_playlist(&id);
            }
            app.persist();
            if let Some(w) = weak_window.upgrade() {
                w.close();
            }
        });
        let weak = save.downgrade();
        object::<gtk::Entry>(&ui, "name").connect_activate(move |_| {
            if let Some(save) = weak.upgrade() {
                save.emit_clicked();
            }
        });
        window.present();
        object::<gtk::Entry>(&ui, "name").grab_focus();
    }

    pub(super) fn delete_playlist(self: &Rc<Self>, id: &str) {
        let name = self.state.borrow().playlist(id).map(|p| p.name.clone());
        let Some(name) = name else { return };
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .title(self.tr("Delete playlist"))
            .text(format!("{} “{name}”?", self.tr("Delete playlist")))
            .secondary_text(self.tr("Tracks remain in your library and play queue."))
            .build();
        dialog.add_button(&self.tr("Cancel"), gtk::ResponseType::Cancel);
        dialog.add_button(&self.tr("Delete"), gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let id = id.to_string();
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(app) = weak.upgrade() {
                    app.state.borrow_mut().playlists.retain(|p| p.id != id);
                    let current = app.playlist.borrow().as_deref() == Some(&id);
                    if current {
                        app.browse_back();
                    } else {
                        app.refresh();
                    }
                    app.persist();
                }
            }
            dialog.close();
        });
        dialog.present();
    }
}
