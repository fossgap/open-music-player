use super::*;
use crate::model::{Playlist, Subscription, Track};
use std::time::{Duration, Instant};

fn await_results(search: &LocalSearch) -> Results {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(results) = search.poll() {
            return results;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Local search timed out");
}

#[test]
fn matches_all_saved_audio_and_collections_without_requiring_accounts() {
    let mut state = State::default();
    for (index, kind) in [
        Kind::Music,
        Kind::Podcast,
        Kind::Radio,
        Kind::Stream,
        Kind::YoutubeMusic,
        Kind::Spotify,
    ]
    .into_iter()
    .enumerate()
    {
        let mut track = Track::new(
            &format!("https://example.test/{index}"),
            "Café sessions",
            kind,
        );
        track.artist = "Björk".into();
        track.album = "Evening".into();
        track.album_artist = "Various Artists".into();
        track.source = "https://example.test/feed".into();
        state.tracks.push(track);
    }
    state.playlists.push(Playlist {
        id: "list".into(),
        name: "Café collection".into(),
        tracks: vec![],
        youtube: None,
        spotify: None,
    });
    state.subscriptions.push(Subscription {
        url: "https://example.test/feed".into(),
        title: "Café talks".into(),
    });
    state.sanitize();
    let docs = snapshot(&state);
    let search = LocalSearch::default();
    search.search(docs.clone(), "cafe".into(), 50);
    let results = await_results(&search);
    assert_eq!(results[&Group::Music].total, 3);
    for group in [
        Group::Episodes,
        Group::Stations,
        Group::Streams,
        Group::Playlists,
        Group::Subscriptions,
    ] {
        assert_eq!(results[&group].total, 1, "{group:?}");
    }
    search.search(docs.clone(), "evening various".into(), 50);
    assert_eq!(
        await_results(&search)[&Group::Albums].items[0].title,
        "Evening"
    );
    search.search(docs.clone(), "bjork cafe".into(), 50);
    assert_eq!(await_results(&search)[&Group::Music].total, 3);
    search.search(docs, String::new(), 50);
    assert!(await_results(&search).is_empty());
}

#[test]
fn exact_titles_rank_first_and_paging_keeps_total_counts() {
    let mut state = State::default();
    for (id, title, artist) in [
        ("a", "Elsewhere", "Night"),
        ("b", "Night walk", "A"),
        ("c", "Night", "Z"),
    ] {
        let mut track = Track::new(id, title, Kind::Music);
        track.artist = artist.into();
        state.tracks.push(track);
    }
    let search = LocalSearch::default();
    let docs = snapshot(&state);
    search.search(docs.clone(), "night".into(), 2);
    let results = await_results(&search);
    let music = &results[&Group::Music];
    assert_eq!(music.total, 3);
    assert_eq!(music.items.len(), 2);
    assert_eq!(music.items[0].title, "Night");
    assert_eq!(music.items[1].title, "Night walk");
    search.search(docs, "night".into(), 5);
    assert_eq!(await_results(&search)[&Group::Music].items.len(), 3);
}

#[test]
fn newer_queries_and_cancellation_discard_obsolete_results() {
    let mut state = State::default();
    for i in 0..5000 {
        state.tracks.push(Track::new(
            &format!("file:///music/{i}.mp3"),
            &format!("Track {i}"),
            Kind::Music,
        ));
    }
    let search = LocalSearch::default();
    let docs = snapshot(&state);
    search.search(docs.clone(), "track".into(), 100);
    search.search(docs.clone(), "track 4999".into(), 50);
    let result = await_results(&search);
    assert_eq!(result[&Group::Music].total, 1);
    assert_eq!(result[&Group::Music].items[0].title, "Track 4999");
    search.search(docs, "track".into(), 10);
    search.cancel();
    std::thread::sleep(Duration::from_millis(40));
    assert!(search.poll().is_none());
}
