//! Ephemeral, ranked search across saved audio and the existing discovery providers.
pub mod providers;
#[cfg(test)]
mod tests;

use crate::{
    model::{Kind, State},
    services::PodcastShow,
};
use gtk::glib::{self, Unichar};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Music,
    Albums,
    Playlists,
    Episodes,
    Subscriptions,
    Stations,
    Streams,
    Apple,
    Gpodder,
    Radio,
    Youtube,
    Spotify,
    Feed,
}

impl Group {
    pub const ONLINE: [Self; 5] = [
        Self::Apple,
        Self::Gpodder,
        Self::Radio,
        Self::Youtube,
        Self::Spotify,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Music => "Music in your collection",
            Self::Albums => "Albums",
            Self::Playlists => "Playlists",
            Self::Episodes => "Saved podcast episodes",
            Self::Subscriptions => "Podcast subscriptions",
            Self::Stations => "Saved stations",
            Self::Streams => "Saved streams",
            Self::Apple => "Apple Podcasts",
            Self::Gpodder => "gPodder",
            Self::Radio => "Radio Browser",
            Self::Youtube => "YouTube Music",
            Self::Spotify => "Spotify",
            Self::Feed => "Podcast episodes",
        }
    }
    pub fn category(self) -> u32 {
        match self {
            Self::Music | Self::Youtube | Self::Spotify => 1,
            Self::Albums => 2,
            Self::Playlists => 3,
            Self::Episodes | Self::Subscriptions | Self::Apple | Self::Gpodder | Self::Feed => 4,
            Self::Stations | Self::Radio => 5,
            Self::Streams => 6,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Target {
    Track(String),
    Album(String),
    Playlist(String),
    Show(PodcastShow),
}

#[derive(Clone, Debug)]
pub struct Document {
    pub key: String,
    pub group: Group,
    pub title: String,
    pub detail: String,
    pub artwork: Option<String>,
    pub target: Target,
    text: String,
}

pub fn remote_track(track: &crate::model::Track, group: Group) -> Document {
    Document {
        key: format!("track:{}", track.id),
        group,
        title: track.title.clone(),
        detail: [track.artist.as_str(), track.album.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        artwork: track.artwork.clone(),
        target: Target::Track(track.id.clone()),
        text: String::new(),
    }
}
pub fn show_document(show: PodcastShow, group: Group) -> Document {
    Document {
        key: format!("show:{}", crate::model::stable_id(&show.feed_url)),
        group,
        title: show.title.clone(),
        detail: show.author.clone(),
        artwork: show.artwork.clone(),
        target: Target::Show(show),
        text: String::new(),
    }
}

pub fn snapshot(state: &State) -> Arc<Vec<Document>> {
    let mut documents = Vec::with_capacity(state.tracks.len() + state.playlists.len());
    let mut albums = HashSet::new();
    for track in &state.tracks {
        let group = match track.kind {
            Kind::Podcast => Group::Episodes,
            Kind::Radio => Group::Stations,
            Kind::Stream => Group::Streams,
            _ => Group::Music,
        };
        let detail = [
            track.artist.as_str(),
            track.album.as_str(),
            if matches!(track.kind, Kind::Spotify | Kind::YoutubeMusic) {
                &track.source
            } else {
                ""
            },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        documents.push(Document {
            key: format!("track:{}", track.id),
            group,
            title: track.title.clone(),
            detail,
            artwork: track.artwork.clone(),
            target: Target::Track(track.id.clone()),
            text: format!(
                "{} {} {} {} {} {} {} {} {}",
                track.title,
                track.artist,
                track.album,
                track.album_artist,
                track.genres.join(" "),
                track.composers.join(" "),
                track.date,
                track.source,
                if matches!(track.kind, Kind::Stream | Kind::Radio) {
                    &track.uri
                } else {
                    ""
                }
            ),
        });
        if track.kind == Kind::Music
            && !track.album.trim().is_empty()
            && albums.insert(track.album_key())
        {
            documents.push(Document {
                key: format!("album:{}", track.id),
                group: Group::Albums,
                title: track.album.clone(),
                detail: track.album_key().0,
                artwork: track.artwork.clone(),
                target: Target::Album(track.id.clone()),
                text: format!("{} {}", track.album, track.album_key().0),
            });
        }
    }
    for playlist in &state.playlists {
        documents.push(Document {
            key: format!("playlist:{}", playlist.id),
            group: Group::Playlists,
            title: playlist.name.clone(),
            detail: if playlist.spotify.is_some() {
                "Spotify"
            } else if playlist.youtube.is_some() {
                "YouTube Music"
            } else {
                ""
            }
            .into(),
            artwork: playlist
                .tracks
                .iter()
                .filter_map(|id| state.track(id))
                .find_map(|t| t.artwork.clone()),
            target: Target::Playlist(playlist.id.clone()),
            text: playlist.name.clone(),
        });
    }
    for subscription in &state.subscriptions {
        let artwork = state
            .tracks
            .iter()
            .find(|t| t.kind == Kind::Podcast && t.source == subscription.url)
            .and_then(|t| t.artwork.clone());
        let show = PodcastShow {
            id: crate::model::stable_id(&subscription.url),
            title: subscription.title.clone(),
            author: String::new(),
            feed_url: subscription.url.clone(),
            artwork: artwork.clone(),
            website: None,
        };
        documents.push(Document {
            key: format!("show:{}", show.id),
            group: Group::Subscriptions,
            title: show.title.clone(),
            detail: String::new(),
            artwork,
            target: Target::Show(show),
            text: format!("{} {}", subscription.title, subscription.url),
        });
    }
    Arc::new(documents)
}

pub fn normalize(text: &str) -> String {
    glib::normalize(
        glib::casefold(text.replace('\0', "")),
        glib::NormalizeMode::Default,
    )
    .chars()
    .filter(|c| !c.is_mark())
    .collect::<String>()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

#[derive(Default, Debug)]
pub struct Matches {
    pub items: Vec<Document>,
    pub total: usize,
}

pub type Results = BTreeMap<Group, Matches>;

fn rank(title: &str, text: &str, query: &str) -> Option<u8> {
    if query.is_empty() || !query.split_whitespace().all(|word| text.contains(word)) {
        return None;
    }
    Some(if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if title.contains(query) {
        2
    } else if query.split_whitespace().all(|word| title.contains(word)) {
        3
    } else {
        4
    })
}

pub struct LocalReply {
    pub generation: u64,
    pub results: Results,
}
struct LocalJob {
    generation: u64,
    documents: Arc<Vec<Document>>,
    query: String,
    limit: usize,
}

pub struct LocalSearch {
    tx: mpsc::Sender<LocalJob>,
    rx: mpsc::Receiver<LocalReply>,
    generation: Arc<AtomicU64>,
}
type NormalizedDocuments = (Arc<Vec<Document>>, Vec<(String, String)>);

impl Default for LocalSearch {
    fn default() -> Self {
        let (tx, jobs) = mpsc::channel::<LocalJob>();
        let (replies, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        std::thread::spawn(move || {
            let mut index: Option<NormalizedDocuments> = None;
            while let Ok(mut job) = jobs.recv() {
                while let Ok(newer) = jobs.try_recv() {
                    job = newer;
                }
                if current.load(Ordering::Relaxed) != job.generation {
                    continue;
                }
                if index
                    .as_ref()
                    .is_none_or(|(docs, _)| !Arc::ptr_eq(docs, &job.documents))
                {
                    let mut normalized = Vec::with_capacity(job.documents.len());
                    for (position, document) in job.documents.iter().enumerate() {
                        if position % 128 == 0 && current.load(Ordering::Relaxed) != job.generation
                        {
                            break;
                        }
                        normalized.push((normalize(&document.title), normalize(&document.text)));
                    }
                    if current.load(Ordering::Relaxed) != job.generation {
                        continue;
                    }
                    index = Some((job.documents.clone(), normalized));
                }
                let query = normalize(&job.query);
                let normalized = &index.as_ref().unwrap().1;
                let mut groups: BTreeMap<Group, Vec<(u8, &str, usize)>> = BTreeMap::new();
                for (position, document) in job.documents.iter().enumerate() {
                    if position % 128 == 0 && current.load(Ordering::Relaxed) != job.generation {
                        break;
                    }
                    let (title, text) = &normalized[position];
                    if let Some(score) = rank(title, text, &query) {
                        groups
                            .entry(document.group)
                            .or_default()
                            .push((score, title, position));
                    }
                }
                if current.load(Ordering::Relaxed) != job.generation {
                    continue;
                }
                let results = groups
                    .into_iter()
                    .map(|(group, mut matches)| {
                        matches.sort_unstable();
                        let total = matches.len();
                        let items = matches
                            .into_iter()
                            .take(job.limit)
                            .map(|(_, _, position)| job.documents[position].clone())
                            .collect();
                        (group, Matches { items, total })
                    })
                    .collect();
                let _ = replies.send(LocalReply {
                    generation: job.generation,
                    results,
                });
            }
        });
        Self { tx, rx, generation }
    }
}

impl LocalSearch {
    pub fn search(&self, documents: Arc<Vec<Document>>, query: String, limit: usize) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(LocalJob {
            generation,
            documents,
            query,
            limit,
        });
    }
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
    pub fn poll(&self) -> Option<Results> {
        self.rx
            .try_iter()
            .filter(|r| r.generation == self.generation.load(Ordering::Relaxed))
            .last()
            .map(|r| r.results)
    }
}

impl Drop for LocalSearch {
    fn drop(&mut self) {
        self.cancel();
    }
}
