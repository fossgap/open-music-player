use super::super::tests::{screenshot, settle};
use super::*;
use crate::enrichment::tests::Fixture;
use std::os::unix::fs::PermissionsExt;

fn until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !condition() && Instant::now() < deadline {
        settle();
    }
    assert!(condition(), "Global search operation timed out");
}

pub(crate) fn exercise(app: &Rc<App>, tone: &std::path::Path) {
    let before = app.state.borrow().clone();
    let page = app.page.borrow().clone();
    let old_albums = object::<gtk::ToggleButton>(&app.ui, "albums_view").is_active();
    let sidebar: gtk::ScrolledWindow = object(&app.ui, "sidebar_scroller");
    let old_sidebar = sidebar.is_visible();
    sidebar.set_visible(true);
    let dir = tempfile::tempdir().unwrap();
    // Synthetic solid-color artwork exercises real image loading without external assets.
    let cover =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 64, 64).unwrap();
    cover.fill(0x608599ff);
    let bytes = cover.save_to_bufferv("png", &[]).unwrap();
    let cover_path = dir.path().join("cover.png");
    std::fs::write(&cover_path, &bytes).unwrap();
    let feed = Fixture::new(vec![("/feed".into(), 200, String::new(),
        b"<rss><channel><title>Night stories</title><item><title>Night episode</title><enclosure url='http://127.0.0.1/episode.mp3' type='audio/mpeg'/></item></channel></rss>".to_vec()),
        ("/cover.png".into(), 200, "Content-Type: image/png\r\n".into(), bytes)]);
    let feed_url = format!("{}feed", feed.url);
    let cover_url = format!("{}cover.png", feed.url);
    let catalog = Fixture::new(vec![
        ("/search".into(), 200, String::new(), serde_json::to_vec(&serde_json::json!({"results":[{"feedUrl":feed_url,"collectionName":"Night stories","artistName":"Open Audio fixture","artworkUrl600":cover_url}]})).unwrap()),
        ("/search.json".into(), 503, String::new(), b"Directory temporarily unavailable".to_vec()),
        ("/json/stations/search".into(), 200, String::new(), br#"[{"name":"Night radio","url_resolved":"https://example.test/night-radio","stationuuid":"night-fixture","country":"Test country","tags":"ambient,jazz"}]"#.to_vec()),
    ]);
    let python = dir.path().join("search-python");
    let reply = serde_json::json!({"ok":true,"data":[{"video_id":"abcdefghijk","title":"Night online","artist":"Fixture ensemble","album":"Moonlight","duration":60,"artwork":cover_url}]});
    std::fs::write(
        &python,
        format!(
            "#!/usr/bin/env python3\nimport json,sys\njson.load(sys.stdin)\nprint({:?})\n",
            reply.to_string()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
    let variables = [
        "OPEN_AUDIO_ITUNES_API",
        "OPEN_AUDIO_GPODDER_API",
        "OPEN_AUDIO_RADIO_API",
        "OPEN_AUDIO_PYTHON",
    ];
    let saved_env: Vec<_> = variables.iter().map(std::env::var_os).collect();
    unsafe {
        for key in &variables[..3] {
            std::env::set_var(key, &catalog.url);
        }
        std::env::set_var("OPEN_AUDIO_PYTHON", &python);
    }
    let uri = gio::File::for_path(tone).uri().to_string();
    let mut song = Track::new(&uri, "Night song", Kind::Music);
    song.id = "globalLocal".into();
    song.artist = "Björk".into();
    song.album = "Night album".into();
    song.duration = 30.0;
    song.artwork = Some(cover_path.to_string_lossy().into());
    let mut stream = Track::new(
        "https://example.test/night-stream",
        "Night stream",
        Kind::Stream,
    );
    stream.id = "globalStream".into();
    app.state.borrow_mut().merge(vec![song, stream]);
    let playlist = app
        .state
        .borrow_mut()
        .create_playlist("Night playlist", vec!["globalLocal".into()])
        .unwrap();
    app.play_track("globalLocal", None);
    app.pause_playback();
    let playing = app.state.borrow().current.clone();
    let queue = app.state.borrow().queue.clone();
    app.navigate("search");
    let query: gtk::SearchEntry = object(&app.search.ui, "global_query");
    query.set_text("night");
    until(|| {
        !app.search.local_busy.get() && app.search.rows.borrow().contains_key("track:globalLocal")
    });
    assert!(
        catalog.requests.lock().unwrap().is_empty(),
        "Typing must not contact directories"
    );
    assert_eq!(app.state.borrow().current, playing);
    assert_eq!(app.state.borrow().queue, queue);
    assert!(!app.audio.borrow().playing);
    let list: gtk::ListView = object(&app.search.ui, "global_results");
    let selection = list
        .model()
        .unwrap()
        .downcast::<gtk::SingleSelection>()
        .unwrap();
    let position = (0..app.search.model.n_items())
        .find(|&p| app.search.model.string(p).as_deref() == Some("track:globalLocal"))
        .unwrap();
    selection.set_selected(position);
    settle();
    assert!(
        !app.audio.borrow().playing,
        "Selection must not start playback"
    );
    app.window.set_default_size(1240, 800);
    settle();
    screenshot(app.window.upcast_ref(), "global-search-local.png");
    query.emit_activate();
    until(|| {
        app.search
            .sections
            .borrow()
            .values()
            .all(|s| !matches!(s.status, Status::Loading))
    });
    assert!(matches!(
        app.search.sections.borrow()[&Group::Apple].status,
        Status::Ready
    ));
    assert!(matches!(
        app.search.sections.borrow()[&Group::Gpodder].status,
        Status::Failed(_)
    ));
    assert!(matches!(
        app.search.sections.borrow()[&Group::Radio].status,
        Status::Ready
    ));
    assert!(matches!(
        app.search.sections.borrow()[&Group::Youtube].status,
        Status::Ready
    ));
    assert!(matches!(
        app.search.sections.borrow()[&Group::Spotify].status,
        Status::Disconnected
    ));
    until(|| app.search.artwork.borrow().contains_key(&cover_url));
    assert_eq!(
        feed.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.contains("GET /cover.png "))
            .count(),
        1,
        "Shared covers must be downloaded only once"
    );
    let youtube_id = app.search.sections.borrow()[&Group::Youtube].items[0]
        .key
        .clone();
    assert!(
        app.state
            .borrow()
            .tracks
            .iter()
            .all(|t| t.title != "Night online"),
        "Discovery results remain ephemeral until used"
    );
    let filter: gtk::DropDown = object(&app.search.ui, "global_filter");
    filter.set_selected(4);
    settle();
    screenshot(app.window.upcast_ref(), "global-search-online.png");
    filter.set_selected(1);
    app.window.set_default_size(780, 640);
    settle();
    screenshot(app.window.upcast_ref(), "global-search-compact.png");
    app.state.borrow_mut().theme = model::Theme::System;
    app.apply_theme();
    app.theme.borrow_mut().system_scheme(2);
    settle();
    screenshot(app.window.upcast_ref(), "global-search-system.png");
    app.state.borrow_mut().theme = model::Theme::Sage;
    app.apply_theme();
    filter.set_selected(0);
    settle();
    let show_key = app.search.sections.borrow()[&Group::Apple].items[0]
        .key
        .clone();
    app.act_global(&show_key, 0);
    until(|| {
        matches!(
            app.search
                .sections
                .borrow()
                .get(&Group::Feed)
                .map(|s| &s.status),
            Some(Status::Ready)
        )
    });
    assert!(
        app.state
            .borrow()
            .subscriptions
            .iter()
            .all(|s| s.url != feed_url)
    );
    assert!(
        app.search
            .rows
            .borrow()
            .values()
            .any(|r| matches!(r, Row::Item(d) if d.title == "Night episode"))
    );
    screenshot(app.window.upcast_ref(), "global-search-podcast.png");
    object::<gtk::Button>(&app.search.ui, "global_subscribe").emit_clicked();
    until(|| {
        app.state
            .borrow()
            .subscriptions
            .iter()
            .any(|s| s.url == feed_url)
    });
    assert_eq!(
        object::<gtk::Button>(&app.search.ui, "global_subscribe")
            .label()
            .as_deref(),
        Some("Unsubscribe")
    );
    object::<gtk::Button>(&app.search.ui, "global_subscribe").emit_clicked();
    assert!(
        app.state
            .borrow()
            .subscriptions
            .iter()
            .all(|s| s.url != feed_url)
    );
    object::<gtk::Button>(&app.search.ui, "global_back").emit_clicked();
    settle();
    app.act_global(&youtube_id, 1);
    assert_eq!(app.state.borrow().current, playing);
    assert!(!app.audio.borrow().playing);
    assert!(app.state.borrow().queue.starts_with(&queue));
    assert_eq!(app.state.borrow().queue.len(), queue.len() + 1);
    app.act_global("track:globalLocal", 0);
    until(|| app.audio.borrow().playing);
    app.pause_playback();
    app.act_global(&format!("playlist:{playlist}"), 2);
    assert_eq!(app.page.borrow().as_str(), "playlists");
    assert_eq!(app.playlist.borrow().as_ref(), Some(&playlist));
    for i in 0..120 {
        let mut track = Track::new(&uri, &format!("Paged result {i:03}"), Kind::Music);
        track.id = format!("globalPaged{i}");
        track.duration = 30.0;
        app.state.borrow_mut().tracks.push(track);
    }
    app.navigate("search");
    filter.set_selected(1);
    query.set_text("paged");
    until(|| {
        app.search
            .matches
            .borrow()
            .get(&Group::Music)
            .is_some_and(|m| m.total == 120)
    });
    assert_eq!(app.search.matches.borrow()[&Group::Music].items.len(), 50);
    let scroll = object::<gtk::ScrolledWindow>(&app.search.ui, "global_scroller").vadjustment();
    selection.set_selected(20);
    scroll.set_value(800.0);
    settle();
    let offset = scroll.value();
    let selected = selection
        .selected_item()
        .and_downcast::<gtk::StringObject>()
        .unwrap()
        .string();
    list.emit_by_name::<()>("activate", &[&20u32]);
    until(|| app.audio.borrow().playing);
    app.pause_playback();
    settle();
    assert!(
        (scroll.value() - offset).abs() < 2.0,
        "Playing a result must preserve search scroll"
    );
    assert_eq!(
        selection
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string(),
        selected
    );
    app.act_global("more", 0);
    until(|| app.search.matches.borrow()[&Group::Music].items.len() == 100);
    settle();
    assert!(
        (scroll.value() - offset).abs() < 2.0,
        "Loading more must preserve search scroll"
    );
    assert_eq!(
        selection
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string(),
        selected
    );
    filter.set_selected(0);
    query.set_text("does not exist");
    settle();
    until(|| !app.search.local_busy.get());
    settle();
    assert!(app.search.rows.borrow().is_empty());
    screenshot(app.window.upcast_ref(), "global-search-empty.png");
    filter.set_selected(2);
    assert!(!object::<gtk::Button>(&app.search.ui, "global_submit").is_sensitive());
    assert_eq!(
        object::<gtk::Label>(&app.search.ui, "global_empty_detail").label(),
        "Try a different search."
    );
    filter.set_selected(0);
    let generation = app.search.service.generation();
    query.set_text("bjork");
    settle();
    app.global_reply(Reply {
        generation,
        group: Group::Radio,
        query: "night".into(),
        result: Ok(Payload::Tracks(vec![])),
    });
    assert!(
        app.search.sections.borrow().is_empty(),
        "Old responses must not change a newer query"
    );
    until(|| !app.search.local_busy.get());
    assert!(app.search.rows.borrow().contains_key("track:globalLocal"));
    app.search.sections.borrow_mut().insert(
        Group::Radio,
        Section {
            status: Status::Loading,
            items: vec![],
        },
    );
    app.cancel_global_search();
    assert!(matches!(
        app.search.sections.borrow()[&Group::Radio].status,
        Status::Cancelled
    ));
    app.stop_playback();
    app.search.sections.borrow_mut().clear();
    query.set_text("");
    *app.state.borrow_mut() = before;
    app.theme.borrow_mut().system_scheme(0);
    app.apply_theme();
    app.navigate(&page);
    object::<gtk::ToggleButton>(
        &app.ui,
        if old_albums {
            "albums_view"
        } else {
            "tracks_view"
        },
    )
    .set_active(true);
    app.refresh();
    app.update_player();
    sidebar.set_visible(old_sidebar);
    unsafe {
        for (key, value) in variables.into_iter().zip(saved_env) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}
