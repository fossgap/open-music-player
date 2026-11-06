use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Music,
    Podcast,
    Radio,
    Stream,
    YoutubeMusic,
    Spotify,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub number: u32,
    pub kind: Kind,
    pub artwork: Option<String>,
    pub lyrics: Option<String>,
    pub favorite: bool,
    pub source: String,
    pub modified: u64,
    pub size: u64,
}

impl Track {
    pub fn new(uri: &str, title: &str, kind: Kind) -> Self {
        Self {
            id: stable_id(uri),
            uri: uri.into(),
            title: title.into(),
            kind,
            ..Self::default()
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!("{} {} {}", self.title, self.artist, self.album).to_lowercase();
        query
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_lowercase()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Subscription {
    pub url: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Playlist {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub tracks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub youtube: Option<YoutubePlaylistLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotify: Option<SpotifyPlaylistLink>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct YoutubeAccount {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpotifyAccount {
    pub id: String,
    pub name: String,
}

pub type SpotifyPlaylistLink = YoutubePlaylistLink;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct YoutubePlaylistLink {
    pub id: String,
    pub account_id: String,
    pub editable: bool,
    pub remote_snapshot: String,
    pub local_snapshot: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub tracks: Vec<Track>,
    pub folders: Vec<PathBuf>,
    pub subscriptions: Vec<Subscription>,
    pub playlists: Vec<Playlist>,
    pub queue: Vec<String>,
    pub current: Option<String>,
    pub position: f64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub language: String,
    pub background: bool,
    pub podcast_positions: std::collections::HashMap<String, f64>,
    pub youtube_account: Option<YoutubeAccount>,
    pub youtube_ignored_playlists: Vec<String>,
    pub spotify_account: Option<SpotifyAccount>,
    pub spotify_ignored_playlists: Vec<String>,
    #[serde(skip)]
    pub(crate) track_index: std::collections::HashMap<String, usize>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            tracks: vec![],
            folders: vec![],
            subscriptions: vec![],
            playlists: vec![],
            queue: vec![],
            current: None,
            position: 0.0,
            volume: 0.75,
            shuffle: false,
            repeat: Repeat::Off,
            language: "system".into(),
            background: true,
            podcast_positions: Default::default(),
            youtube_account: None,
            youtube_ignored_playlists: vec![],
            spotify_account: None,
            spotify_ignored_playlists: vec![],
            track_index: Default::default(),
        }
    }
}

impl State {
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.track_index
            .get(id)
            .and_then(|&index| self.tracks.get(index))
            .filter(|t| t.id == id)
            .or_else(|| self.tracks.iter().find(|t| t.id == id))
    }
    pub fn current_track(&self) -> Option<&Track> {
        self.current.as_deref().and_then(|id| self.track(id))
    }

    pub fn merge(&mut self, incoming: Vec<Track>) {
        let mut indices: std::collections::HashMap<String, usize> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        for mut track in incoming {
            if let Some(&i) = indices.get(&track.id) {
                track.favorite = self.tracks[i].favorite;
                self.tracks[i] = track;
            } else {
                indices.insert(track.id.clone(), self.tracks.len());
                self.tracks.push(track);
            }
        }
        self.track_index = indices;
    }

    pub fn sanitize(&mut self) {
        let mut ids = HashSet::new();
        self.tracks
            .retain(|t| !t.id.is_empty() && !t.uri.is_empty() && ids.insert(t.id.clone()));
        self.queue.retain(|id| ids.contains(id));
        let mut playlist_ids = HashSet::new();
        for (index, list) in self.playlists.iter_mut().enumerate() {
            if list.id.is_empty() || !playlist_ids.insert(list.id.clone()) {
                list.id = stable_id(&format!("playlist:{index}:{}", list.name));
                while !playlist_ids.insert(list.id.clone()) {
                    list.id.push('p');
                }
            }
            list.tracks.retain(|id| ids.contains(id));
        }
        if self.current.as_ref().is_some_and(|id| !ids.contains(id)) {
            self.current = None;
            self.position = 0.0;
        }
        self.volume = if self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            0.75
        };
        self.position = if self.position.is_finite() {
            self.position.max(0.0)
        } else {
            0.0
        };
        self.reindex();
    }

    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn playlist_name(&self, name: &str, except: Option<&str>) -> Result<String, &'static str> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a playlist name.");
        }
        if name.chars().count() > 100 {
            return Err("Use a name with 100 characters or fewer.");
        }
        if self
            .playlists
            .iter()
            .any(|p| Some(p.id.as_str()) != except && p.name.to_lowercase() == name.to_lowercase())
        {
            return Err("A playlist with this name already exists.");
        }
        Ok(name.into())
    }

    pub fn create_playlist(
        &mut self,
        name: &str,
        tracks: Vec<String>,
    ) -> Result<String, &'static str> {
        let name = self.playlist_name(name, None)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut id = stable_id(&format!("playlist:{name}:{nonce}"));
        while self.playlist(&id).is_some() {
            id.push('p');
        }
        self.playlists.push(Playlist {
            id: id.clone(),
            name,
            tracks,
            youtube: None,
            spotify: None,
        });
        Ok(id)
    }

    pub fn rename_playlist(&mut self, id: &str, name: &str) -> Result<(), &'static str> {
        let name = self.playlist_name(name, Some(id))?;
        let playlist = self
            .playlists
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Playlist not found.")?;
        playlist.name = name;
        Ok(())
    }

    pub fn unique_playlist_name(&self, name: &str) -> String {
        let base: String = name.trim().chars().take(90).collect();
        let base = if base.is_empty() { "Playlist" } else { &base };
        let mut candidate = base.to_string();
        let mut suffix = 2;
        while self.playlist_name(&candidate, None).is_err() {
            candidate = format!("{base} ({suffix})");
            suffix += 1;
        }
        candidate
    }

    pub fn adjacent(&self, forward: bool, automatic: bool) -> Option<String> {
        if automatic && self.repeat == Repeat::One {
            return self.current.clone();
        }
        if self.queue.is_empty() {
            return None;
        }
        let index = self
            .current
            .as_ref()
            .and_then(|id| self.queue.iter().position(|i| i == id));
        let next = match (index, forward) {
            (None, _) => 0,
            (Some(i), true) if i + 1 < self.queue.len() => i + 1,
            (Some(i), false) if i > 0 => i - 1,
            (_, true) if self.repeat == Repeat::All => 0,
            (_, false) if self.repeat == Repeat::All => self.queue.len() - 1,
            _ => return None,
        };
        self.queue.get(next).cloned()
    }

    pub fn forget(&mut self, id: &str) {
        self.tracks.retain(|t| t.id != id);
        self.queue.retain(|t| t != id);
        for list in &mut self.playlists {
            list.tracks.retain(|t| t != id);
        }
        self.podcast_positions.remove(id);
        if self.current.as_deref() == Some(id) {
            self.current = None;
            self.position = 0.0;
        }
        self.reindex();
    }

    fn reindex(&mut self) {
        self.track_index = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
    }
}

pub fn stable_id(value: &str) -> String {
    let hash = value.bytes().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("t{hash:016x}")
}

pub fn duration(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn shuffle<T>(items: &mut [T]) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        | 1;
    for i in (1..items.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        items.swap(i, seed as usize % (i + 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        State {
            queue: vec!["a".into(), "b".into()],
            current: Some("b".into()),
            ..State::default()
        }
    }
    #[test]
    fn queue_boundaries_and_repeat() {
        let mut s = state();
        assert_eq!(s.adjacent(true, true), None);
        assert_eq!(s.adjacent(false, false), Some("a".into()));
        s.repeat = Repeat::All;
        assert_eq!(s.adjacent(true, true), Some("a".into()));
        s.repeat = Repeat::One;
        assert_eq!(s.adjacent(true, true), Some("b".into()));
        assert_eq!(s.adjacent(true, false), None);
    }
    #[test]
    fn merge_preserves_favorites_and_deduplicates() {
        let mut s = State::default();
        let mut t = Track::new("file:///music.flac", "Old title", Kind::Music);
        t.favorite = true;
        s.merge(vec![t]);
        let t = Track::new("file:///music.flac", "New title", Kind::Music);
        s.merge(vec![t.clone(), t]);
        assert_eq!(s.tracks.len(), 1);
        assert!(s.tracks[0].favorite);
        assert_eq!(s.tracks[0].title, "New title");
    }
    #[test]
    fn search_matches_all_words_case_insensitively() {
        let t = Track {
            title: "Blue Moon".into(),
            artist: "Billie Holiday".into(),
            ..Track::default()
        };
        assert!(t.matches("BILLIE moon"));
        assert!(!t.matches("billie sun"));
    }
    #[test]
    fn removal_cleans_queue_playlists_and_resume() {
        let mut s = state();
        s.playlists.push(Playlist {
            id: "playlist".into(),
            name: "Set".into(),
            tracks: s.queue.clone(),
            youtube: None,
            spotify: None,
        });
        s.forget("b");
        assert!(s.current.is_none());
        assert_eq!(s.queue, vec!["a"]);
        assert_eq!(s.playlists[0].tracks, vec!["a"]);
    }
    #[test]
    fn formats_time() {
        assert_eq!(duration(3672.0), "1:01:12");
        assert_eq!(duration(65.0), "1:05");
    }

    #[test]
    fn legacy_playlists_get_stable_ids_without_losing_membership() {
        let mut state: State = serde_json::from_str(r#"{"tracks":[{"id":"track","uri":"file:///song.flac"}],"playlists":[{"name":"Old mix","tracks":["track"]},{"name":"Other","tracks":[]}]}"#).unwrap();
        state.sanitize();
        let id = state.playlists[0].id.clone();
        assert!(!id.is_empty());
        state.rename_playlist(&id, "New name").unwrap();
        state.sanitize();
        assert_eq!(state.playlist(&id).unwrap().tracks, vec!["track"]);
        let copy: State = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(copy.playlists[0].id, id);
    }

    #[test]
    fn playlist_names_are_validated_without_merging_existing_lists() {
        let mut state = State::default();
        let id = state.create_playlist("  Evening  ", vec![]).unwrap();
        assert!(state.create_playlist("evening", vec![]).is_err());
        assert!(state.rename_playlist(&id, " ").is_err());
        state.rename_playlist(&id, "EVENING").unwrap();
        assert_eq!(state.unique_playlist_name("Evening"), "Evening (2)");
        assert_eq!(state.playlists.len(), 1);
    }
}
