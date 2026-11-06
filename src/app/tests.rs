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
    app.shutdown();
    app.window.destroy();
    object::<gtk::Window>(&app.mini_ui, "mini_window").destroy();
}
