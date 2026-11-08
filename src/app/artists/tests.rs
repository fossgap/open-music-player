use super::*;
use crate::app::tests::{screenshot, settle};
use std::path::Path;

fn track(id: &str, title: &str, artist: &str, album: &str, number: u32) -> Track {
    let mut track = Track::new(&format!("file:///{id}.wav"), title, Kind::Music);
    track.id = id.into();
    track.artist = artist.into();
    track.album = album.into();
    track.number = number;
    track.duration = 30.0;
    track
}

fn library() -> Vec<Track> {
    let first = track("first", "First light", "Ada Vale", "Morning", 1);
    let mut late = track("late", "Night air", "Ada Vale", "Morning", 2);
    late.artists = vec![" Ada Vale ".into(), "ADA VALE".into()];
    let mut duet = track(
        "duet",
        "Together",
        "Ada Vale & Benoît Lune",
        "Compilation",
        1,
    );
    duet.artists = vec!["Ada Vale".into(), "Benoît Lune".into()];
    duet.album_artist = "Various Artists".into();
    let beta = track("beta", "Rivers", "Benoît Lune", "Evening", 1);
    let long = track(
        "long",
        "Drift",
        "Camille and the Very Long Name Ensemble from the Other Side of the City",
        "Faraway",
        1,
    );
    let mut unknown = track("unknown", "Unlabelled", "  ", "Untitled", 1);
    unknown.artists = vec![" ".into()];
    let mut radio = track("radio", "Live radio", "Not a local artist", "", 0);
    radio.kind = Kind::Radio;
    vec![late, duet, beta, unknown, long, first, radio]
}

#[test]
fn artist_groups_deduplicate_credits_and_use_performers_in_compilations() {
    let groups = group_tracks(&library(), "");
    assert_eq!(groups.len(), 4);
    assert_eq!(groups["ada vale"].name, "Ada Vale");
    assert_eq!(groups["ada vale"].tracks, ["first", "late", "duet"]);
    assert_eq!(groups["benoît lune"].tracks, ["beta", "duet"]);
    assert_eq!(groups[""].tracks, ["unknown"]);
    assert!(!groups.contains_key("various artists"));
    assert!(!groups.contains_key("not a local artist"));
}

#[test]
fn artist_search_keeps_full_collections_and_orders_discs_before_track_numbers() {
    let mut tracks = library();
    tracks.iter_mut().find(|t| t.id == "first").unwrap().disc = 2;
    tracks.iter_mut().find(|t| t.id == "late").unwrap().artwork = Some("cover.png".into());
    let groups = group_tracks(&tracks, "Together");
    assert_eq!(groups["ada vale"].tracks, ["late", "first", "duet"]);
    assert_eq!(groups["ada vale"].artwork.as_deref(), Some("cover.png"));
    assert!(groups["ada vale"].matches && groups["benoît lune"].matches);
    assert!(!groups[""].matches);
    let groups = group_tracks(&tracks, "BENOÎT");
    assert!(groups["benoît lune"].matches);
    assert!(
        group_tracks(&tracks, "no such performer")
            .values()
            .all(|a| !a.matches)
    );
}

pub(crate) fn exercise(app: &Rc<App>, audio: &Path) {
    let before = app.state.borrow().clone();
    let size = (app.window.width(), app.window.height());
    let directory = tempfile::tempdir().unwrap();
    let cover = directory.path().join("morning.svg");
    std::fs::write(&cover, r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect width="256" height="256" fill="#31546d"/><circle cx="128" cy="128" r="85" fill="#9dc6ce"/><circle cx="128" cy="128" r="38" fill="#31546d"/><circle cx="128" cy="128" r="8" fill="#9dc6ce"/></svg>"##).unwrap();
    let mut tracks = library();
    for track in &mut tracks {
        track.uri = url::Url::from_file_path(audio).unwrap().to_string();
    }
    tracks.iter_mut().find(|t| t.id == "first").unwrap().artwork =
        Some(cover.to_string_lossy().into_owned());
    *app.state.borrow_mut() = State {
        tracks,
        fetch_covers: false,
        fetch_lyrics: false,
        fetch_information: false,
        ..State::default()
    };
    app.apply_theme();
    app.window.set_default_size(1240, 800);
    app.application.activate_action("artists", None);
    settle();
    let grid: gtk::GridView = object(&app.ui, "artist_grid");
    let search: gtk::SearchEntry = object(&app.ui, "search");
    let stack: gtk::Stack = object(&app.ui, "content_stack");
    let index = |id: &str| {
        (0..app.artists.model.n_items())
            .find(|&i| app.artists.model.string(i).as_deref() == Some(id))
            .unwrap()
    };
    let card = |id: &str| {
        app.artists
            .cards
            .borrow()
            .values()
            .find_map(|(item, ui)| {
                item.upgrade()
                    .and_then(|item| item.item())
                    .and_downcast::<gtk::StringObject>()
                    .filter(|value| value.string() == id)
                    .map(|_| ui.clone())
            })
            .unwrap()
    };
    assert_eq!(app.artists.model.n_items(), 4);
    assert_eq!(stack.visible_child_name().as_deref(), Some("artists"));
    assert!(!grid.is_single_click_activate());
    assert_eq!(
        object::<gtk::Label>(&card("ada vale"), "artist").text(),
        "3 tracks"
    );
    assert_eq!(
        object::<gtk::Label>(&card(""), "title").text(),
        "Unknown artist"
    );
    grid.model().unwrap().select_item(index("ada vale"), true);
    settle();
    assert!(app.artists.selected.borrow().is_none());
    assert!(app.state.borrow().current.is_none());
    object::<gtk::Button>(&card("ada vale"), "card_play").grab_focus();
    settle();
    screenshot(app.window.upcast_ref(), "artists-desktop.png");

    search.set_text("Morning");
    settle();
    assert_eq!(app.artists.model.n_items(), 1);
    grid.emit_by_name::<()>("activate", &[&0_u32]);
    settle();
    assert_eq!(stack.visible_child_name().as_deref(), Some("tracks"));
    assert_eq!(*app.visible.borrow(), ["first", "late", "duet"]);
    assert!(search.text().is_empty());
    assert!(app.button("browse_back").is_visible());
    assert!(!object::<gtk::Box>(&app.ui, "view_switch").is_visible());
    let list: gtk::ListView = object(&app.ui, "track_list");
    list.model().unwrap().select_item(0, true);
    app.refresh();
    assert_eq!(app.selected_rows(), [0]);
    screenshot(app.window.upcast_ref(), "artist-tracks.png");
    search.set_text("Night");
    settle();
    assert_eq!(*app.visible.borrow(), ["late"]);
    app.button("browse_back").emit_clicked();
    settle();
    assert_eq!(search.text(), "Morning");
    assert_eq!(stack.visible_child_name().as_deref(), Some("artists"));
    assert!(app.selected_rows().is_empty());

    search.set_text("Together");
    settle();
    assert_eq!(app.artists.model.n_items(), 2);
    app.button("play_all").emit_clicked();
    settle();
    let queue = app.state.borrow().queue.clone();
    assert_eq!(queue.len(), 4);
    assert_eq!(
        queue.iter().collect::<HashSet<_>>().len(),
        4,
        "Collaborations must not repeat in the queue"
    );
    search.set_text("");
    settle();
    object::<gtk::Button>(&card("ada vale"), "card_play").emit_clicked();
    settle();
    assert_eq!(app.state.borrow().queue, ["first", "late", "duet"]);
    assert_eq!(app.state.borrow().current.as_deref(), Some("first"));
    assert_eq!(stack.visible_child_name().as_deref(), Some("artists"));
    grid.emit_by_name::<()>("activate", &[&index("")]);
    settle();
    assert_eq!(
        object::<gtk::Label>(&app.ui, "page_title").text(),
        "Unknown artist"
    );
    assert_eq!(*app.visible.borrow(), ["unknown"]);
    assert_eq!(app.state.borrow().current.as_deref(), Some("first"));
    app.browse_back();
    for (button, view) in [
        ("albums_view", "albums"),
        ("tracks_view", "tracks"),
        ("artists_view", "artists"),
    ] {
        object::<gtk::ToggleButton>(&app.ui, button).set_active(true);
        settle();
        assert_eq!(stack.visible_child_name().as_deref(), Some(view));
    }
    search.set_text("no such performer");
    settle();
    assert_eq!(stack.visible_child_name().as_deref(), Some("empty"));
    assert!(!app.button("play_all").is_sensitive());
    screenshot(app.window.upcast_ref(), "artists-no-matches.png");
    search.set_text("");
    settle();
    app.stop_playback();
    app.state.borrow_mut().theme = model::Theme::System;
    app.apply_theme();
    app.theme.borrow_mut().system_scheme(2);
    settle();
    screenshot(app.window.upcast_ref(), "artists-light.png");
    app.window.set_default_size(820, 620);
    settle();
    let scroller: gtk::ScrolledWindow = object(&app.ui, "artist_scroller");
    scroller.vadjustment().set_value(0.0);
    settle();
    screenshot(app.window.upcast_ref(), "artists-compact-light.png");
    for id in ["", "ada vale"] {
        for label in ["title", "artist"] {
            let bounds = object::<gtk::Label>(&card(id), label)
                .compute_bounds(&scroller)
                .unwrap();
            assert!(
                bounds.y() >= 0.0 && bounds.y() + bounds.height() <= scroller.height() as f32,
                "Artist {id:?} {label} must fit in compact rows: {bounds:?}, viewport height {}",
                scroller.height()
            );
        }
    }
    app.state.borrow_mut().theme = model::Theme::Sage;
    app.apply_theme();
    settle();
    screenshot(app.window.upcast_ref(), "artists-compact.png");
    app.navigate("queue");
    settle();
    let rows = app.track_rows.borrow();
    let row = rows
        .values()
        .find_map(|(item, ui)| {
            item.upgrade()
                .and_then(|item| item.item())
                .map(|_| ui.clone())
        })
        .unwrap();
    assert!(object::<gtk::Image>(&row, "drag_handle").is_visible());
    drop(rows);
    screenshot(app.window.upcast_ref(), "queue-drag-handle.png");

    app.application.activate_action("artists", None);
    app.open_artist("ada vale");
    for track in &mut app.state.borrow_mut().tracks {
        track.artist = track.artist.replace("Ada Vale", "Alma Vale");
        for artist in &mut track.artists {
            if artist.trim().eq_ignore_ascii_case("ada vale") {
                *artist = "Alma Vale".into();
            }
        }
    }
    app.refresh();
    settle();
    assert!(
        app.artists.selected.borrow().is_none(),
        "Renamed artists must return to the overview"
    );
    assert_eq!(stack.visible_child_name().as_deref(), Some("artists"));
    assert!(app.artists.groups.borrow().contains_key("alma vale"));
    app.state.borrow_mut().tracks.clear();
    app.refresh();
    assert_eq!(stack.visible_child_name().as_deref(), Some("empty"));
    assert_eq!(app.artists.model.n_items(), 0);
    assert!(app.button("empty_action").is_visible());

    *app.state.borrow_mut() = before;
    app.theme.borrow_mut().system_scheme(0);
    app.apply_theme();
    app.navigate("library");
    object::<gtk::ToggleButton>(&app.ui, "tracks_view").set_active(true);
    app.window.set_default_size(size.0, size.1);
    app.refresh();
    app.update_player();
    settle();
}
