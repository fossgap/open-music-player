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
    let mut state = State::default();
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
    app.navigate("playlists");
    settle();
    let playlists: gtk::GridView = object(&app.ui, "playlist_grid");
    assert!(playlists.is_single_click_activate());
    assert_eq!(app.playlists_model.n_items(), 2);
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
