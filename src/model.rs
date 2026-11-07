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
    pub album_artist: String,
    pub musicbrainz_release_id: Option<String>,
    pub musicbrainz_recording_id: Option<String>,
    pub musicbrainz_artist_id: Option<String>,
    pub title_from_filename: bool,
    pub metadata_version: u8,
    pub music_info: Option<crate::enrichment::Information>,
    pub duration: f64,
    pub number: u32,
    pub kind: Kind,
    pub artwork: Option<String>,
    pub lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
    pub lyrics_timing_checked: bool,
    pub audio_details: crate::audio_info::Details,
    pub artwork_source: Option<String>,
    pub lyrics_source: Option<String>,
    pub instrumental: bool,
    pub favorite: bool,
    pub source: String,
    pub modified: u64,
    pub size: u64,
    pub download_path: Option<PathBuf>,
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
pub struct SavedAudio {
    pub id: String,
    pub title: String,
    pub source_id: String,
    pub path: PathBuf,
    pub recording: bool,
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayGain {
    #[default]
    Off,
    Track,
    Album,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlaybackSettings {
    pub gapless: bool,
    pub crossfade_seconds: u32,
    pub replay_gain: ReplayGain,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            gapless: true,
            crossfade_seconds: 0,
            replay_gain: ReplayGain::Off,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub tracks: Vec<Track>,
    pub folders: Vec<PathBuf>,
    pub ignored_local: Vec<String>,
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
    pub fetch_covers: bool,
    pub fetch_lyrics: bool,
    pub fetch_information: bool,
    pub refresh_minutes: u32,
    pub watch_music: bool,
    pub playback_rate: f64,
    pub show_remaining: bool,
    pub download_directory: Option<PathBuf>,
    pub saved_audio: Vec<SavedAudio>,
    pub playback: PlaybackSettings,
    #[serde(skip)]
    pub(crate) track_index: std::collections::HashMap<String, usize>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            tracks: vec![],
            folders: vec![],
            ignored_local: vec![],
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
            fetch_covers: true,
            fetch_lyrics: true,
            fetch_information: true,
            refresh_minutes: 0,
            watch_music: true,
            playback_rate: 1.0,
            show_remaining: false,
            download_directory: None,
            saved_audio: Vec::new(),
            playback: PlaybackSettings::default(),
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
        let ignored: HashSet<_> = self.ignored_local.iter().map(String::as_str).collect();
        let mut indices: std::collections::HashMap<String, usize> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        for mut track in incoming {
            if track.kind == Kind::Music && ignored.contains(track.uri.as_str()) {
                continue;
            }
            if let Some(&i) = indices.get(&track.id) {
                track.favorite = self.tracks[i].favorite;
                track.download_path = self.tracks[i].download_path.clone();
                crate::enrichment::preserve_downloaded(&self.tracks[i], &mut track);
                self.tracks[i] = track;
            } else {
                indices.insert(track.id.clone(), self.tracks.len());
                self.tracks.push(track);
            }
        }
        self.track_index = indices;
    }

    pub fn sanitize(&mut self) {
        self.playback_rate = if self.playback_rate.is_finite() {
            self.playback_rate.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self.refresh_minutes = self.refresh_minutes.min(1440);
        self.playback.crossfade_seconds = self.playback.crossfade_seconds.min(12);
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
        self.adjacent_from(self.current.as_deref(), forward, automatic)
    }

    pub fn adjacent_from(
        &self,
        current: Option<&str>,
        forward: bool,
        automatic: bool,
    ) -> Option<String> {
        if automatic && self.repeat == Repeat::One {
            return current.map(str::to_string);
        }
        if self.queue.is_empty() {
            return None;
        }
        let index = current.and_then(|id| self.queue.iter().position(|i| i == id));
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
        self.forget_many([id]);
    }

    /// Explicit library removal must survive automatic folder refreshes.
    pub fn remove_from_library(&mut self, ids: &[String]) {
        let selected: HashSet<_> = ids.iter().collect();
        let mut ignored: HashSet<_> = self.ignored_local.iter().cloned().collect();
        for track in &self.tracks {
            if selected.contains(&track.id)
                && track.kind == Kind::Music
                && crate::library::local_path(&track.uri).is_some()
                && ignored.insert(track.uri.clone())
            {
                self.ignored_local.push(track.uri.clone());
            }
        }
        self.forget_many(ids.iter().map(String::as_str));
    }

    pub fn forget_many<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) {
        let ids: HashSet<&str> = ids.into_iter().collect();
        self.tracks.retain(|t| !ids.contains(t.id.as_str()));
        self.queue.retain(|t| !ids.contains(t.as_str()));
        for list in &mut self.playlists {
            list.tracks.retain(|t| !ids.contains(t.as_str()));
        }
        self.podcast_positions
            .retain(|id, _| !ids.contains(id.as_str()));
        if self.current.as_deref().is_some_and(|id| ids.contains(id)) {
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
    #[test]
    fn saved_audio_preferences_migrate_and_survive_metadata_refresh() {
        let mut state: State = serde_json::from_str("{}").unwrap();
        assert_eq!(state.playback_rate, 1.0);
        assert!(!state.show_remaining);
        assert!(state.saved_audio.is_empty());
        assert!(state.download_directory.is_none());
        state.playback_rate = 1.5;
        state.show_remaining = true;
        state.download_directory = Some("/tmp/saved audio".into());
        let track = Track::new("https://example.test/a.mp3", "Podcast", Kind::Podcast);
        let mut downloaded = track.clone();
        downloaded.download_path = Some("/tmp/saved audio/a.mp3".into());
        state.merge(vec![downloaded]);
        state.merge(vec![track.clone()]);
        assert!(state.track(&track.id).unwrap().download_path.is_some());
        state.saved_audio.push(SavedAudio {
            id: "1".into(),
            title: track.title,
            source_id: track.id,
            path: "/tmp/saved audio/a.mp3".into(),
            recording: false,
        });
        let restored: State =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.playback_rate, 1.5);
        assert!(restored.show_remaining);
        assert_eq!(restored.download_directory, state.download_directory);
        assert_eq!(restored.saved_audio[0].path, state.saved_audio[0].path);
        state.playback_rate = f64::NAN;
        state.sanitize();
        assert_eq!(state.playback_rate, 1.0);
        state.playback_rate = 20.0;
        state.sanitize();
        assert_eq!(state.playback_rate, 2.0);
    }
    #[test]
    fn refresh_preferences_default_to_manual_and_survive_storage() {
        let mut state: State = serde_json::from_str("{}").unwrap();
        assert_eq!(state.refresh_minutes, 0);
        assert!(state.watch_music);
        state.refresh_minutes = 45;
        state.watch_music = false;
        let restored: State =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.refresh_minutes, 45);
        assert!(!restored.watch_music);
        state.refresh_minutes = u32::MAX;
        state.sanitize();
        assert_eq!(state.refresh_minutes, 1440);
    }

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
    fn playback_settings_migrate_round_trip_and_bound_crossfade() {
        let mut state: State = serde_json::from_str(r#"{"volume":0.5}"#).unwrap();
        assert!(state.playback.gapless);
        assert_eq!(state.playback.crossfade_seconds, 0);
        assert_eq!(state.playback.replay_gain, ReplayGain::Off);
        state.playback = PlaybackSettings {
            gapless: false,
            crossfade_seconds: 900,
            replay_gain: ReplayGain::Album,
        };
        state.sanitize();
        assert_eq!(state.playback.crossfade_seconds, 12);
        let restored: State =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.playback, state.playback);
        assert_eq!(restored.volume, 0.5);
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

    #[test]
    fn library_removal_survives_rescans_and_state_restoration() {
        let mut state = State::default();
        let track = Track::new(
            "file:///tmp/library-removal.wav",
            "Keep on disk",
            Kind::Music,
        );
        state.merge(vec![track.clone()]);
        state.queue.push(track.id.clone());
        state
            .create_playlist("Favorites", vec![track.id.clone()])
            .unwrap();
        state.current = Some(track.id.clone());
        state.remove_from_library(std::slice::from_ref(&track.id));
        assert!(state.current.is_none() && state.queue.is_empty());
        assert!(state.playlists[0].tracks.is_empty());
        let mut restored: State =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        restored.merge(vec![track.clone()]);
        assert!(restored.track(&track.id).is_none());
        restored.ignored_local.clear();
        restored.merge(vec![track.clone()]);
        assert!(restored.track(&track.id).is_some());
    }
}
