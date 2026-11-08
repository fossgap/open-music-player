use crate::model::{Kind, Subscription, Track, stable_id};
use reqwest::blocking::Client;
use std::{fs, io::Read, path::Path, time::Duration};

pub fn http_url(input: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(input.trim())
        .map_err(|_| "Enter a complete http:// or https:// address")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Enter an HTTP or HTTPS address without embedded credentials".into());
    }
    Ok(url)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("OpenMusicPlayer/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())
}

fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let url = http_url(url)?;
    let response = client()?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|n| n > limit) {
        return Err("The response is too large".into());
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("The response is too large".into());
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PodcastDirectory {
    Apple,
    Gpodder,
}

impl PodcastDirectory {
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::Gpodder,
            _ => Self::Apple,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Apple => "Apple Podcasts",
            Self::Gpodder => "gPodder",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PodcastShow {
    pub id: String,
    pub title: String,
    pub author: String,
    pub feed_url: String,
    pub artwork: Option<String>,
    pub website: Option<String>,
}

pub fn podcast_search(
    directory: PodcastDirectory,
    query: &str,
) -> Result<Vec<PodcastShow>, String> {
    let (variable, default) = match directory {
        PodcastDirectory::Apple => ("OPEN_MUSIC_ITUNES_API", "https://itunes.apple.com/"),
        PodcastDirectory::Gpodder => ("OPEN_MUSIC_GPODDER_API", "https://gpodder.net/"),
    };
    // Overrides support private mirrors and deterministic offline integration tests.
    let base = std::env::var(variable).unwrap_or_else(|_| default.into());
    let mut base = http_url(&base)?;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    let query = query.trim();
    let mut url = base
        .join(match directory {
            PodcastDirectory::Apple => "search",
            PodcastDirectory::Gpodder if query.is_empty() => "toplist/50.json",
            PodcastDirectory::Gpodder => "search.json",
        })
        .map_err(|e| e.to_string())?;
    match directory {
        PodcastDirectory::Apple => {
            url.query_pairs_mut()
                .append_pair("media", "podcast")
                .append_pair("entity", "podcast")
                .append_pair("limit", "50")
                .append_pair("country", "US")
                .append_pair("term", if query.is_empty() { "podcast" } else { query });
        }
        PodcastDirectory::Gpodder if !query.is_empty() => {
            url.query_pairs_mut()
                .append_pair("q", query)
                .append_pair("scale_logo", "256");
        }
        _ => {}
    }
    parse_podcast_search(directory, &fetch(url.as_str(), 4_000_000)?)
}

pub fn parse_podcast_search(
    directory: PodcastDirectory,
    bytes: &[u8],
) -> Result<Vec<PodcastShow>, String> {
    let json: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let results = match directory {
        PodcastDirectory::Apple => json["results"].as_array(),
        PodcastDirectory::Gpodder => json.as_array(),
    }
    .ok_or("The podcast directory returned an invalid response")?;
    let mut seen = std::collections::HashSet::new();
    let mut shows = Vec::new();
    for item in results.iter().take(200) {
        let string = |key: &str| item[key].as_str().unwrap_or("").trim();
        let (feed, title, author, artwork, website) = match directory {
            PodcastDirectory::Apple => (
                string("feedUrl"),
                string("collectionName"),
                string("artistName"),
                item["artworkUrl600"]
                    .as_str()
                    .unwrap_or(string("artworkUrl100")),
                string("collectionViewUrl"),
            ),
            PodcastDirectory::Gpodder => (
                string("url"),
                string("title"),
                string("author"),
                item["scaled_logo_url"]
                    .as_str()
                    .unwrap_or(string("logo_url")),
                string("website"),
            ),
        };
        let Ok(mut feed) = http_url(feed) else {
            continue;
        };
        feed.set_fragment(None);
        let feed = feed.to_string();
        if !seen.insert(feed.clone()) {
            continue;
        }
        let title = if title.is_empty() {
            string("trackName")
        } else {
            title
        };
        shows.push(PodcastShow {
            id: stable_id(&feed),
            title: if title.is_empty() {
                "Podcast".into()
            } else {
                title.into()
            },
            author: author.into(),
            feed_url: feed,
            artwork: http_url(artwork).ok().map(|u| u.to_string()),
            website: http_url(website).ok().map(|u| u.to_string()),
        });
        if shows.len() == 50 {
            break;
        }
    }
    Ok(shows)
}

pub fn radio(query: &str, cache: &Path) -> Result<Vec<Track>, String> {
    radio_cancellable(query, cache, || false)
}

pub fn radio_cancellable(
    query: &str,
    cache: &Path,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<Track>, String> {
    let mut last_error = String::new();
    let servers = std::env::var("OPEN_MUSIC_RADIO_API")
        .map(|base| vec![base])
        .unwrap_or_else(|_| {
            vec![
                "https://de1.api.radio-browser.info/".into(),
                "https://nl1.api.radio-browser.info/".into(),
                "https://at1.api.radio-browser.info/".into(),
            ]
        });
    for server in servers {
        if cancelled() {
            return Err("Cancelled".into());
        }
        let mut base = http_url(&server)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let mut url = base
            .join("json/stations/search")
            .map_err(|e| e.to_string())?;
        url.query_pairs_mut()
            .append_pair("name", query)
            .append_pair("limit", "60")
            .append_pair("hidebroken", "true")
            .append_pair("order", "clickcount")
            .append_pair("reverse", "true");
        match fetch(url.as_str(), 4_000_000) {
            Ok(bytes) => {
                let stations: Vec<serde_json::Value> =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let mut tracks = Vec::new();
                for s in stations {
                    let uri = s["url_resolved"]
                        .as_str()
                        .filter(|u| !u.is_empty())
                        .or_else(|| s["url"].as_str())
                        .unwrap_or("");
                    if http_url(uri).is_err() {
                        continue;
                    }
                    let mut t = Track::new(uri, s["name"].as_str().unwrap_or("Radio"), Kind::Radio);
                    if let Some(uuid) = s["stationuuid"].as_str() {
                        t.id = stable_id(&format!("radio:{uuid}"));
                    }
                    t.artist = s["country"].as_str().unwrap_or("").into();
                    t.album = s["tags"]
                        .as_str()
                        .unwrap_or("")
                        .split(',')
                        .take(3)
                        .collect::<Vec<_>>()
                        .join(" · ");
                    t.source = "Radio Browser".into();
                    // Fetch images separately after showing the search results.
                    t.artwork = s["favicon"]
                        .as_str()
                        .filter(|u| http_url(u).is_ok())
                        .map(String::from);
                    tracks.push(t);
                }
                let _ = fs::create_dir_all(cache);
                return Ok(tracks);
            }
            Err(e) => last_error = e,
        }
    }
    Err(format!(
        "Radio Browser is unavailable. Try again or add a stream URL. {last_error}"
    ))
}

pub fn podcast(url: &str) -> Result<(Subscription, Vec<Track>), String> {
    let bytes = fetch(url, 8_000_000)?;
    let text = String::from_utf8(bytes).map_err(|_| "The feed is not UTF-8 XML")?;
    parse_feed(url, &text)
}

pub fn parse_feed(url: &str, text: &str) -> Result<(Subscription, Vec<Track>), String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("Invalid podcast feed: {e}"))?;
    let channel = doc
        .descendants()
        .find(|n| n.has_tag_name("channel") || n.has_tag_name("feed"))
        .ok_or("This address is not an RSS or Atom podcast feed")?;
    let child_text = |node: roxmltree::Node<'_, '_>, tag: &str| -> String {
        node.children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim()
            .into()
    };
    let title = child_text(channel, "title");
    let title = if title.is_empty() {
        "Podcast".into()
    } else {
        title
    };
    let image = channel
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "image")
        .find_map(|n| {
            n.attribute("href").map(String::from).or_else(|| {
                n.children()
                    .find(|n| n.has_tag_name("url"))
                    .and_then(|n| n.text())
                    .map(String::from)
            })
        });
    let mut tracks = Vec::new();
    let base = http_url(url)?;
    for item in channel
        .children()
        .filter(|n| n.is_element() && matches!(n.tag_name().name(), "item" | "entry"))
    {
        let media = item.children().find_map(|n| {
            if n.has_tag_name("enclosure") {
                n.attribute("url")
            } else if n.is_element()
                && n.tag_name().name() == "link"
                && n.attribute("rel") == Some("enclosure")
            {
                n.attribute("href")
            } else {
                None
            }
        });
        let Some(uri) = media
            .and_then(|u| base.join(u).ok())
            .filter(|u| http_url(u.as_str()).is_ok())
        else {
            continue;
        };
        let episode_title = child_text(item, "title");
        let mut t = Track::new(
            uri.as_str(),
            if episode_title.is_empty() {
                &title
            } else {
                &episode_title
            },
            Kind::Podcast,
        );
        let guid = child_text(item, "guid");
        if !guid.is_empty() {
            t.id = stable_id(&format!("{url}#{guid}"));
        }
        t.artist = title.clone();
        t.album = child_text(item, "pubDate");
        t.source = url.into();
        t.artwork = item
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "image")
            .and_then(|n| n.attribute("href"))
            .map(String::from)
            .or_else(|| image.clone());
        t.duration = parse_duration(&child_text(item, "duration"));
        tracks.push(t);
        if tracks.len() >= 1000 {
            break;
        }
    }
    if tracks.is_empty() {
        return Err("No playable audio enclosures found in this feed".into());
    }
    Ok((
        Subscription {
            url: url.into(),
            title,
        },
        tracks,
    ))
}

fn parse_duration(value: &str) -> f64 {
    value
        .trim()
        .split(':')
        .try_fold(0.0, |total, part| {
            part.parse::<f64>().ok().map(|n| total * 60.0 + n)
        })
        .unwrap_or(0.0)
}

pub fn artwork(uri: &str, cache: &Path) -> Result<String, String> {
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let path = cache.join(format!("{}.image", stable_id(uri)));
    if !path.is_file() {
        let bytes = fetch(uri, 5_000_000)?;
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(path.to_string_lossy().into())
}

pub fn youtube_url(input: &str) -> bool {
    http_url(input).is_ok_and(|u| {
        matches!(
            u.host_str(),
            Some(
                "youtube.com"
                    | "www.youtube.com"
                    | "m.youtube.com"
                    | "music.youtube.com"
                    | "youtu.be"
            )
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn podcast_directories_filter_invalid_feeds_and_deduplicate() {
        let apple = br#"{"results":[{"collectionName":"Science","artistName":"Lab","feedUrl":"https://example.org/feed","artworkUrl600":"https://example.org/art.png"},{"collectionName":"Duplicate","feedUrl":"https://example.org/feed"},{"collectionName":"Invalid","feedUrl":"file:///tmp/feed"}]}"#;
        let results = parse_podcast_search(PodcastDirectory::Apple, apple).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].author, "Lab");
        let gpodder = br#"[{"title":"Other source","url":"https://example.org/rss","logo_url":"javascript:bad()","website":"https://example.org"}]"#;
        let results = parse_podcast_search(PodcastDirectory::Gpodder, gpodder).unwrap();
        assert_eq!(results[0].title, "Other source");
        assert!(results[0].artwork.is_none());
        assert!(parse_podcast_search(PodcastDirectory::Apple, b"{}").is_err());
    }
    #[test]
    fn rejects_non_network_and_credential_urls() {
        for s in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pass@example.com",
            "example.com",
        ] {
            assert!(http_url(s).is_err());
        }
        assert!(http_url("https://example.org/music?q=a%20b").is_ok());
        assert!(!youtube_url("https://youtube.com.evil.org/watch?v=a"));
    }
    #[test]
    fn parses_rss_namespaces_and_relative_enclosures() {
        let xml = r#"<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel><title>Sound &amp; Space</title><itunes:image href="https://example.org/cover.jpg"/><item><title>Episode one</title><guid>one</guid><itunes:duration>1:02:03</itunes:duration><enclosure url="/audio/one.mp3" type="audio/mpeg"/></item><item><title>No media</title></item></channel></rss>"#;
        let (sub, tracks) = parse_feed("https://example.org/feed.xml", xml).unwrap();
        assert_eq!(sub.title, "Sound & Space");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].duration, 3723.0);
        assert_eq!(tracks[0].uri, "https://example.org/audio/one.mp3");
        assert_eq!(
            tracks[0].artwork.as_deref(),
            Some("https://example.org/cover.jpg")
        );
    }
    #[test]
    fn parses_atom_audio_links() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Test</title><entry><title>Hello</title><link rel="enclosure" href="https://example.org/hello.ogg"/></entry></feed>"#;
        assert_eq!(
            parse_feed("https://example.org/feed", xml).unwrap().1.len(),
            1
        );
    }
    #[test]
    fn rejects_non_feeds_and_feeds_without_audio() {
        assert!(parse_feed("https://example.org", "<html/>").is_err());
        assert!(
            parse_feed(
                "https://example.org",
                "<rss><channel><title>Empty</title></channel></rss>"
            )
            .is_err()
        );
    }
}
