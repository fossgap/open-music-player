use super::*;

fn settle() {
    let until = Instant::now() + Duration::from_millis(350);
    while Instant::now() < until {
        let context = glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "requires an isolated display, D-Bus session and data directory; run scripts/ui-test.sh"]
fn ui_browsing_preserves_scroll_selection_and_collections() {
    assert!(
        std::env::var_os("OPEN_AUDIO_DATA_DIR").is_some(),
        "Use scripts/ui-test.sh to isolate listening data"
    );
    assert!(std::env::var_os("OPEN_AUDIO_FAKE_SINK").is_some());
    gtk::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_data(include_str!("../../resources/style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let bytes =
        glib::Bytes::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/icons.gresource")));
    gio::resources_register(&gio::Resource::from_data(&bytes).unwrap());
    gtk::IconTheme::for_display(&gdk::Display::default().unwrap())
        .add_resource_path("/io/github/openaudio/Player/icons");
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("tone.wav");
    let size = 8000_u32 * 2 * 30;
    let mut wav = b"RIFF".to_vec();
    wav.extend((size + 36).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(8000_u32.to_le_bytes());
    wav.extend(16000_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(size.to_le_bytes());
    wav.resize(44 + size as usize, 0);
    std::fs::write(&audio, wav).unwrap();
    let uri = url::Url::from_file_path(&audio).unwrap().to_string();
    let mut state = State {
        fetch_covers: false,
        fetch_lyrics: false,
        ..State::default()
    };
    for i in 0..150 {
        let mut track = Track::new(&uri, &format!("Track {i:03}"), Kind::Music);
        track.id = format!("test{i:03}");
        track.artist = "Fixture artist".into();
        track.album = format!("Album {}", i / 30);
        track.number = i % 30 + 1;
        track.duration = 30.0;
        state.tracks.push(track);
    }
    state.queue = state.tracks.iter().map(|t| t.id.clone()).collect();
    let list = state
        .create_playlist("Evening", vec!["test003".into(), "test001".into()])
        .unwrap();
    state.create_playlist("Empty", vec![]).unwrap();
    state.sanitize();
    let application = gtk::Application::builder()
        .application_id("io.github.openaudio.BrowseTest")
        .build();
    application.register(None::<&gio::Cancellable>).unwrap();
    let app = App::new(&application, state).unwrap();
    settle();
    // Large podcast libraries must not decode hundreds of covers inside navigation.
    let cover =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 64, 64).unwrap();
    cover.fill(0x668844ff);
    let png = cover.save_to_bufferv("png", &[]).unwrap();
    let cover_paths: Vec<_> = (0..300)
        .map(|i| {
            let path = directory.path().join(format!("cover-{i}.png"));
            std::fs::write(&path, &png).unwrap();
            path.to_string_lossy().into_owned()
        })
        .collect();
    for i in 0..1000 {
        let mut episode = Track::new(
            &format!("https://example.org/episode-{i}.mp3"),
            &format!("Episode {i}"),
            Kind::Podcast,
        );
        episode.artwork = Some(cover_paths[i % cover_paths.len()].clone());
        app.state.borrow_mut().tracks.push(episode);
    }
    for _ in 0..2 {
        let start = Instant::now();
        app.button("nav_podcasts").emit_clicked();
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "Podcast navigation blocked on artwork"
        );
        settle();
        assert_eq!(app.tracks_model.n_items(), 1000);
        app.button("nav_library").emit_clicked();
        settle();
    }
    app.state
        .borrow_mut()
        .tracks
        .retain(|t| t.kind != Kind::Podcast);
    app.refresh();
    // Cache eviction must not clear and reload images already displayed by widgets.
    let pictures: Vec<_> = cover_paths
        .iter()
        .map(|path| {
            let picture = gtk::Picture::new();
            app.set_art(&picture, Some(path));
            picture
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while pictures.iter().any(|p| p.paintable().is_none()) && Instant::now() < deadline {
        settle();
    }
    assert!(pictures.iter().all(|p| p.paintable().is_some()));
    let replacements = Rc::new(Cell::new(0));
    for (picture, path) in pictures.iter().zip(&cover_paths) {
        let count = replacements.clone();
        picture.connect_paintable_notify(move |_| count.set(count.get() + 1));
        app.set_art(picture, Some(path));
    }
    settle();
    assert_eq!(
        replacements.get(),
        0,
        "Unchanged artwork must not flicker after cache eviction"
    );
    let late = directory.path().join("late-cover.png");
    std::fs::write(&late, &png).unwrap();
    let late = late.to_str().unwrap();
    let recycled = gtk::Picture::new();
    let waiting = gtk::Picture::new();
    app.set_art(&recycled, Some(late));
    app.set_art(&waiting, Some(late));
    app.set_art(&recycled, None);
    settle();
    assert!(waiting.paintable().is_some());
    assert!(
        recycled.paintable().is_none(),
        "A late cover must not overwrite a recycled row"
    );
    drop(pictures);
    let scroller: gtk::ScrolledWindow = object(&app.ui, "track_scroller");
    let view: gtk::ListView = object(&app.ui, "track_list");
    assert!(!view.is_single_click_activate());
    for page in ["library", "queue"] {
        app.navigate(page);
        settle();
        scroller.vadjustment().set_value(2000.0);
        settle();
        let position = app
            .track_rows
            .borrow()
            .values()
            .filter_map(|(item, _)| item.upgrade())
            .find(|item| {
                item.child()
                    .and_then(|child| child.compute_bounds(&scroller))
                    .is_some_and(|bounds| {
                        bounds.y() >= 5.0 && bounds.y() + bounds.height() < scroller.height() as f32
                    })
            })
            .expect("A visible track after scrolling")
            .position();
        let selection = view
            .model()
            .unwrap()
            .downcast::<gtk::SingleSelection>()
            .unwrap();
        selection.set_selected(position);
        settle();
        let before = scroller.vadjustment().value();
        assert!(before > 500.0);
        let item = app.tracks_model.item(position).unwrap();
        let id = app.visible.borrow()[position as usize].clone();
        let queue_before = app.state.borrow().queue.len();
        view.emit_by_name::<()>("activate", &[&position]);
        settle();
        assert_eq!(app.state.borrow().current.as_deref(), Some(id.as_str()));
        assert_eq!(
            selection.selected(),
            position,
            "{page}: selection changed during playback"
        );
        assert_eq!(
            app.tracks_model.item(position).unwrap(),
            item,
            "{page}: unchanged row was recreated"
        );
        assert!(
            (scroller.vadjustment().value() - before).abs() < 1.0,
            "{page}: playback jumped from {before} to {}",
            scroller.vadjustment().value()
        );
        assert_eq!(app.state.borrow().queue.len(), queue_before);
        app.favorite(&id);
        settle();
        assert!(
            (scroller.vadjustment().value() - before).abs() < 1.0,
            "Favorite reset the scroll position"
        );
    }
    app.navigate("queue");
    object::<gtk::SearchEntry>(&app.ui, "search").set_text("Track 00");
    settle();
    let full_queue = app.state.borrow().queue.clone();
    view.emit_by_name::<()>("activate", &[&0_u32]);
    settle();
    assert_eq!(
        app.state.borrow().queue,
        full_queue,
        "Playing a search result truncated the queue"
    );
    app.navigate("library");
    object::<gtk::ToggleButton>(&app.ui, "albums_view").set_active(true);
    settle();
    let albums: gtk::GridView = object(&app.ui, "album_grid");
    assert!(!albums.is_single_click_activate());
    let current = app.state.borrow().current.clone();
    albums.model().unwrap().select_item(0, true);
    settle();
    assert!(
        app.album.borrow().is_none(),
        "Single selection opened an album"
    );
    assert_eq!(app.state.borrow().current, current);
    albums.emit_by_name::<()>("activate", &[&0_u32]);
    settle();
    assert_eq!(
        object::<gtk::Stack>(&app.ui, "content_stack")
            .visible_child_name()
            .as_deref(),
        Some("tracks")
    );
    assert_eq!(app.visible.borrow().len(), 30);
    assert_eq!(
        app.state.borrow().current,
        current,
        "Opening an album started playback"
    );
    view.emit_by_name::<()>("activate", &[&2_u32]);
    settle();
    assert_eq!(app.state.borrow().queue.len(), 30);
    app.browse_back();
    settle();
    assert_eq!(
        object::<gtk::Stack>(&app.ui, "content_stack")
            .visible_child_name()
            .as_deref(),
        Some("albums")
    );
    // Play the entire album directly from its card without leaving the grid.
    let album_card = app
        .album_cards
        .borrow()
        .values()
        .find_map(|(item, ui)| {
            item.upgrade()
                .filter(|item| item.position() == 0)
                .map(|_| ui.clone())
        })
        .unwrap();
    let play: gtk::Button = object(&album_card, "card_play");
    play.grab_focus();
    settle();
    screenshot(app.window.upcast_ref(), "album-quick-play.png");
    play.emit_clicked();
    settle();
    assert_eq!(app.state.borrow().queue.len(), 30);
    assert_eq!(app.state.borrow().current.as_deref(), Some("test000"));
    assert!(app.album.borrow().is_none());
    assert_eq!(
        object::<gtk::Stack>(&app.ui, "content_stack")
            .visible_child_name()
            .as_deref(),
        Some("albums")
    );

    app.navigate("playlists");
    settle();
    let playlists: gtk::GridView = object(&app.ui, "playlist_grid");
    assert!(playlists.is_single_click_activate());
    assert_eq!(app.playlists_model.n_items(), 2);
    for (item, ui) in app.playlist_cards.borrow().values() {
        let id = item
            .upgrade()
            .unwrap()
            .item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string();
        if id != list {
            assert!(!object::<gtk::Button>(ui, "card_play").is_sensitive());
        }
    }
    let card = app
        .playlist_cards
        .borrow()
        .values()
        .find_map(|(item, ui)| {
            item.upgrade()
                .filter(|item| item.position() == 0)
                .map(|_| ui.clone())
        })
        .unwrap();
    object::<gtk::Button>(&card, "card_play").grab_focus();
    settle();
    screenshot(app.window.upcast_ref(), "playlist-quick-play.png");
    object::<gtk::Button>(&card, "card_play").emit_clicked();
    settle();
    assert_eq!(app.state.borrow().queue, vec!["test003", "test001"]);
    assert!(app.playlist.borrow().is_none());
    playlists.emit_by_name::<()>("activate", &[&0_u32]);
    settle();
    assert_eq!(app.playlist.borrow().as_deref(), Some(list.as_str()));
    assert_eq!(*app.visible.borrow(), vec!["test003", "test001"]);
    app.state
        .borrow_mut()
        .rename_playlist(&list, "Renamed")
        .unwrap();
    app.refresh();
    assert_eq!(
        object::<gtk::Label>(&app.ui, "page_title").label(),
        "Renamed"
    );
    let queue = app.state.borrow().queue.clone();
    app.delete_playlist(&list);
    settle();
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::MessageDialog>().ok())
        .next()
        .unwrap();
    dialog.response(gtk::ResponseType::Cancel);
    settle();
    assert!(app.state.borrow().playlist(&list).is_some());
    app.delete_playlist(&list);
    settle();
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::MessageDialog>().ok())
        .next()
        .unwrap();
    dialog.response(gtk::ResponseType::Accept);
    settle();
    assert!(app.state.borrow().playlist(&list).is_none());
    assert!(app.playlist.borrow().is_none());
    assert_eq!(app.state.borrow().queue, queue);
    assert_eq!(app.state.borrow().tracks.len(), 150);

    // Compact collection headings retain their identity and expose maintenance in a menu.
    let compact_list = app
        .state
        .borrow_mut()
        .create_playlist("Weekend selections", vec![])
        .unwrap();
    app.open_playlist(&compact_list);
    app.window.set_default_size(820, 620);
    settle();
    assert!(app.compact_browse.get());
    assert!(object::<gtk::MenuButton>(&app.ui, "playlist_options").is_visible());
    assert!(!app.button("page_action").is_visible());
    assert_eq!(
        object::<gtk::Label>(&app.ui, "page_title")
            .tooltip_text()
            .as_deref(),
        Some("Weekend selections")
    );

    // A compact result exposes its identity and action in the same initial viewport.
    app.navigate("podcasts");
    app.podcast_results
        .borrow_mut()
        .push(services::PodcastShow {
            id: "fixture-show".into(),
            title: "Fixture podcast".into(),
            author: "Fixture author".into(),
            feed_url: "https://example.org/feed".into(),
            artwork: None,
            website: None,
        });
    object::<gtk::ToggleButton>(&app.ui, "podcast_discover").set_active(true);
    settle();
    let scroller: gtk::ScrolledWindow = object(&app.ui, "podcast_scroller");
    let card = app
        .podcast_cards
        .borrow()
        .values()
        .find_map(|(item, ui)| {
            item.upgrade()
                .and_then(|item| item.item())
                .map(|_| ui.clone())
        })
        .expect("Visible podcast result");
    for id in ["title", "subscribe"] {
        let widget: gtk::Widget = object(&card, id);
        let bounds = widget.compute_bounds(&scroller).unwrap();
        assert!(
            bounds.y() >= 0.0 && bounds.y() + bounds.height() <= scroller.height() as f32,
            "{id} must fit in the compact result viewport: {bounds:?}, viewport height {}, window {}×{}",
            scroller.height(),
            app.window.width(),
            app.window.height()
        );
    }

    // Failed requests must not claim a successful search with zero matches.
    app.podcast_results.borrow_mut().clear();
    app.tx
        .send(Message::PodcastSearch(
            app.podcast_generation.get(),
            Err("Offline fixture".into()),
        ))
        .unwrap();
    settle();
    assert_eq!(
        object::<gtk::Label>(&app.ui, "page_subtitle").text(),
        "Search unavailable"
    );
    assert_eq!(
        object::<gtk::Label>(&app.ui, "empty_title").text(),
        "Podcast directory unavailable"
    );
    assert_eq!(
        app.button("empty_action").label().as_deref(),
        Some("Try again")
    );
    // YouTube Music uses the same track list and playlist grid, with stale job protection.
    app.navigate("youtube");
    assert!(object::<gtk::Box>(&app.ui, "youtube_tabs").is_visible());
    assert!(app.button("youtube_search").is_visible());
    app.handle_youtube(youtube::Reply::Connected(Ok(model::YoutubeAccount {
        id: "@fixture".into(),
        name: "Fixture account".into(),
    })));
    let song = youtube_music::Song {
        video_id: "abcdefghijk".into(),
        title: "Fixture song".into(),
        artist: "Fixture artist".into(),
        album: "Fixture album".into(),
        duration: 30.0,
        artwork: None,
    }
    .into_track()
    .unwrap();
    let song_id = song.id.clone();
    app.youtube.generation.set(5);
    app.handle_youtube(youtube::Reply::Search(4, Ok(vec![song.clone()])));
    assert!(app.youtube.results.borrow().is_empty());
    app.handle_youtube(youtube::Reply::Search(5, Ok(vec![song])));
    object::<gtk::SearchEntry>(&app.ui, "search")
        .set_text("query need not occur literally in provider results");
    app.refresh();
    assert_eq!(&*app.visible.borrow(), std::slice::from_ref(&song_id));
    settle();
    screenshot(app.window.upcast_ref(), "youtube-search-compact.png");
    // Pausing or stopping before URL resolution must not start delayed audio.
    app.state.borrow_mut().current = Some(song_id.clone());
    app.play_generation.set(77);
    app.resolving.set(true);
    app.resolve_autoplay.set(true);
    app.toggle_play();
    app.tx
        .send(Message::Youtube(
            77,
            Ok(youtube_music::Resolved {
                uri: uri.clone(),
                headers: HashMap::new(),
            }),
        ))
        .unwrap();
    settle();
    assert!(app.audio.borrow().loaded);
    assert!(!app.audio.borrow().playing);
    app.stop_playback();
    app.tx
        .send(Message::Youtube(
            77,
            Ok(youtube_music::Resolved {
                uri: uri.clone(),
                headers: HashMap::new(),
            }),
        ))
        .unwrap();
    settle();
    assert!(!app.audio.borrow().loaded);
    let remote = youtube_music::RemotePlaylist {
        id: "PL_fixture".into(),
        name: "YouTube collection".into(),
        tracks: vec![youtube_music::Song {
            video_id: "abcdefghijk".into(),
            title: "Fixture song".into(),
            artist: "Fixture artist".into(),
            album: "Fixture album".into(),
            duration: 30.0,
            artwork: None,
        }],
        editable: true,
        snapshot: youtube_music::snapshot("YouTube collection", &["abcdefghijk".into()]),
        unavailable: 0,
        local_id: None,
    };
    app.handle_youtube(youtube::Reply::Synced(
        "@fixture".into(),
        vec![],
        Ok(youtube_music::SyncResult {
            playlists: vec![remote],
            ..Default::default()
        }),
    ));
    object::<gtk::ToggleButton>(&app.ui, "youtube_playlists").set_active(true);
    settle();
    assert_eq!(app.playlists_model.n_items(), 1);
    screenshot(app.window.upcast_ref(), "youtube-playlists-compact.png");
    let playlist_id = app.playlists_model.string(0).unwrap();
    app.open_playlist(&playlist_id);
    assert_eq!(app.page.borrow().as_str(), "youtube");
    assert_eq!(&*app.visible.borrow(), std::slice::from_ref(&song_id));
    app.remove(&song_id);
    assert!(
        app.state
            .borrow()
            .playlist(&playlist_id)
            .unwrap()
            .tracks
            .is_empty()
    );
    assert!(app.state.borrow().track(&song_id).is_some());
    app.youtube_primary();
    settle();
    let account_window = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.title().as_deref() == Some("YouTube Music account"))
        .unwrap();
    screenshot(&account_window, "youtube-account.png");
    account_window.close();
    app.handle_youtube(youtube::Reply::Disconnected(Ok(())));
    assert!(app.state.borrow().youtube_account.is_none());
    assert!(app.state.borrow().track(&song_id).is_none());
    assert!(app.state.borrow().track("test000").is_some());
    // One sidebar entry serves all streaming providers, preserving provider selection.
    assert!(app.ui.object::<gtk::Button>("nav_youtube").is_none());
    assert!(app.ui.object::<gtk::Button>("nav_spotify").is_none());
    app.navigate("spotify");
    assert!(app.button("nav_streams").has_css_class("selected"));
    assert_eq!(
        object::<gtk::Label>(&app.ui, "page_title").text(),
        "Streaming"
    );
    assert!(object::<gtk::Box>(&app.ui, "spotify_tabs").is_visible());
    assert!(!object::<gtk::Box>(&app.ui, "youtube_tabs").is_visible());
    settle();
    screenshot(app.window.upcast_ref(), "spotify-disconnected-compact.png");
    let stack: gtk::Stack = object(&app.ui, "content_stack");
    for id in ["empty_title", "empty_description", "empty_action"] {
        let bounds = object::<gtk::Widget>(&app.ui, id)
            .compute_bounds(&stack)
            .unwrap();
        assert!(
            bounds.y() >= 0.0 && bounds.y() + bounds.height() <= stack.height() as f32 + 1.0,
            "{id} must be visible without scrolling: {bounds:?}, height {}",
            stack.height()
        );
    }

    app.navigate("library");
    application.activate_action("streaming", None);
    assert_eq!(app.page.borrow().as_str(), "spotify");
    object::<gtk::ToggleButton>(&app.ui, "service_streams").set_active(true);
    assert_eq!(app.page.borrow().as_str(), "streams");
    assert!(!object::<gtk::Box>(&app.ui, "spotify_tabs").is_visible());
    app.navigate("spotify");
    // An offline player/session fixture tests GTK integration without contacting Spotify.
    let spotify_runtime = tokio::runtime::Runtime::new().unwrap();
    let playback = {
        let _entered = spotify_runtime.enter();
        crate::spotify::playback::Playback::new(librespot_core::session::Session::new(
            Default::default(),
            None,
        ))
        .unwrap()
    };
    app.handle_spotify(crate::spotify::Reply::Connected(Ok(Box::new((
        model::SpotifyAccount {
            id: "spotify-fixture".into(),
            name: "Fixture listener".into(),
        },
        playback,
    )))));
    let mut song = Track::new(
        "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
        "Spotify fixture song",
        Kind::Spotify,
    );
    song.artist = "Fixture artist".into();
    song.album = "Fixture album".into();
    song.duration = 45.0;
    let spotify_id = song.id.clone();
    app.spotify.generation.set(3);
    app.handle_spotify(crate::spotify::Reply::Search(2, Ok(vec![song.clone()])));
    assert!(app.spotify.results.borrow().is_empty());
    app.handle_spotify(crate::spotify::Reply::Search(3, Ok(vec![song.clone()])));
    object::<gtk::SearchEntry>(&app.ui, "search")
        .set_text("a provider query need not appear in the result");
    app.refresh();
    assert_eq!(&*app.visible.borrow(), std::slice::from_ref(&spotify_id));
    settle();
    screenshot(app.window.upcast_ref(), "spotify-search-compact.png");
    app.window.set_default_size(1280, 820);
    settle();
    screenshot(app.window.upcast_ref(), "streaming-spotify-desktop.png");
    let remote = crate::spotify::catalog::RemotePlaylist {
        id: "37i9dQZF1DXcBWIGoYBM5M".into(),
        name: "Spotify collection".into(),
        snapshot: youtube_music::snapshot("Spotify collection", std::slice::from_ref(&song.uri)),
        tracks: vec![song],
        editable: true,
        unavailable: 0,
        local_id: None,
        revision: "fixture-revision".into(),
    };
    app.handle_spotify(crate::spotify::Reply::Synced(
        "spotify-fixture".into(),
        vec![],
        Ok(crate::spotify::catalog::SyncResult {
            playlists: vec![remote],
            ..Default::default()
        }),
    ));
    object::<gtk::ToggleButton>(&app.ui, "spotify_playlists").set_active(true);
    assert_eq!(app.playlists_model.n_items(), 1);
    settle();
    screenshot(app.window.upcast_ref(), "spotify-playlists-desktop.png");
    app.window.set_default_size(820, 620);
    settle();
    screenshot(app.window.upcast_ref(), "spotify-playlists-compact.png");
    let playlist_scroller = object::<gtk::GridView>(&app.ui, "playlist_grid")
        .parent()
        .unwrap();
    for (_, ui) in app.playlist_cards.borrow().values() {
        for id in ["title", "artist", "menu"] {
            let bounds = object::<gtk::Widget>(ui, id)
                .compute_bounds(&playlist_scroller)
                .unwrap();
            assert!(
                bounds.y() >= 0.0
                    && bounds.y() + bounds.height() <= playlist_scroller.height() as f32 + 1.0,
                "Spotify card {id} should fit in the compact viewport"
            );
        }
    }

    let playlist_id = app.playlists_model.string(0).unwrap();
    app.open_playlist(&playlist_id);
    assert_eq!(app.page.borrow().as_str(), "spotify");
    app.remove(&spotify_id);
    assert!(
        app.state
            .borrow()
            .playlist(&playlist_id)
            .unwrap()
            .tracks
            .is_empty()
    );
    assert!(app.state.borrow().track(&spotify_id).is_some());
    app.browse_back();
    object::<gtk::ToggleButton>(&app.ui, "spotify_songs").set_active(true);
    app.handle_spotify(crate::spotify::Reply::Search(
        3,
        Err("Offline Spotify fixture".into()),
    ));
    app.spotify.results.borrow_mut().clear();
    app.refresh();
    assert_eq!(
        object::<gtk::Label>(&app.ui, "empty_title").text(),
        "Spotify unavailable"
    );
    app.spotify_primary();
    settle();
    let account = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.title().as_deref() == Some("Spotify account"))
        .unwrap();
    screenshot(&account, "spotify-account.png");
    account.close();
    // A failed Spotify reconnect must not cancel a newer YouTube resolution.
    app.play_generation.set(200);
    app.resolving.set(true);
    app.spotify.pending_play.set(Some(199));
    app.handle_spotify(crate::spotify::Reply::Connected(Err(
        "Expired fixture sign-in".into(),
    )));
    assert!(app.resolving.get());
    app.spotify.pending_play.set(Some(200));
    app.handle_spotify(crate::spotify::Reply::Connected(Err(
        "Expired fixture sign-in".into(),
    )));
    assert!(!app.resolving.get());
    // Disconnect removes only Spotify material and leaves local playback usable.
    app.audio.borrow_mut().detach_spotify();
    app.handle_spotify(crate::spotify::Reply::Disconnected(Ok(())));
    assert!(app.state.borrow().spotify_account.is_none());
    assert!(app.state.borrow().track(&spotify_id).is_none());
    app.play_track("test003", Some(0.0));
    settle();
    assert!(app.audio.borrow().playing);
    // Exercise both real HTTP clients against local fixtures and the actual detail/settings UI.
    let mut routes = enrichment::tests::artwork_routes();
    routes.push((
        "/lyrics/get".into(),
        200,
        String::new(),
        serde_json::to_vec(&enrichment::tests::lyric()).unwrap(),
    ));
    let metadata_fixture = enrichment::tests::Fixture::new(routes);
    let metadata_cache = directory.path().join("metadata");
    *app.metadata.borrow_mut() = enrichment::Service::new(metadata_fixture.config(&metadata_cache));
    let mut song = enrichment::tests::song();
    song.uri = uri.clone();
    song.id = "metadatafixture".into();
    let mut sibling = song.clone();
    sibling.id = "metadatasibling".into();
    sibling.title = "Another original song".into();
    {
        let mut state = app.state.borrow_mut();
        state.merge(vec![song, sibling]);
        state.fetch_covers = true;
        state.fetch_lyrics = true;
        state.queue = vec!["metadatafixture".into(), "metadatasibling".into()];
    }
    app.navigate("queue");
    app.play_track("metadatafixture", None);
    assert_eq!(
        object::<gtk::Label>(&app.ui, "lyrics_text").text(),
        "Finding lyrics…"
    );
    assert!(!app.button("lyrics_retry").is_sensitive());
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until
        && app
            .state
            .borrow()
            .track("metadatafixture")
            .unwrap()
            .artwork
            .is_none()
    {
        settle();
    }
    settle();
    {
        let state = app.state.borrow();
        let track = state.track("metadatafixture").unwrap();
        assert!(track.artwork_source.is_some());
        assert!(track.lyrics_source.is_some());
        assert!(PathBuf::from(track.artwork.as_ref().unwrap()).is_file());
        assert_eq!(
            state.track("metadatasibling").unwrap().artwork,
            track.artwork
        );
        assert!(
            state.track("metadatasibling").unwrap().lyrics.is_none(),
            "Only the current track needs lyrics"
        );
    }
    assert_eq!(
        object::<gtk::Label>(&app.ui, "lyrics_text").text(),
        enrichment::tests::lyric()["plainLyrics"].as_str().unwrap()
    );
    if !object::<gtk::Revealer>(&app.ui, "detail_revealer").is_visible() {
        app.toggle_details();
    }
    assert!(object::<gtk::LinkButton>(&app.ui, "lyrics_source").is_visible());
    assert!(!app.button("lyrics_retry").is_visible());
    assert!(
        app.audio.borrow().playing,
        "Metadata lookup must not interrupt playback"
    );
    app.window.set_default_size(1280, 820);
    if !object::<gtk::Revealer>(&app.ui, "detail_revealer").is_visible() {
        app.toggle_details();
    }
    settle();
    screenshot(app.window.upcast_ref(), "metadata-desktop.png");
    object::<gtk::ScrolledWindow>(&app.ui, "sidebar_scroller").set_visible(false);
    app.window.set_default_size(820, 620);
    settle();
    // Continue the resize after GTK has remeasured the compact columns, like a drag.
    app.window.set_default_size(800, 620);
    settle();
    screenshot(app.window.upcast_ref(), "metadata-compact.png");
    assert!(
        app.window.width() <= 820,
        "Compact width {}; content {:?}; panel {:?}; player {:?}; header {:?}",
        app.window.width(),
        object::<gtk::Widget>(&app.ui, "content").measure(gtk::Orientation::Horizontal, -1),
        object::<gtk::Widget>(&app.ui, "detail_panel").measure(gtk::Orientation::Horizontal, -1),
        object::<gtk::Widget>(&app.ui, "player_bar").measure(gtk::Orientation::Horizontal, -1),
        object::<gtk::Widget>(&app.ui, "track_header").measure(gtk::Orientation::Horizontal, -1),
    );
    let details: gtk::ScrolledWindow = object(&app.ui, "detail_scroller");
    details
        .vadjustment()
        .set_value(details.vadjustment().upper());
    settle();
    let bounds = object::<gtk::Label>(&app.ui, "lyrics_text")
        .compute_bounds(&details)
        .unwrap();
    assert!(
        bounds.y() + bounds.height() <= details.height() as f32 + 1.0,
        "Lyrics should be reachable by scrolling"
    );

    // Replacing a downloaded lyric with a local sidecar must also remove provider attribution.
    let local = audio.with_extension("lrc");
    std::fs::write(&local, "[00:00.00]My local fixture lyric").unwrap();
    let fresh = library::scan(
        std::slice::from_ref(&directory.path().to_path_buf()),
        &[],
        &directory.path().join("scan-covers"),
    )
    .unwrap();
    assert_eq!(
        fresh.tracks[0].lyrics.as_deref(),
        Some("[00:00.00]My local fixture lyric")
    );
    assert!(fresh.tracks[0].lyrics_source.is_none());

    app.settings();
    settle();
    let settings = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.title().as_deref() == Some("Settings"))
        .unwrap();
    screenshot(&settings, "metadata-settings.png");
    let covers = descendant(settings.upcast_ref(), "fetch_covers")
        .unwrap()
        .downcast::<gtk::Switch>()
        .unwrap();
    let lyrics = descendant(settings.upcast_ref(), "fetch_lyrics")
        .unwrap()
        .downcast::<gtk::Switch>()
        .unwrap();
    covers.set_active(false);
    lyrics.set_active(false);
    assert!(!app.state.borrow().fetch_covers && !app.state.borrow().fetch_lyrics);
    // Cached content remains available while lookups are switched off.
    assert!(
        object::<gtk::Label>(&app.ui, "lyrics_text")
            .text()
            .contains("A quiet window")
    );
    settings.close();

    // Missing and failed results remain local to the metadata panel and offer retry.
    let offline = enrichment::tests::Fixture::new(vec![(
        "/lyrics/get".into(),
        503,
        "Retry-After: 60\r\n".into(),
        b"Busy".to_vec(),
    )]);
    *app.metadata.borrow_mut() =
        enrichment::Service::new(offline.config(&directory.path().join("offline-metadata")));
    app.state.borrow_mut().fetch_lyrics = true;
    app.play_track("metadatasibling", None);
    settle();
    assert!(
        object::<gtk::Label>(&app.ui, "lyrics_text")
            .text()
            .contains("provider is busy")
    );
    assert!(app.button("lyrics_retry").is_visible());
    assert!(!app.button("lyrics_retry").is_sensitive());
    screenshot(app.window.upcast_ref(), "metadata-busy-compact.png");
    assert!(app.audio.borrow().playing);
    let missing = enrichment::tests::Fixture::new(vec![(
        "/lyrics/search".into(),
        200,
        String::new(),
        b"[]".to_vec(),
    )]);
    *app.metadata.borrow_mut() =
        enrichment::Service::new(missing.config(&directory.path().join("missing-metadata")));
    app.schedule_metadata();
    settle();
    assert!(
        object::<gtk::Label>(&app.ui, "lyrics_text")
            .text()
            .contains("No matching lyrics")
    );
    assert!(app.button("lyrics_retry").is_sensitive());
    let requests = missing.count();
    app.button("lyrics_retry").emit_clicked();
    settle();
    assert!(
        missing.count() > requests,
        "Retry bypasses a negative cache entry"
    );
    screenshot(app.window.upcast_ref(), "metadata-missing-compact.png");
    // Mouse-wheel controls use fixed musical steps and clamp at the ends.
    app.play_track("metadatafixture", None);
    settle();
    app.set_volume(0.5);
    let volume = scroll_controller(
        &object::<gtk::Widget>(&app.ui, "volume_box"),
        "volume-wheel",
    );
    assert_eq!(volume.propagation_phase(), gtk::PropagationPhase::Capture);
    assert!(volume.emit_by_name::<bool>("scroll", &[&0.0_f64, &-1.0_f64]));
    assert!((app.state.borrow().volume - 0.55).abs() < 0.001);
    let seek: gtk::Scale = object(&app.ui, "seek");
    seek.set_value(12.0);
    let wheel = scroll_controller(seek.upcast_ref(), "seek-wheel");
    assert!(wheel.emit_by_name::<bool>("scroll", &[&0.0_f64, &1.0_f64]));
    assert!((app.state.borrow().position - 7.0).abs() < 0.01);
    assert!(wheel.emit_by_name::<bool>("scroll", &[&0.0_f64, &100.0_f64]));
    assert_eq!(app.state.borrow().position, 0.0);

    // Repeated completed seeks leave artwork, list geometry and scroll untouched.
    app.state.borrow_mut().fetch_covers = false;
    app.state.borrow_mut().fetch_lyrics = false;
    app.state
        .borrow_mut()
        .tracks
        .iter_mut()
        .find(|t| t.id == "metadatafixture")
        .unwrap()
        .artwork = Some(cover_paths[0].clone());
    app.update_player();
    app.navigate("library");
    object::<gtk::ToggleButton>(&app.ui, "tracks_view").set_active(true);
    object::<gtk::Box>(&app.ui, "notice").set_visible(false);
    settle();
    let player_art: gtk::Picture = object(&app.ui, "player_art");
    assert!(player_art.paintable().is_some());
    let repaints = Rc::new(Cell::new(0));
    let count = repaints.clone();
    let paint_handler = player_art.connect_paintable_notify(move |_| count.set(count.get() + 1));
    scroller.vadjustment().set_value(1000.0);
    settle();
    let scroll_before = scroller.vadjustment().value();
    let height_before = scroller.height();
    for dy in [-1.0_f64, 1.0, -1.0, 1.0] {
        assert!(wheel.emit_by_name::<bool>("scroll", &[&0.0_f64, &dy]));
        app.set_buffering(20);
        app.update_buffering_notice();
        app.set_buffering(100);
        settle();
        assert!(!object::<gtk::Box>(&app.ui, "notice").is_visible());
        assert_eq!(scroller.height(), height_before);
        assert!((scroller.vadjustment().value() - scroll_before).abs() < 1.0);
    }
    assert_eq!(repaints.get(), 0, "Seeking must not replace player artwork");
    player_art.disconnect(paint_handler);
    app.set_buffering(30);
    app.buffering_since
        .set(Some(Instant::now() - Duration::from_secs(1)));
    app.update_buffering_notice();
    assert!(
        object::<gtk::Box>(&app.ui, "notice").is_visible(),
        "Sustained buffering still needs feedback"
    );
    app.set_buffering(100);
    assert!(!object::<gtk::Box>(&app.ui, "notice").is_visible());
    app.set_buffering(30);
    app.play_track("metadatafixture", Some(0.0));
    assert!(
        app.buffering_since.get().is_none(),
        "A new track must clear old buffering state"
    );
    app.notice("Keep this unrelated status", false, false);
    app.set_buffering(100);
    assert!(
        object::<gtk::Box>(&app.ui, "notice").is_visible(),
        "Buffering must not dismiss another operation's status"
    );
    object::<gtk::Box>(&app.ui, "notice").set_visible(false);

    let feed = b"<rss><channel><title>Auto refresh fixture</title><item><title>Original fixture episode</title><guid>fixture-episode</guid><enclosure url='https://example.org/audio.mp3' type='audio/mpeg'/></item></channel></rss>".to_vec();
    let feeds = enrichment::tests::Fixture::new(vec![("/feed".into(), 200, String::new(), feed)]);
    let url = format!("{}feed", feeds.url);
    let show_id = model::stable_id(&url);
    app.podcast_failed.set(false);
    app.podcast_results.borrow_mut().clear();
    app.podcast_results
        .borrow_mut()
        .push(services::PodcastShow {
            id: show_id.clone(),
            title: "Auto refresh fixture".into(),
            author: "Fixture creator".into(),
            feed_url: url.clone(),
            artwork: None,
            website: None,
        });
    app.navigate("podcasts");
    object::<gtk::ToggleButton>(&app.ui, "podcast_discover").set_active(true);
    app.refresh();
    settle();
    app.subscribe_show(&show_id);
    settle();
    assert!(
        app.state
            .borrow()
            .subscriptions
            .iter()
            .any(|s| s.url == url)
    );
    let card = app
        .podcast_cards
        .borrow()
        .values()
        .find_map(|(item, ui)| {
            item.upgrade()
                .and_then(|item| item.item())
                .map(|_| ui.clone())
        })
        .unwrap();
    assert_eq!(
        object::<gtk::Button>(&card, "subscribe").label().as_deref(),
        Some("Unsubscribe")
    );
    screenshot(app.window.upcast_ref(), "podcast-unsubscribe.png");
    let episodes = app
        .state
        .borrow()
        .tracks
        .iter()
        .filter(|t| t.kind == Kind::Podcast)
        .count();
    object::<gtk::Button>(&card, "subscribe").emit_clicked();
    assert!(
        !app.state
            .borrow()
            .subscriptions
            .iter()
            .any(|s| s.url == url)
    );
    assert_eq!(
        app.state
            .borrow()
            .tracks
            .iter()
            .filter(|t| t.kind == Kind::Podcast)
            .count(),
        episodes
    );
    assert_eq!(
        object::<gtk::Button>(&card, "subscribe").label().as_deref(),
        Some("Subscribe")
    );
    app.subscribe_show(&show_id);
    settle();
    app.refresh_podcasts(true);
    app.unsubscribe(&url);
    settle();
    assert!(
        !app.state
            .borrow()
            .subscriptions
            .iter()
            .any(|s| s.url == url),
        "A late refresh must not re-subscribe a show"
    );
    app.subscribe_show(&show_id);
    settle();

    app.navigate("queue");
    app.state.borrow_mut().folders = vec![directory.path().to_path_buf()];
    let auto_file = directory.path().join("auto-new.wav");
    std::fs::copy(&audio, &auto_file).unwrap();
    let auto_uri = url::Url::from_file_path(&auto_file).unwrap().to_string();
    let current = app.state.borrow().current.clone();
    let requests = feeds.count();
    app.state.borrow_mut().refresh_minutes = 0;
    app.refresh_timer
        .set(Instant::now() - Duration::from_secs(61));
    app.poll_source_refresh();
    assert!(!app.scanning.get());
    assert_eq!(feeds.count(), requests);
    app.state.borrow_mut().refresh_minutes = 1;
    app.poll_source_refresh();
    assert!(app.scanning.get());
    assert!(app.podcast_pending(&url));
    app.poll_source_refresh();
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until && app.scanning.get() {
        settle();
    }
    settle();
    assert!(app.state.borrow().tracks.iter().any(|t| t.uri == auto_uri));
    assert_eq!(
        feeds.count(),
        requests + 1,
        "The refresh must not overlap its previous requests"
    );
    assert_eq!(app.state.borrow().current, current);
    app.state.borrow_mut().refresh_minutes = 0;
    app.settings();
    settle();
    let settings = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.title().as_deref() == Some("Settings"))
        .unwrap();
    let interval = descendant(settings.upcast_ref(), "refresh_minutes")
        .unwrap()
        .downcast::<gtk::SpinButton>()
        .unwrap();
    interval.set_value(45.0);
    assert_eq!(app.state.borrow().refresh_minutes, 45);
    assert!(app.refresh_timer.get().elapsed() < Duration::from_secs(1));
    settle();
    screenshot(&settings, "refresh-settings.png");
    interval.set_value(0.0);
    let gapless = descendant(settings.upcast_ref(), "gapless")
        .unwrap()
        .downcast::<gtk::Switch>()
        .unwrap();
    let crossfade = descendant(settings.upcast_ref(), "crossfade_seconds")
        .unwrap()
        .downcast::<gtk::SpinButton>()
        .unwrap();
    let gain = descendant(settings.upcast_ref(), "replay_gain")
        .unwrap()
        .downcast::<gtk::DropDown>()
        .unwrap();
    assert!(gapless.is_active());
    assert_eq!(crossfade.value_as_int(), 0);
    assert_eq!(gain.selected(), 0);
    gapless.set_active(false);
    crossfade.set_value(6.0);
    gain.set_selected(2);
    assert_eq!(
        app.state.borrow().playback,
        model::PlaybackSettings {
            gapless: false,
            crossfade_seconds: 6,
            replay_gain: model::ReplayGain::Album
        }
    );
    settings.close();
    app.settings();
    settle();
    let settings = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.title().as_deref() == Some("Settings"))
        .unwrap();
    let crossfade = descendant(settings.upcast_ref(), "crossfade_seconds")
        .unwrap()
        .downcast::<gtk::SpinButton>()
        .unwrap();
    assert_eq!(crossfade.value_as_int(), 6);
    settings
        .child()
        .unwrap()
        .downcast::<gtk::ScrolledWindow>()
        .unwrap()
        .vadjustment()
        .set_value(450.0);
    settle();
    screenshot(&settings, "playback-settings.png");
    settings.close();
    app.shutdown();
    drop(spotify_runtime);
    app.window.destroy();
    object::<gtk::Window>(&app.mini_ui, "mini_window").destroy();
}

fn screenshot(window: &gtk::Window, name: &str) {
    let Some(directory) = std::env::var_os("OPEN_AUDIO_TEST_SCREENSHOTS") else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
    let node = snapshot.to_node().unwrap();
    window
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png(PathBuf::from(directory).join(name))
        .unwrap();
}

fn descendant(widget: &gtk::Widget, id: &str) -> Option<gtk::Widget> {
    if widget.buildable_id().as_deref() == Some(id) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = descendant(&current, id) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

fn scroll_controller(widget: &gtk::Widget, name: &str) -> gtk::EventControllerScroll {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerScroll>()
        })
        .find(|c| c.name().as_deref() == Some(name))
        .unwrap()
}
