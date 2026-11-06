//! Missing cover and lyric lookup. All network/cache I/O runs off the GTK thread.
mod providers;
#[cfg(test)]
pub(crate) mod tests;
use crate::model::{Kind, Track, stable_id};
pub use providers::Config;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Field {
    Cover,
    Lyrics,
}
#[derive(Clone, Debug)]
pub struct Query {
    pub field: Field,
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub release_id: Option<String>,
}
impl Query {
    pub fn for_track(track: &Track, field: Field) -> Option<Self> {
        if !matches!(track.kind, Kind::Music | Kind::Spotify | Kind::YoutubeMusic) {
            return None;
        }
        let title = track.title.trim().to_string();
        let album = track.album.trim().to_string();
        let artist = if field == Field::Cover && !track.album_artist.trim().is_empty() {
            &track.album_artist
        } else {
            &track.artist
        }
        .trim()
        .to_string();
        let release_id = track
            .musicbrainz_release_id
            .clone()
            .filter(|s| valid_mbid(s));
        if field == Field::Cover {
            if release_id.is_none()
                && (normalize(&artist).is_empty() || normalize(&album).is_empty())
            {
                return None;
            }
        } else if normalize(&artist).is_empty() || normalize(&title).is_empty() {
            return None;
        }
        let duration = if track.duration.is_finite() {
            track.duration.max(0.0)
        } else {
            0.0
        };
        let identity = match field {
            Field::Cover => release_id
                .as_ref()
                .map(|id| format!("cover-mbid:{id}"))
                .unwrap_or_else(|| format!("cover:{}\0{}", normalize(&artist), normalize(&album))),
            Field::Lyrics => format!(
                "lyrics:{}\0{}\0{}\0{:.0}",
                normalize(&artist),
                normalize(&title),
                normalize(&album),
                duration
            ),
        };
        Some(Self {
            field,
            key: stable_id(&identity),
            title,
            artist,
            album,
            duration,
            release_id,
        })
    }
}
pub fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}
pub fn valid_mbid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
pub fn missing(track: &Track, field: Field) -> bool {
    match field {
        Field::Cover => track.artwork.as_ref().is_none_or(|s| {
            s.trim().is_empty() || (track.artwork_source.is_some() && !Path::new(s).is_file())
        }),
        Field::Lyrics => {
            !track.instrumental && track.lyrics.as_ref().is_none_or(|s| s.trim().is_empty())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Found {
    Cover {
        path: String,
        source: String,
    },
    Lyrics {
        plain: String,
        synced: Option<String>,
        source: String,
        instrumental: bool,
    },
}
impl Found {
    pub fn field(&self) -> Field {
        match self {
            Self::Cover { .. } => Field::Cover,
            Self::Lyrics { .. } => Field::Lyrics,
        }
    }
    pub fn available(&self) -> bool {
        match self {
            Self::Cover { path, .. } => Path::new(path).is_file(),
            Self::Lyrics { .. } => true,
        }
    }
}
pub fn apply(track: &mut Track, key: &str, found: &Found) -> bool {
    let field = found.field();
    if !missing(track, field) || !Query::for_track(track, field).is_some_and(|q| q.key == key) {
        return false;
    }
    match found {
        Found::Cover { path, source } => {
            track.artwork = Some(path.clone());
            track.artwork_source = Some(source.clone());
        }
        Found::Lyrics {
            plain,
            source,
            instrumental,
            ..
        } => {
            track.lyrics = (!plain.is_empty()).then(|| plain.clone());
            track.lyrics_source = Some(source.clone());
            track.instrumental = *instrumental;
        }
    }
    true
}
/// A metadata refresh may replace provider content, but must keep matching downloaded fallbacks.
pub fn preserve_downloaded(old: &Track, new: &mut Track) {
    for field in [Field::Cover, Field::Lyrics] {
        if !missing(new, field)
            || Query::for_track(old, field)
                .zip(Query::for_track(new, field))
                .is_none_or(|(a, b)| a.key != b.key)
        {
            continue;
        }
        match field {
            Field::Cover if old.artwork_source.is_some() => {
                new.artwork.clone_from(&old.artwork);
                new.artwork_source.clone_from(&old.artwork_source);
            }
            Field::Lyrics if old.lyrics_source.is_some() => {
                new.lyrics.clone_from(&old.lyrics);
                new.lyrics_source.clone_from(&old.lyrics_source);
                new.instrumental = old.instrumental;
            }
            _ => {}
        }
    }
}
#[derive(Clone, Debug)]
pub struct Failure {
    pub message: String,
    pub retry_at: SystemTime,
}
impl Failure {
    fn new(message: impl Into<String>, seconds: u64) -> Self {
        Self {
            message: message.into(),
            retry_at: SystemTime::now() + Duration::from_secs(seconds),
        }
    }
    pub fn retryable(&self) -> bool {
        SystemTime::now() >= self.retry_at
    }
}
#[derive(Clone, Debug)]
pub enum Status {
    Pending,
    Found,
    Missing,
    Failed(Failure),
}
pub struct Reply {
    pub query: Query,
    pub epoch: u64,
    pub result: Result<Option<Found>, Failure>,
}
struct Job {
    query: Query,
    epoch: u64,
    force: bool,
}

pub struct Service {
    cover: mpsc::SyncSender<Job>,
    lyrics: mpsc::SyncSender<Job>,
    rx: mpsc::Receiver<Reply>,
    epochs: [Arc<AtomicU64>; 2],
    alive: Arc<AtomicBool>,
    pub status: HashMap<String, Status>,
    pending: HashMap<String, Field>,
}
impl Service {
    pub fn new(config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let epochs = [Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0))];
        let start = |field: Field| {
            let (jobs, receiver) = mpsc::sync_channel::<Job>(8);
            let replies = tx.clone();
            let alive = alive.clone();
            let epoch = epochs[index(field)].clone();
            let config = config.clone();
            std::thread::spawn(move || {
                let mut client = providers::Client::new(config);
                while let Ok(job) = receiver.recv() {
                    if !alive.load(Ordering::Relaxed) {
                        break;
                    }
                    if epoch.load(Ordering::Relaxed) != job.epoch {
                        continue;
                    }
                    let cancelled = || {
                        !alive.load(Ordering::Relaxed) || epoch.load(Ordering::Relaxed) != job.epoch
                    };
                    let result = client.lookup(&job.query, job.force, &cancelled);
                    if !cancelled() {
                        let _ = replies.send(Reply {
                            query: job.query,
                            epoch: job.epoch,
                            result,
                        });
                    }
                }
            });
            jobs
        };
        let cover = start(Field::Cover);
        let lyrics = start(Field::Lyrics);
        Self {
            cover,
            lyrics,
            rx,
            epochs,
            alive,
            status: HashMap::new(),
            pending: HashMap::new(),
        }
    }
    pub fn request(&mut self, query: Query, force: bool) -> bool {
        if let Some(status) = self.status.get(&query.key) {
            if (!force && !matches!(status, Status::Found))
                || matches!(status, Status::Pending)
                || matches!(status, Status::Failed(e) if !e.retryable())
            {
                return false;
            }
        }
        let epoch = self.epochs[index(query.field)].load(Ordering::Relaxed);
        let key = query.key.clone();
        let field = query.field;
        let tx = if query.field == Field::Cover {
            &self.cover
        } else {
            &self.lyrics
        };
        if tx
            .try_send(Job {
                query,
                epoch,
                force,
            })
            .is_err()
        {
            return false;
        }
        self.pending.insert(key.clone(), field);
        self.status.insert(key, Status::Pending);
        true
    }
    pub fn cancel(&mut self, field: Field) {
        self.epochs[index(field)].fetch_add(1, Ordering::Relaxed);
        // Pending jobs can be retried on re-enable. Other completed results remain useful.
        self.pending.retain(|key, pending_field| {
            if *pending_field == field {
                self.status.remove(key);
                false
            } else {
                true
            }
        });
    }
    pub fn poll(&mut self) -> Vec<Reply> {
        let replies: Vec<_> = self
            .rx
            .try_iter()
            .filter(|r| r.epoch == self.epochs[index(r.query.field)].load(Ordering::Relaxed))
            .collect();
        for r in &replies {
            self.pending.remove(&r.query.key);
            self.status.insert(
                r.query.key.clone(),
                match &r.result {
                    Ok(Some(_)) => Status::Found,
                    Ok(None) => Status::Missing,
                    Err(e) => Status::Failed(e.clone()),
                },
            );
        }
        replies
    }
    pub fn stop(&self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}
fn index(field: Field) -> usize {
    if field == Field::Cover { 0 } else { 1 }
}
