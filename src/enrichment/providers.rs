use super::*;
mod information;
use crate::storage;
use gtk::gdk_pixbuf::prelude::*;
use reqwest::{StatusCode, blocking::Client as HttpClient};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{Instant, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Config {
    pub cache: PathBuf,
    pub musicbrainz: String,
    pub covers: String,
    pub lrclib: String,
    pub wikipedia: String,
    pub wikidata: String,
    pub interval: Duration,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            cache: storage::data_dir().join("metadata"),
            musicbrainz: std::env::var("OPEN_AUDIO_MUSICBRAINZ_API")
                .unwrap_or_else(|_| "https://musicbrainz.org/ws/2/".into()),
            covers: std::env::var("OPEN_AUDIO_COVER_ART_API")
                .unwrap_or_else(|_| "https://coverartarchive.org/".into()),
            lrclib: std::env::var("OPEN_AUDIO_LRCLIB_API")
                .unwrap_or_else(|_| "https://lrclib.net/api/".into()),
            wikipedia: std::env::var("OPEN_AUDIO_WIKIPEDIA_API")
                .unwrap_or_else(|_| "https://en.wikipedia.org/w/api.php".into()),
            wikidata: std::env::var("OPEN_AUDIO_WIKIDATA_API")
                .unwrap_or_else(|_| "https://www.wikidata.org/w/api.php".into()),
            interval: Duration::from_millis(1100),
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Cached {
    version: u8,
    checked: u64,
    value: Option<Found>,
}
pub(super) struct Pacing {
    next: Instant,
    cooldown: SystemTime,
}
impl Default for Pacing {
    fn default() -> Self {
        Self {
            next: Instant::now(),
            cooldown: UNIX_EPOCH,
        }
    }
}
pub(super) struct Client {
    config: Config,
    http: Result<HttpClient, String>,
    next: Instant,
    cooldowns: HashMap<String, SystemTime>,
    pacing: Arc<std::sync::Mutex<Pacing>>,
    refresh_information: bool,
}
impl Client {
    #[cfg(test)]
    pub fn new(config: Config) -> Self {
        Self::with_pacing(config, Arc::new(std::sync::Mutex::new(Pacing::default())))
    }
    pub fn with_pacing(config: Config, pacing: Arc<std::sync::Mutex<Pacing>>) -> Self {
        let http = HttpClient::builder()
            .user_agent(concat!(
                "OpenAudioPlayer/",
                env!("CARGO_PKG_VERSION"),
                " (",
                env!("CARGO_PKG_HOMEPAGE"),
                ")"
            ))
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| e.to_string());
        Self {
            config,
            http,
            next: Instant::now(),
            cooldowns: HashMap::new(),
            pacing,
            refresh_information: false,
        }
    }
    pub fn lookup(
        &mut self,
        query: &Query,
        force: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Found>, Failure> {
        let path = self.config.cache.join(format!("{}.json", query.key));
        if !force {
            if let Ok(bytes) = fs::read(&path) {
                if let Ok(cached) = serde_json::from_slice::<Cached>(&bytes) {
                    let age = now().saturating_sub(cached.checked);
                    if cached.version == 1
                        && cached.value.as_ref().map_or(age < 7 * 86400, |v| {
                            v.field() == query.field
                                && v.available()
                                && (!matches!(query.field, Field::Information | Field::Discography)
                                    || age < 30 * 86400)
                        })
                    {
                        return Ok(cached.value);
                    }
                }
            }
        }
        self.refresh_information = force;
        let value = match query.field {
            Field::Cover => self.cover(query, cancelled)?,
            Field::Lyrics => self.lyrics(query, cancelled)?,
            Field::Information => self
                .information(query, cancelled)?
                .map(|info| Found::Information(Box::new(info))),
            Field::Discography => Some(Found::Discography(self.discography(
                query.artist_id.as_deref().unwrap_or(""),
                query.offset,
                cancelled,
            )?)),
        };
        if cancelled() {
            return Err(Failure::new("Lookup cancelled.", 0));
        }
        fs::create_dir_all(&self.config.cache)
            .map_err(|_| Failure::new("Could not create the metadata cache.", 5))?;
        let cached = Cached {
            version: 1,
            checked: now(),
            value: value.clone(),
        };
        atomic_write(
            &path,
            &serde_json::to_vec(&cached)
                .map_err(|_| Failure::new("Could not save the metadata cache.", 5))?,
        )?;
        Ok(value)
    }
    fn get(
        &mut self,
        url: url::Url,
        max: u64,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Vec<u8>>, Failure> {
        let provider = [
            ("musicbrainz", &self.config.musicbrainz),
            ("covers", &self.config.covers),
            ("lrclib", &self.config.lrclib),
            ("wikipedia", &self.config.wikipedia),
            ("wikidata", &self.config.wikidata),
        ]
        .into_iter()
        .find(|(_, base)| {
            crate::services::http_url(base).is_ok_and(|base| {
                url.origin() == base.origin()
                    && (url.path() == base.path()
                        || (base.path().ends_with('/') && url.path().starts_with(base.path())))
            })
        })
        .map(|(name, _)| name.to_string())
        .unwrap_or_else(|| url.origin().ascii_serialization());
        if let Some(&cooldown) = self
            .cooldowns
            .get(&provider)
            .filter(|&&time| SystemTime::now() < time)
        {
            return Err(Failure {
                message: "The provider is busy. Try again later.".into(),
                retry_at: cooldown,
            });
        }
        while Instant::now() < self.next {
            if cancelled() {
                return Err(Failure::new("Lookup cancelled.", 0));
            }
            std::thread::sleep(
                self.next
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            );
        }
        if cancelled() {
            return Err(Failure::new("Lookup cancelled.", 0));
        }
        // Cover and information workers share MusicBrainz's one-request-per-second budget.
        let musicbrainz = endpoint(&self.config.musicbrainz, "")
            .is_ok_and(|base| url.origin() == base.origin() && url.path().starts_with(base.path()));
        if musicbrainz {
            loop {
                if cancelled() {
                    return Err(Failure::new("Lookup cancelled.", 0));
                }
                let mut pacing = self.pacing.lock().unwrap_or_else(|e| e.into_inner());
                if SystemTime::now() < pacing.cooldown {
                    return Err(Failure {
                        message: "The provider is busy. Try again later.".into(),
                        retry_at: pacing.cooldown,
                    });
                }
                if Instant::now() >= pacing.next {
                    pacing.next = Instant::now() + self.config.interval;
                    break;
                }
                drop(pacing);
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        let response = self
            .http
            .as_ref()
            .map_err(|_| Failure::new("Could not start the metadata client.", 30))?
            .get(url)
            .send();
        self.next = Instant::now() + self.config.interval;
        let response = response.map_err(|_| {
            Failure::new("Could not reach the provider. Check your connection.", 15)
        })?;
        if matches!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) {
            let delay = retry_delay(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
            );
            let jitter = Duration::from_millis(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as u64
                    % 750,
            );
            let cooldown = SystemTime::now() + delay + jitter;
            self.cooldowns.insert(provider, cooldown);
            if musicbrainz {
                self.pacing
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .cooldown = cooldown;
            }
            return Err(Failure {
                message: "The provider is busy. Try again later.".into(),
                retry_at: cooldown,
            });
        }
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Failure::new(
                format!("Provider returned HTTP {}.", response.status().as_u16()),
                30,
            ));
        }
        if response.content_length().is_some_and(|n| n > max) {
            return Err(Failure::new("The provider response is too large.", 60));
        }
        let mut bytes = Vec::new();
        response
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Failure::new("Could not read the provider response.", 15))?;
        if bytes.len() as u64 > max {
            return Err(Failure::new("The provider response is too large.", 60));
        }
        if cancelled() {
            return Err(Failure::new("Lookup cancelled.", 0));
        }
        Ok(Some(bytes))
    }
    fn json(
        &mut self,
        url: url::Url,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Value>, Failure> {
        self.get(url, 2_000_000, cancelled)?
            .map(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|_| Failure::new("The provider returned an invalid response.", 30))
            })
            .transpose()
    }
    fn cover(
        &mut self,
        query: &Query,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Found>, Failure> {
        let candidates = if let Some(id) = &query.release_id {
            vec![("release", id.clone())]
        } else {
            let mut url = endpoint(&self.config.musicbrainz, "release/")?;
            let search = format!(
                "release:\"{}\" AND artist:\"{}\"",
                lucene(&query.album),
                lucene(&query.artist)
            );
            url.query_pairs_mut().extend_pairs([
                ("query", search.as_str()),
                ("fmt", "json"),
                ("limit", "12"),
            ]);
            let response = self
                .json(url, cancelled)?
                .ok_or_else(|| Failure::new("MusicBrainz search is unavailable.", 30))?;
            release_candidates(query, &response)?
        };
        for (entity, id) in candidates {
            let url = endpoint(&self.config.covers, &format!("{entity}/{id}/front-500"))?;
            let Some(bytes) = self.get(url, 5_000_000, cancelled)? else {
                continue;
            };
            let loader = gtk::gdk_pixbuf::PixbufLoader::new();
            loader.connect_size_prepared(|loader, w, h| {
                if w > 1200 || h > 1200 {
                    let scale = 1200.0 / f64::from(w.max(h));
                    loader.set_size(
                        (f64::from(w) * scale).max(1.0) as i32,
                        (f64::from(h) * scale).max(1.0) as i32,
                    );
                }
            });
            loader
                .write(&bytes)
                .and_then(|_| loader.close())
                .map_err(|_| Failure::new("The cover provider returned an invalid image.", 60))?;
            let pixbuf = loader
                .pixbuf()
                .ok_or_else(|| Failure::new("The cover provider returned an invalid image.", 60))?;
            let image = pixbuf
                .save_to_bufferv("png", &[])
                .map_err(|_| Failure::new("Could not save the downloaded cover.", 5))?;
            fs::create_dir_all(&self.config.cache)
                .map_err(|_| Failure::new("Could not create the metadata cache.", 5))?;
            let path = self.config.cache.join(format!("{}.png", query.key));
            atomic_write(&path, &image)?;
            return Ok(Some(Found::Cover {
                path: path.to_string_lossy().into(),
                source: format!("https://musicbrainz.org/{entity}/{id}"),
            }));
        }
        Ok(None)
    }
    fn lyrics(
        &mut self,
        query: &Query,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Found>, Failure> {
        let mut url = endpoint(&self.config.lrclib, "get")?;
        url.query_pairs_mut()
            .append_pair("track_name", &query.title)
            .append_pair("artist_name", &query.artist);
        if !query.album.is_empty() {
            url.query_pairs_mut()
                .append_pair("album_name", &query.album);
        }
        if (1.0..=3600.0).contains(&query.duration) {
            url.query_pairs_mut()
                .append_pair("duration", &query.duration.to_string());
        }
        if let Some(value) = self.json(url, cancelled)? {
            if let Some(found) = matching_lyrics(query, &value) {
                return Ok(Some(found));
            }
        }
        let mut url = endpoint(&self.config.lrclib, "search")?;
        url.query_pairs_mut()
            .append_pair("track_name", &query.title)
            .append_pair("artist_name", &query.artist);
        let response = self
            .json(url, cancelled)?
            .ok_or_else(|| Failure::new("LRCLIB search is unavailable.", 30))?;
        let results = response
            .as_array()
            .ok_or_else(|| Failure::new("The provider returned an invalid response.", 30))?;
        // Prefer the matching album, then timing data; never accept a different title/version.
        Ok(results
            .iter()
            .filter_map(|v| {
                matching_lyrics(query, v).map(|found| {
                    let album = !query.album.is_empty()
                        && normalize(v["albumName"].as_str().unwrap_or(""))
                            == normalize(&query.album);
                    let synced = matches!(
                        found,
                        Found::Lyrics {
                            synced: Some(_),
                            ..
                        }
                    );
                    ((album, synced), found)
                })
            })
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, found)| found))
    }
}
fn endpoint(base: &str, path: &str) -> Result<url::Url, Failure> {
    let mut base = crate::services::http_url(base)
        .map_err(|_| Failure::new("Invalid metadata provider URL.", 60))?;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join(path)
        .map_err(|_| Failure::new("Invalid metadata provider URL.", 60))
}
fn lucene(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
fn release_candidates(
    query: &Query,
    response: &Value,
) -> Result<Vec<(&'static str, String)>, Failure> {
    let releases = response["releases"]
        .as_array()
        .ok_or_else(|| Failure::new("MusicBrainz returned an invalid response.", 30))?;
    let mut candidates = Vec::new();
    let mut groups = Vec::new();
    for r in releases {
        let score = r["score"]
            .as_u64()
            .or_else(|| r["score"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        let names = r["artist-credit"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v["name"].as_str().or_else(|| v["artist"]["name"].as_str()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let credit = names.join(" ");
        if score < 95
            || normalize(r["title"].as_str().unwrap_or("")) != normalize(&query.album)
            || (normalize(&credit) != normalize(&query.artist)
                && !names
                    .iter()
                    .any(|n| normalize(n) == normalize(&query.artist)))
        {
            continue;
        }
        if r["status"].as_str().is_some_and(|s| s != "Official") {
            continue;
        }
        if let Some(id) = r["id"].as_str().filter(|s| valid_mbid(s)) {
            if !candidates.iter().any(|(_, c)| c == id) {
                candidates.push(("release", id.to_string()));
            }
        }
        if let Some(id) = r["release-group"]["id"].as_str().filter(|s| valid_mbid(s)) {
            if !groups.iter().any(|(_, c)| c == id) {
                groups.push(("release-group", id.to_string()));
            }
        }
    }
    candidates.truncate(3);
    groups.truncate(1);
    candidates.extend(groups);
    Ok(candidates)
}
fn matching_lyrics(query: &Query, value: &Value) -> Option<Found> {
    if normalize(value["trackName"].as_str()?) != normalize(&query.title)
        || normalize(value["artistName"].as_str()?) != normalize(&query.artist)
    {
        return None;
    }
    let duration = value["duration"].as_f64()?;
    if query.duration > 0.0 && (duration - query.duration).abs() > 2.0 {
        return None;
    }
    // Without a known duration, require the album too to distinguish different recordings.
    if query.duration <= 0.0
        && (query.album.is_empty()
            || normalize(value["albumName"].as_str()?) != normalize(&query.album))
    {
        return None;
    }
    let instrumental = value["instrumental"].as_bool().unwrap_or(false);
    let synced = value["syncedLyrics"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    let plain = value["plainLyrics"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| synced.as_ref().map(|s| plain_lrc(s)))
        .unwrap_or_default();
    if !instrumental && plain.trim().is_empty() {
        return None;
    }
    let id = value["id"].as_u64()?;
    Some(Found::Lyrics {
        plain,
        synced,
        source: format!("https://lrclib.net/api/get/{id}"),
        instrumental,
    })
}
/// Keep a plain fallback as well as the original synchronized lyrics.
fn plain_lrc(text: &str) -> String {
    text.lines()
        .filter_map(|line| {
            let mut text = line.trim();
            while text.starts_with('[') {
                let end = text.find(']')?;
                let tag = &text[1..end];
                if !tag.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    return None;
                }
                text = text[end + 1..].trim_start();
            }
            Some(text)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
fn retry_delay(header: Option<&str>) -> Duration {
    header
        .and_then(|s| {
            s.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                httpdate::parse_http_date(s)
                    .ok()
                    .and_then(|t| t.duration_since(SystemTime::now()).ok())
            })
        })
        .unwrap_or(Duration::from_secs(60))
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)
        .and_then(|_| fs::rename(&temp, path))
        .map_err(|_| Failure::new("Could not save the metadata cache.", 5))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
