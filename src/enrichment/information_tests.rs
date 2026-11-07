use super::tests::{Fixture, song};
use super::*;
use serde_json::json;

pub(crate) const ARTIST: &str = "32345678-1234-1234-1234-123456789abc";
pub(crate) const GROUP: &str = "22345678-1234-1234-1234-123456789abc";
const RECORDING: &str = "42345678-1234-1234-1234-123456789abc";

fn route(path: impl Into<String>, value: serde_json::Value) -> (String, u16, String, Vec<u8>) {
    (
        path.into(),
        200,
        String::new(),
        serde_json::to_vec(&value).unwrap(),
    )
}
fn credit() -> serde_json::Value {
    json!([{"name":"The Test Ensemble","artist":{"id":ARTIST,"name":"The Test Ensemble"}}])
}
fn recording() -> serde_json::Value {
    json!({"id":RECORDING,"title":"Evening Window","score":100,"length":30000,"artist-credit":credit(),"releases":[{"title":"Small Hours","status":"Official","release-group":{"id":GROUP}}]})
}
pub(crate) fn routes() -> Vec<(String, u16, String, Vec<u8>)> {
    vec![
        route(
            "/mb/release-group/?artist=32345678-1234-1234-1234-123456789abc&offset=2",
            json!({"release-group-count":3,"release-groups":[{"id":"62345678-1234-1234-1234-123456789abc","title":"After the Rain","first-release-date":"2025-04","primary-type":"EP"}]}),
        ),
        route(
            "/mb/release-group/?artist=32345678-1234-1234-1234-123456789abc&offset=0",
            json!({"release-group-count":3,"release-groups":[{"id":GROUP,"title":"Small Hours","first-release-date":"2020-05-06","primary-type":"Album"},{"id":"52345678-1234-1234-1234-123456789abc","title":"First Light","first-release-date":"2018","primary-type":"Single"}]}),
        ),
        route(
            "/mb/release-group/",
            json!({"count":1,"release-groups":[{"id":GROUP,"score":100,"title":"Small Hours","artist-credit":credit()}]}),
        ),
        route(
            format!("/mb/release-group/{GROUP}"),
            json!({"id":GROUP,"title":"Small Hours","artist-credit":credit(),"first-release-date":"2020-05-06","genres":[{"name":"ambient"}],"relations":[{"type":"wikipedia","url":{"resource":"https://en.wikipedia.org/wiki/Small_Hours"}}]}),
        ),
        route(
            "/mb/artist/",
            json!({"count":1,"artists":[{"id":ARTIST,"score":100,"name":"The Test Ensemble"}]}),
        ),
        route(
            format!("/mb/artist/{ARTIST}"),
            json!({"id":ARTIST,"name":"The Test Ensemble","genres":[{"name":"ambient"}],"relations":[{"type":"wikidata","url":{"resource":"https://www.wikidata.org/wiki/Q12345"}}]}),
        ),
        route(
            "/wikidata",
            json!({"entities":{"Q12345":{"sitelinks":{"enwiki":{"title":"The Test Ensemble"}}}}}),
        ),
        route(
            "/wiki?titles=Small_Hours",
            json!({"query":{"pages":[{"title":"Small Hours","extract":"Small Hours is the ensemble’s first studio album, recorded during a winter residency.","fullurl":"https://en.wikipedia.org/wiki/Small_Hours"}]}}),
        ),
        route(
            "/wiki?titles=The+Test+Ensemble",
            json!({"query":{"pages":[{"title":"The Test Ensemble","extract":"The Test Ensemble is an independent group exploring acoustic instruments and quiet electronic textures.","fullurl":"https://en.wikipedia.org/wiki/The_Test_Ensemble"}]}}),
        ),
        route(
            "/mb/recording/",
            json!({"count":1,"recordings":[recording()]}),
        ),
        route(format!("/mb/recording/{RECORDING}"), recording()),
        route(
            "/mb/release/",
            json!({"release-count":1,"releases":[{"title":"Small Hours","status":"Official","release-group":{"id":GROUP}}]}),
        ),
    ]
}

#[test]
fn information_fetches_attributed_summaries_dates_and_paged_discography() {
    let fixture = Fixture::new(routes());
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    let mut track = song();
    let query = Query::for_track(&track, Field::Information).unwrap();
    let result = client.lookup(&query, false, &|| false).unwrap().unwrap();
    assert!(apply(&mut track, &query.key, &result));
    let info = track.music_info.as_ref().unwrap();
    assert!(!info.partial);
    assert!(
        info.artist
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .text
            .contains("independent group")
    );
    assert_eq!(info.album.as_ref().unwrap().date, "2020-05-06");
    assert_eq!(info.discography.entries[0].title, "First Light");
    assert!(!needs_lookup(&track, Field::Information));
    let count = fixture.count();
    let mut sibling = song();
    sibling.title = "Another song".into();
    let sibling_query = Query::for_track(&sibling, Field::Information).unwrap();
    client.lookup(&sibling_query, false, &|| false).unwrap();
    assert_eq!(
        fixture.count(),
        count,
        "Entity caches must avoid repeated album/artist requests"
    );
    let page = Query::for_track(&track, Field::Discography).unwrap();
    assert_eq!(page.offset, 2);
    let result = client.lookup(&page, false, &|| false).unwrap().unwrap();
    assert!(apply(&mut track, &page.key, &result));
    assert!(
        !apply(&mut track, &page.key, &result),
        "Duplicate/stale pages must not apply"
    );
    assert_eq!(
        track.music_info.as_ref().unwrap().discography.entries.len(),
        3
    );
    assert!(Query::for_track(&track, Field::Discography).is_none());
    assert!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.to_lowercase().contains("user-agent: openaudioplayer/"))
    );
    let restored: Track = serde_json::from_str(&serde_json::to_string(&track).unwrap()).unwrap();
    assert_eq!(restored.music_info, track.music_info);
}

#[test]
fn missing_tags_fill_conservatively_and_survive_refresh_without_overwriting_file_tags() {
    let fixture = Fixture::new(routes());
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    for field in ["artist", "album", "both"] {
        let mut track = song();
        if field != "album" {
            track.artist.clear();
        }
        if field != "artist" {
            track.album.clear();
        }
        if field == "both" {
            track.musicbrainz_recording_id = Some(RECORDING.into());
        }
        let original = track.clone();
        let query = Query::for_track(&track, Field::Information).unwrap();
        let result = client.lookup(&query, false, &|| false).unwrap().unwrap();
        assert!(apply(&mut track, &query.key, &result));
        assert_eq!(track.artist, "The Test Ensemble");
        assert_eq!(track.album, "Small Hours");
        let mut refreshed = original.clone();
        preserve_downloaded(&track, &mut refreshed);
        assert_eq!(
            (&refreshed.artist, &refreshed.album),
            (&track.artist, &track.album)
        );
        let refresh_query = Query::for_track(&track, Field::Information).unwrap();
        let fresh = client
            .lookup(&refresh_query, true, &|| false)
            .unwrap()
            .unwrap();
        assert!(apply(&mut track, &refresh_query.key, &fresh));
        let mut refreshed = original.clone();
        preserve_downloaded(&track, &mut refreshed);
        assert_eq!(
            (&refreshed.artist, &refreshed.album),
            (&track.artist, &track.album),
            "Manual online refresh must retain fallback provenance"
        );
        let mut edited = original.clone();
        edited.artist = "My corrected artist".into();
        preserve_downloaded(&track, &mut edited);
        assert_eq!(edited.artist, "My corrected artist");
        assert!(edited.music_info.is_none());
        let mut stale = original;
        stale.title = "A different song".into();
        assert!(!apply(&mut stale, &query.key, &result));
    }
}

#[test]
fn ambiguous_names_recordings_and_truncated_release_lists_do_not_fill_tags() {
    let mut ambiguous = recording();
    ambiguous["id"] = json!("72345678-1234-1234-1234-123456789abc");
    ambiguous["artist-credit"] = json!([{"name":"Another artist","artist":{"id":"82345678-1234-1234-1234-123456789abc","name":"Another artist"}}]);
    let mut fixture_routes = routes();
    fixture_routes.insert(
        0,
        route(
            "/mb/recording/",
            json!({"count":2,"recordings":[recording(),ambiguous]}),
        ),
    );
    fixture_routes.insert(0, route("/mb/artist/", json!({"count":2,"artists":[{"id":ARTIST,"score":100,"name":"The Test Ensemble"},{"id":"82345678-1234-1234-1234-123456789abc","score":100,"name":"The Test Ensemble"}]})));
    let fixture = Fixture::new(fixture_routes);
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    let mut track = song();
    track.artist.clear();
    let query = Query::for_track(&track, Field::Information).unwrap();
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    track = song();
    track.album.clear();
    track.title_from_filename = true;
    let query = Query::for_track(&track, Field::Information).unwrap();
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    track.artist.clear();
    assert!(Query::for_track(&track, Field::Information).is_none());

    let mut fixture_routes = routes();
    fixture_routes.insert(0, route("/mb/release/", json!({"release-count":101,"releases":[{"title":"Small Hours","status":"Official","release-group":{"id":GROUP}}]})));
    let fixture = Fixture::new(fixture_routes);
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    let mut track = song();
    track.album.clear();
    let query = Query::for_track(&track, Field::Information).unwrap();
    let result = client.lookup(&query, false, &|| false).unwrap().unwrap();
    apply(&mut track, &query.key, &result);
    assert!(track.album.is_empty());
    assert!(track.music_info.unwrap().artist.is_some());
}

#[test]
fn biography_failures_keep_metadata_and_only_trusted_non_disambiguation_pages_are_used() {
    for (status, body) in [
        (500, json!({})),
        (503, json!({})),
        (
            200,
            json!({"query":{"pages":[{"extract":"Wrong page","fullurl":"https://en.wikipedia.org/wiki/Other","pageprops":{"disambiguation":""}}]}}),
        ),
        (
            200,
            json!({"query":{"pages":[{"extract":"Untrusted source","fullurl":"https://untrusted.invalid/page"}]}}),
        ),
    ] {
        let mut fixture_routes = routes();
        fixture_routes.insert(
            0,
            (
                "/wiki".into(),
                status,
                String::new(),
                serde_json::to_vec(&body).unwrap(),
            ),
        );
        let fixture = Fixture::new(fixture_routes);
        let temp = tempfile::tempdir().unwrap();
        let mut client = providers::Client::new(fixture.config(temp.path()));
        let query = Query::for_track(&song(), Field::Information).unwrap();
        let Found::Information(info) = client.lookup(&query, false, &|| false).unwrap().unwrap()
        else {
            panic!()
        };
        assert_eq!(info.partial, status >= 500);
        assert_eq!(info.album.unwrap().date, "2020-05-06");
        assert!(info.artist.unwrap().summary.is_none());
        let count = fixture.count();
        client.lookup(&query, false, &|| false).unwrap();
        assert_eq!(fixture.count(), count);
        client.lookup(&query, true, &|| false).unwrap();
        assert!(fixture.count() > count);
    }
}

#[test]
fn cover_and_information_workers_share_musicbrainz_pacing() {
    let mut fixture_routes = super::tests::artwork_routes();
    fixture_routes.extend(routes());
    let fixture = Fixture::new(fixture_routes);
    let temp = tempfile::tempdir().unwrap();
    let mut config = fixture.config(temp.path());
    config.interval = Duration::from_millis(80);
    let mut service = Service::new(config);
    for field in [Field::Cover, Field::Information] {
        assert!(service.request(Query::for_track(&song(), field).unwrap(), false));
    }
    let mut replies = vec![];
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while replies.len() < 2 && std::time::Instant::now() < deadline {
        replies.extend(service.poll());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(replies.len(), 2);
    assert!(
        replies
            .iter()
            .all(|r| r.result.as_ref().is_ok_and(|v| v.is_some()))
    );
    let times: Vec<_> = fixture
        .received
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.starts_with("/mb/"))
        .map(|(_, t)| *t)
        .collect();
    assert!(times.len() >= 5);
    assert!(
        times
            .windows(2)
            .all(|w| w[1].duration_since(w[0]) >= Duration::from_millis(60)),
        "{times:?}"
    );
}

#[test]
fn negative_information_cache_expires_with_search_responses() {
    let fixture = Fixture::new(vec![
        route("/mb/release-group/", json!({"count":0,"release-groups":[]})),
        route("/mb/artist/", json!({"count":0,"artists":[]})),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    let query = Query::for_track(&song(), Field::Information).unwrap();
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    let count = fixture.count();
    client.lookup(&query, false, &|| false).unwrap();
    assert_eq!(fixture.count(), count);
    let checked = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 8 * 86400;
    for entry in std::fs::read_dir(temp.path()).unwrap() {
        let path = entry.unwrap().path();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["checked"] = json!(checked);
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    assert!(
        fixture.count() > count,
        "Expired empty searches must reach the provider again"
    );
}

#[test]
#[ignore = "contacts public MusicBrainz, Wikidata and Wikipedia APIs"]
fn live_artist_album_information() {
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(Config {
        cache: temp.path().into(),
        ..Config::default()
    });
    let mut track = Track::new(
        "file:///public-metadata-check.flac",
        "Instant Crush",
        Kind::Music,
    );
    track.artist = "Daft Punk".into();
    track.album = "Random Access Memories".into();
    track.duration = 337.0;
    let query = Query::for_track(&track, Field::Information).unwrap();
    let Found::Information(info) = client
        .lookup(&query, false, &|| false)
        .unwrap()
        .expect("Known album and artist")
    else {
        panic!()
    };
    assert_eq!(info.artist.as_ref().unwrap().name, "Daft Punk");
    assert!(
        info.artist.as_ref().unwrap().summary.is_some(),
        "Artist summary: {info:?}"
    );
    assert!(
        info.album.as_ref().unwrap().summary.is_some(),
        "Album summary: {info:?}"
    );
    assert!(info.album.as_ref().unwrap().date.starts_with("2013"));
    assert!(!info.discography.entries.is_empty());
}
