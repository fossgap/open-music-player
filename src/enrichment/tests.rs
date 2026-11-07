use super::*;
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::Mutex,
    thread,
    time::Instant,
};

const RELEASE: &str = "12345678-1234-1234-1234-123456789abc";
const GROUP: &str = "22345678-1234-1234-1234-123456789abc";

pub(crate) struct Fixture {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    done: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    pub fn new(routes: Vec<(String, u16, String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let worker = {
            let requests = requests.clone();
            let done = done.clone();
            thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut chunk = [0; 2048];
                    while !bytes.windows(4).any(|b| b == b"\r\n\r\n") {
                        let n = stream.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&chunk[..n]);
                    }
                    let request = String::from_utf8_lossy(&bytes).to_string();
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .split('?')
                        .next()
                        .unwrap_or("");
                    let (status, headers, body) = routes
                        .iter()
                        .find(|(p, ..)| p == path)
                        .map(|(_, status, headers, body)| {
                            (*status, headers.as_str(), body.as_slice())
                        })
                        .unwrap_or((404, "", b""));
                    requests.lock().unwrap().push(request);
                    let header = format!(
                        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
            })
        };
        Self {
            url,
            requests,
            done,
            worker: Some(worker),
        }
    }
    pub fn config(&self, path: &Path) -> Config {
        Config {
            cache: path.to_path_buf(),
            musicbrainz: format!("{}mb/", self.url),
            covers: format!("{}caa/", self.url),
            lrclib: format!("{}lyrics/", self.url),
            interval: Duration::ZERO,
        }
    }
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
pub(crate) fn song() -> Track {
    let mut track = Track::new("file:///fixture.wav", "Evening Window", Kind::Music);
    track.artist = "The Test Ensemble".into();
    track.album = "Small Hours".into();
    track.duration = 30.0;
    track
}
pub(crate) fn lyric() -> serde_json::Value {
    json!({"id": 42, "trackName": "Evening Window", "artistName": "The Test Ensemble", "albumName": "Small Hours", "duration": 30.0, "instrumental": false, "plainLyrics": "A quiet window, a new day.\nWe make a little room to play.", "syncedLyrics": "[00:01.00]A quiet window, a new day.\n[00:03.00]We make a little room to play."})
}
pub(crate) fn artwork_routes() -> Vec<(String, u16, String, Vec<u8>)> {
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 64, 64).unwrap();
    pixbuf.fill(0x8daa79ff);
    let image = pixbuf.save_to_bufferv("png", &[]).unwrap();
    vec![
        ("/mb/release/".into(), 200, String::new(), serde_json::to_vec(&json!({"releases": [{"id": RELEASE,"title":"Small Hours","score":100,"status":"Official","artist-credit":[{"name":"The Test Ensemble"}],"release-group":{"id":GROUP}}]})).unwrap()),
        (format!("/caa/release/{RELEASE}/front-500"), 200, "Content-Type: image/png\r\n".into(), image),
    ]
}
fn route(path: &str, value: serde_json::Value) -> (String, u16, String, Vec<u8>) {
    (
        path.into(),
        200,
        String::new(),
        serde_json::to_vec(&value).unwrap(),
    )
}

#[test]
fn legacy_preferences_and_queries_preserve_versions() {
    let state: crate::model::State = serde_json::from_str("{}").unwrap();
    assert!(state.fetch_covers && state.fetch_lyrics);
    let mut track = song();
    let key = Query::for_track(&track, Field::Lyrics).unwrap().key;
    track.title.push_str(" (Live)");
    assert_ne!(key, Query::for_track(&track, Field::Lyrics).unwrap().key);
    track.kind = Kind::Podcast;
    assert!(Query::for_track(&track, Field::Lyrics).is_none());
    assert!(Query::for_track(&track, Field::Cover).is_none());
    track.kind = Kind::Music;
    track.artist.clear();
    track.album.clear();
    assert!(Query::for_track(&track, Field::Cover).is_none());
    track.musicbrainz_release_id = Some(RELEASE.into());
    assert!(Query::for_track(&track, Field::Cover).is_some());
    track.musicbrainz_release_id = Some("../bad".into());
    assert!(Query::for_track(&track, Field::Cover).is_none());
}

#[test]
fn results_are_fallbacks_and_metadata_refresh_keeps_only_matching_downloads() {
    let mut track = song();
    let key = Query::for_track(&track, Field::Lyrics).unwrap().key;
    let found = Found::Lyrics {
        plain: "Original fixture line".into(),
        synced: None,
        source: "https://lrclib.net/api/get/42".into(),
        instrumental: false,
    };
    assert!(apply(&mut track, &key, &found));
    let mut refreshed = song();
    preserve_downloaded(&track, &mut refreshed);
    assert_eq!(refreshed.lyrics, track.lyrics);
    refreshed = song();
    refreshed.title.push_str(" (Live)");
    assert!(!apply(&mut refreshed, &key, &found));
    preserve_downloaded(&track, &mut refreshed);
    assert!(refreshed.lyrics.is_none());
    refreshed = song();
    refreshed.lyrics = Some("My own sidecar".into());
    preserve_downloaded(&track, &mut refreshed);
    assert_eq!(refreshed.lyrics.as_deref(), Some("My own sidecar"));
    assert!(refreshed.lyrics_source.is_none());
    assert!(!apply(&mut refreshed, &key, &found));
    let instrumental = Found::Lyrics {
        plain: String::new(),
        synced: None,
        source: "https://lrclib.net/api/get/42".into(),
        instrumental: true,
    };
    let mut track = song();
    assert!(apply(&mut track, &key, &instrumental));
    assert!(!missing(&track, Field::Lyrics));
}

#[test]
fn existing_provider_lyrics_gain_timing_without_overwriting_local_lyrics() {
    let mut track = song();
    track.lyrics = Some("Existing lyric".into());
    track.lyrics_source = Some("https://lrclib.net/api/get/42".into());
    assert!(!missing(&track, Field::Lyrics));
    assert!(needs_lookup(&track, Field::Lyrics));
    let key = Query::for_track(&track, Field::Lyrics).unwrap().key;
    let found = Found::Lyrics {
        plain: "Existing lyric".into(),
        synced: Some("[00:01]Existing lyric".into()),
        source: "https://lrclib.net/api/get/42".into(),
        instrumental: false,
    };
    assert!(apply(&mut track, &key, &found));
    assert!(!needs_lookup(&track, Field::Lyrics));
    assert!(track.synced_lyrics.is_some());
    let mut refreshed = song();
    preserve_downloaded(&track, &mut refreshed);
    assert_eq!(refreshed.synced_lyrics, track.synced_lyrics);
    refreshed.lyrics_source = None;
    refreshed.lyrics_timing_checked = false;
    assert!(!apply(&mut refreshed, &key, &found));
    let restored: Track = serde_json::from_str(&serde_json::to_string(&track).unwrap()).unwrap();
    assert_eq!(restored.synced_lyrics, track.synced_lyrics);
}

#[test]
fn cover_matching_download_validation_cache_and_album_artist() {
    let fixture = Fixture::new(artwork_routes());
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    let mut track = song();
    track.album_artist = track.artist.clone();
    track.artist = "Guest performer".into();
    let query = Query::for_track(&track, Field::Cover).unwrap();
    let found = client.lookup(&query, false, &|| false).unwrap().unwrap();
    let Found::Cover { path, source } = found else {
        panic!("Expected artwork")
    };
    assert!(gtk::gdk_pixbuf::Pixbuf::from_file(&path).is_ok());
    assert_eq!(source, format!("https://musicbrainz.org/release/{RELEASE}"));
    assert_eq!(fixture.count(), 2);
    let mut restarted = providers::Client::new(fixture.config(temp.path()));
    assert!(
        restarted
            .lookup(&query, false, &|| false)
            .unwrap()
            .is_some()
    );
    assert_eq!(fixture.count(), 2);
    fs::remove_file(path).unwrap();
    assert!(
        restarted
            .lookup(&query, false, &|| false)
            .unwrap()
            .is_some()
    );
    assert_eq!(fixture.count(), 4);
    assert!(
        fixture.requests.lock().unwrap()[0]
            .contains("https://github.com/fossgap/open-audio-player")
    );
}

#[test]
fn cover_tries_another_edition_and_rejects_unrelated_releases() {
    let mut routes = artwork_routes();
    routes[1].0 = format!("/caa/release-group/{GROUP}/front-500");
    let fixture = Fixture::new(routes);
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    assert!(
        client
            .lookup(
                &Query::for_track(&song(), Field::Cover).unwrap(),
                false,
                &|| false
            )
            .unwrap()
            .is_some()
    );
    assert_eq!(fixture.count(), 3);
    let mut other = song();
    other.artist = "Another artist".into();
    assert!(
        client
            .lookup(
                &Query::for_track(&other, Field::Cover).unwrap(),
                false,
                &|| false
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture.count(),
        4,
        "An unrelated album must not trigger an image download"
    );
}

#[test]
fn lyrics_match_recording_and_support_synced_only_and_instrumentals() {
    let mut wrong_artist = lyric();
    wrong_artist["artistName"] = json!("Another artist");
    let mut live = lyric();
    live["trackName"] = json!("Evening Window (Live)");
    let mut wrong_duration = lyric();
    wrong_duration["duration"] = json!(90);
    let mut synced = lyric();
    synced["plainLyrics"] = serde_json::Value::Null;
    let fixture = Fixture::new(vec![
        route("/lyrics/get", live.clone()),
        route(
            "/lyrics/search",
            json!([wrong_artist, live, wrong_duration, synced]),
        ),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let query = Query::for_track(&song(), Field::Lyrics).unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    let Some(Found::Lyrics { plain, synced, .. }) =
        client.lookup(&query, false, &|| false).unwrap()
    else {
        panic!("Expected lyrics")
    };
    assert_eq!(plain, lyric()["plainLyrics"].as_str().unwrap());
    assert!(synced.is_some());
    assert_eq!(fixture.count(), 2);
    assert!(client.lookup(&query, false, &|| false).unwrap().is_some());
    assert_eq!(fixture.count(), 2);
    let mut instrumental = lyric();
    instrumental["instrumental"] = json!(true);
    instrumental["plainLyrics"] = json!(null);
    instrumental["syncedLyrics"] = json!(null);
    let instrumental_fixture = Fixture::new(vec![route("/lyrics/get", instrumental)]);
    let mut client = providers::Client::new(instrumental_fixture.config(temp.path()));
    assert!(matches!(
        client.lookup(&query, true, &|| false).unwrap(),
        Some(Found::Lyrics {
            instrumental: true,
            ..
        })
    ));
}

#[test]
fn negative_cache_retry_and_durationless_album_matching() {
    let mut no_duration = song();
    no_duration.duration = 0.0;
    no_duration.album = "Another album".into();
    let query = Query::for_track(&no_duration, Field::Lyrics).unwrap();
    let fixture = Fixture::new(vec![
        route("/lyrics/get", lyric()),
        route("/lyrics/search", json!([lyric()])),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let mut client = providers::Client::new(fixture.config(temp.path()));
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    assert_eq!(fixture.count(), 2);
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    assert_eq!(fixture.count(), 2);
    assert!(client.lookup(&query, true, &|| false).unwrap().is_none());
    assert_eq!(fixture.count(), 4);
    let cache = temp.path().join(format!("{}.json", query.key));
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
    value["checked"] = json!(0);
    fs::write(cache, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(client.lookup(&query, false, &|| false).unwrap().is_none());
    assert_eq!(fixture.count(), 6);
    no_duration.album = song().album;
    assert!(
        client
            .lookup(
                &Query::for_track(&no_duration, Field::Lyrics).unwrap(),
                false,
                &|| false
            )
            .unwrap()
            .is_some()
    );
}

#[test]
fn failures_are_not_cached_as_missing_and_rate_limit_blocks_forced_retries() {
    let temp = tempfile::tempdir().unwrap();
    let query = Query::for_track(&song(), Field::Lyrics).unwrap();
    for retry_after in [
        "120".to_string(),
        httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(120)),
    ] {
        let fixture = Fixture::new(vec![(
            "/lyrics/get".into(),
            429,
            format!("Retry-After: {retry_after}\r\n"),
            b"<html>Rate limit</html>".to_vec(),
        )]);
        let mut client = providers::Client::new(fixture.config(temp.path()));
        let error = client.lookup(&query, false, &|| false).unwrap_err();
        assert!(!error.retryable());
        assert!(
            error
                .retry_at
                .duration_since(SystemTime::now())
                .unwrap()
                .as_secs()
                >= 118
        );
        assert!(client.lookup(&query, true, &|| false).is_err());
        assert_eq!(fixture.count(), 1);
        assert!(!temp.path().join(format!("{}.json", query.key)).exists());
    }
    let fixture = Fixture::new(vec![route("/lyrics/get", lyric())]);
    assert!(
        providers::Client::new(fixture.config(temp.path()))
            .lookup(&query, false, &|| false)
            .unwrap()
            .is_some()
    );
}

#[test]
fn malformed_and_oversized_covers_are_not_cached() {
    for body in [b"<html>Not an image</html>".to_vec(), vec![0; 5_000_001]] {
        let mut routes = artwork_routes();
        routes[1].3 = body;
        let fixture = Fixture::new(routes);
        let temp = tempfile::tempdir().unwrap();
        let query = Query::for_track(&song(), Field::Cover).unwrap();
        let mut client = providers::Client::new(fixture.config(temp.path()));
        assert!(client.lookup(&query, false, &|| false).is_err());
        assert!(!temp.path().join(format!("{}.json", query.key)).exists());
        assert!(!temp.path().join(format!("{}.png", query.key)).exists());
    }
}

#[test]
fn cancelling_one_field_does_not_cancel_the_other_or_apply_stale_results() {
    let mut routes = artwork_routes();
    routes.push(route("/lyrics/get", lyric()));
    let fixture = Fixture::new(routes);
    let temp = tempfile::tempdir().unwrap();
    let mut service = Service::new(fixture.config(temp.path()));
    let cover = Query::for_track(&song(), Field::Cover).unwrap();
    let lyrics = Query::for_track(&song(), Field::Lyrics).unwrap();
    assert!(service.request(cover.clone(), false));
    assert!(service.request(lyrics.clone(), false));
    service.cancel(Field::Cover);
    assert!(!service.status.contains_key(&cover.key));
    assert!(matches!(
        service.status.get(&lyrics.key),
        Some(Status::Pending)
    ));
    let until = Instant::now() + Duration::from_secs(3);
    let mut replies = Vec::new();
    while replies.is_empty() && Instant::now() < until {
        replies = service.poll();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].query.field, Field::Lyrics);
    assert!(replies[0].result.as_ref().unwrap().is_some());
    service.stop();
}

#[test]
#[ignore = "contacts MusicBrainz, Cover Art Archive and LRCLIB; run explicitly with network access"]
fn live_metadata_providers() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        cache: temp.path().to_path_buf(),
        ..Config::default()
    };
    let mut track = Track::new(
        "file:///live-verification.flac",
        "Instant Crush",
        Kind::Music,
    );
    track.artist = "Daft Punk".into();
    track.album = "Random Access Memories".into();
    track.duration = 337.0;
    for field in [Field::Cover, Field::Lyrics] {
        let query = Query::for_track(&track, field).unwrap();
        let found = providers::Client::new(config.clone())
            .lookup(&query, false, &|| false)
            .unwrap_or_else(|e| panic!("{field:?}: {}", e.message))
            .expect("A known recording should match");
        assert!(found.available());
        assert!(apply(&mut track, &query.key, &found));
    }
    // No downloaded lyric text or copyrighted cover is printed or retained in the source tree.
    assert!(track.artwork_source.is_some() && track.lyrics_source.is_some());
}
