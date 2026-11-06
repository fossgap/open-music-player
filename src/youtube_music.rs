//! YouTube Music catalog, account and playlist bridge. Credentials stay in Secret Service.
use crate::{
    model::{Kind, State, Track, YoutubeAccount, YoutubePlaylistLink, stable_id},
    process, storage,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, process::Command, time::Duration};

#[derive(Clone, Debug, Deserialize)]
pub struct Song {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub artwork: Option<String>,
}

impl Song {
    pub fn into_track(self) -> Result<Track, String> {
        if !valid_video_id(&self.video_id) {
            return Err("Invalid YouTube Music track.".into());
        }
        let mut track = Track::new(
            &format!("https://music.youtube.com/watch?v={}", self.video_id),
            &self.title,
            Kind::YoutubeMusic,
        );
        track.artist = self.artist;
        track.album = self.album;
        track.duration = if self.duration.is_finite() {
            self.duration.max(0.0)
        } else {
            0.0
        };
        track.artwork = self
            .artwork
            .filter(|uri| crate::services::http_url(uri).is_ok());
        track.source = "YouTube Music".into();
        Ok(track)
    }
}

#[derive(Debug, Deserialize)]
pub struct Resolved {
    pub uri: String,
    pub headers: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalPlaylist {
    pub local_id: String,
    pub name: String,
    pub video_ids: Vec<String>,
    pub snapshot: String,
    pub valid: bool,
    pub link: Option<YoutubePlaylistLink>,
}

#[derive(Debug, Deserialize)]
pub struct RemotePlaylist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Song>,
    pub editable: bool,
    pub snapshot: String,
    pub unavailable: usize,
    pub local_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SyncResult {
    pub playlists: Vec<RemotePlaylist>,
    pub conflicts: Vec<String>,
    pub failed: Vec<String>,
    pub uploaded: usize,
}

#[derive(Default, Debug)]
pub struct Applied {
    pub synced: usize,
    pub conflicts: usize,
    pub failed: usize,
    pub unavailable: usize,
    pub uploaded: usize,
}

pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn video_id(uri: &str) -> Option<String> {
    let url = crate::services::http_url(uri).ok()?;
    let id = match url.host_str()? {
        "music.youtube.com" | "www.youtube.com" | "youtube.com" | "m.youtube.com"
            if url.path() == "/watch" =>
        {
            url.query_pairs().find(|(k, _)| k == "v")?.1.into_owned()
        }
        "youtu.be" => url.path().trim_start_matches('/').to_string(),
        _ => return None,
    };
    valid_video_id(&id).then_some(id)
}

pub fn snapshot(name: &str, ids: &[String]) -> String {
    stable_id(
        &std::iter::once(name.to_string())
            .chain(ids.iter().cloned())
            .collect::<Vec<_>>()
            .join("\0"),
    )
}

pub fn local_playlist(state: &State, id: &str) -> Option<LocalPlaylist> {
    let playlist = state.playlist(id)?;
    let video_ids: Vec<String> = playlist
        .tracks
        .iter()
        .map(|id| {
            state
                .track(id)
                .and_then(|t| video_id(&t.uri))
                .unwrap_or_else(|| format!("local:{id}"))
        })
        .collect();
    Some(LocalPlaylist {
        local_id: id.into(),
        name: playlist.name.clone(),
        valid: video_ids.len() <= 5000 && video_ids.iter().all(|id| valid_video_id(id)),
        snapshot: snapshot(&playlist.name, &video_ids),
        video_ids,
        link: playlist.youtube.clone(),
    })
}

pub fn linked_playlists(state: &State) -> Vec<LocalPlaylist> {
    state
        .playlists
        .iter()
        .filter(|p| {
            p.youtube.as_ref().is_some_and(|link| {
                state
                    .youtube_account
                    .as_ref()
                    .is_some_and(|a| a.id == link.account_id)
            })
        })
        .filter_map(|p| local_playlist(state, &p.id))
        .collect()
}

fn request<T: DeserializeOwned>(
    account: Option<&YoutubeAccount>,
    input: Value,
    timeout: u64,
) -> Result<T, String> {
    request_cancellable(account, input, timeout, || false)
}

fn request_cancellable<T: DeserializeOwned>(
    account: Option<&YoutubeAccount>,
    mut input: Value,
    timeout: u64,
    cancelled: impl Fn() -> bool,
) -> Result<T, String> {
    let data_dir = storage::data_dir();
    input["namespace"] = json!(stable_id(&data_dir.to_string_lossy()));
    input["account_id"] = json!(account.map(|a| &a.id));
    let installed = data_dir.join("youtube-python/bin/python");
    let python = std::env::var_os("OPEN_AUDIO_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if installed.is_file() {
                installed
            } else {
                "python3".into()
            }
        });
    let output = process::output_with_input_cancellable(
        Command::new(python).args(["-c", include_str!("../resources/youtube_music.py")]),
        &serde_json::to_vec(&input).map_err(|e| e.to_string())?,
        Duration::from_secs(timeout),
        32 * 1024 * 1024,
        cancelled,
    )?;
    let response: Value = serde_json::from_slice(&output)
        .map_err(|_| "Invalid response from YouTube Music support.")?;
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("YouTube Music could not complete the request.")
            .to_string());
    }
    serde_json::from_value(response["data"].clone())
        .map_err(|_| "Incomplete response from YouTube Music support.".into())
}

pub fn connect(
    browser: &str,
    profile: &str,
    cookie_file: Option<&str>,
) -> Result<YoutubeAccount, String> {
    request(
        None,
        json!({"op":"connect","browser":browser,"profile":profile,"cookie_file":cookie_file}),
        120,
    )
}
pub fn disconnect() -> Result<(), String> {
    let _: Value = request(None, json!({"op":"disconnect"}), 60)?;
    Ok(())
}
pub fn search(account: Option<&YoutubeAccount>, query: &str) -> Result<Vec<Track>, String> {
    let songs: Vec<Song> = request(account, json!({"op":"search","query":query}), 90)?;
    songs.into_iter().map(Song::into_track).collect()
}
pub fn resolve(account: Option<&YoutubeAccount>, uri: &str) -> Result<Resolved, String> {
    resolve_cancellable(account, uri, || false)
}
pub fn resolve_cancellable(
    account: Option<&YoutubeAccount>,
    uri: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Resolved, String> {
    let id =
        video_id(uri).ok_or("Choose a YouTube Music track or a music.youtube.com/watch link.")?;
    let mut resolved: Resolved = request_cancellable(
        account,
        json!({"op":"resolve","video_id":id}),
        100,
        cancelled,
    )?;
    crate::services::http_url(&resolved.uri)?;
    resolved.headers.retain(|k, v| {
        ["user-agent", "referer", "origin"].contains(&k.to_ascii_lowercase().as_str())
            && !v.contains(['\r', '\n'])
    });
    Ok(resolved)
}
pub fn synchronize(
    account: &YoutubeAccount,
    playlists: &[LocalPlaylist],
    ignored: &[String],
) -> Result<SyncResult, String> {
    request(
        Some(account),
        json!({"op":"sync","playlists":playlists,"ignored":ignored}),
        900,
    )
}
pub fn publish(
    account: &YoutubeAccount,
    playlist: &LocalPlaylist,
) -> Result<RemotePlaylist, String> {
    if !playlist.valid || playlist.video_ids.is_empty() {
        return Err("Only playlists containing YouTube Music tracks can be published.".into());
    }
    let mut input = serde_json::to_value(playlist).map_err(|e| e.to_string())?;
    input["op"] = json!("publish");
    request(Some(account), input, 150)
}

pub fn reload(
    account: &YoutubeAccount,
    playlist: &LocalPlaylist,
) -> Result<RemotePlaylist, String> {
    let link = playlist
        .link
        .as_ref()
        .ok_or("This playlist is not linked to YouTube Music.")?;
    request(
        Some(account),
        json!({"op":"pull", "playlist_id":link.id, "local_id":playlist.local_id}),
        120,
    )
}

/// Merge a completed job without overwriting edits made while its worker was running.
pub fn apply_sync(
    state: &mut State,
    account_id: &str,
    before: &[LocalPlaylist],
    result: SyncResult,
) -> Applied {
    let mut applied = Applied {
        conflicts: result.conflicts.len(),
        failed: result.failed.len(),
        uploaded: result.uploaded,
        ..Applied::default()
    };
    if !state
        .youtube_account
        .as_ref()
        .is_some_and(|a| a.id == account_id)
    {
        return applied;
    }
    for remote in result.playlists {
        if state.youtube_ignored_playlists.contains(&remote.id) {
            continue;
        }
        let existing_id = remote.local_id.clone().or_else(|| {
            state
                .playlists
                .iter()
                .find(|p| {
                    p.youtube
                        .as_ref()
                        .is_some_and(|l| l.id == remote.id && l.account_id == account_id)
                })
                .map(|p| p.id.clone())
        });
        let captured = existing_id
            .as_ref()
            .and_then(|id| before.iter().find(|p| &p.local_id == id));
        let current = existing_id
            .as_ref()
            .and_then(|id| local_playlist(state, id));
        if existing_id.is_some() && current.is_none() {
            continue;
        } // deleted during synchronization
        let edited_in_flight = current
            .as_ref()
            .zip(captured)
            .is_some_and(|(now, old)| now.snapshot != old.snapshot);
        if edited_in_flight && !captured.is_some_and(|old| old.snapshot == remote.snapshot) {
            applied.conflicts += 1;
            continue;
        }
        let tracks: Result<Vec<_>, _> = remote.tracks.into_iter().map(Song::into_track).collect();
        let Ok(tracks) = tracks else {
            applied.failed += 1;
            continue;
        };
        let ids = tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        state.merge(tracks);
        let unchanged = current.as_ref().is_some_and(|p| {
            p.link.as_ref().is_some_and(|l| {
                l.remote_snapshot == remote.snapshot && l.local_snapshot == p.snapshot
            })
        });
        let id = if let Some(id) = existing_id {
            if !edited_in_flight && !unchanged {
                let name = state
                    .playlist_name(&remote.name, Some(&id))
                    .unwrap_or_else(|_| state.unique_playlist_name(&remote.name));
                if let Some(p) = state.playlists.iter_mut().find(|p| p.id == id) {
                    p.name = name;
                    p.tracks = ids;
                }
            }
            id
        } else {
            let name = state.unique_playlist_name(&remote.name);
            let Ok(id) = state.create_playlist(&name, ids) else {
                applied.failed += 1;
                continue;
            };
            id
        };
        let local_snapshot = if edited_in_flight {
            captured.unwrap().snapshot.clone()
        } else {
            local_playlist(state, &id).unwrap().snapshot
        };
        state
            .playlists
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .youtube = Some(YoutubePlaylistLink {
            id: remote.id,
            account_id: account_id.into(),
            editable: remote.editable,
            remote_snapshot: remote.snapshot,
            local_snapshot,
        });
        applied.synced += 1;
        applied.unavailable += remote.unavailable;
    }
    applied
}

pub fn clear_account(state: &mut State) {
    state.youtube_account = None;
    state.youtube_ignored_playlists.clear();
    state.playlists.retain(|p| p.youtube.is_none());
    state.tracks.retain(|t| t.kind != Kind::YoutubeMusic);
    state.sanitize();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn remote(name: &str) -> RemotePlaylist {
        RemotePlaylist {
            id: "PL_test".into(),
            name: name.into(),
            tracks: vec![Song {
                video_id: "abcdefghijk".into(),
                title: "Song".into(),
                artist: "Artist".into(),
                album: "Album".into(),
                duration: 100.0,
                artwork: None,
            }],
            editable: true,
            snapshot: snapshot(name, &["abcdefghijk".into()]),
            unavailable: 0,
            local_id: None,
        }
    }
    fn state() -> State {
        State {
            youtube_account: Some(YoutubeAccount {
                id: "@test".into(),
                name: "Test".into(),
            }),
            ..State::default()
        }
    }
    fn apply(state: &mut State, before: &[LocalPlaylist], remote: RemotePlaylist) -> Applied {
        apply_sync(
            state,
            "@test",
            before,
            SyncResult {
                playlists: vec![remote],
                ..SyncResult::default()
            },
        )
    }
    #[test]
    fn only_valid_youtube_track_urls_are_resolved() {
        assert_eq!(
            video_id("https://music.youtube.com/watch?v=abcdefghijk&list=PL123"),
            Some("abcdefghijk".into())
        );
        for uri in [
            "https://youtube.com.evil/watch?v=abcdefghijk",
            "https://music.youtube.com/watch?v=short",
            "file:///abcdefghijk",
            "https://user:pass@youtube.com/watch?v=abcdefghijk",
        ] {
            assert!(video_id(uri).is_none());
        }
    }
    #[test]
    fn sync_preserves_local_collisions_and_in_flight_edits() {
        let mut state = state();
        state.create_playlist("Mix", vec![]).unwrap();
        assert_eq!(apply(&mut state, &[], remote("Mix")).synced, 1);
        let id = state.playlists[1].id.clone();
        assert_eq!(state.playlist(&id).unwrap().name, "Mix (2)");
        let before = linked_playlists(&state);
        apply(&mut state, &before, remote("Mix"));
        assert_eq!(state.playlist(&id).unwrap().name, "Mix (2)");
        state.rename_playlist(&id, "My edit").unwrap();
        assert_eq!(
            apply(&mut state, &before, remote("Online edit")).conflicts,
            1
        );
        assert_eq!(state.playlist(&id).unwrap().name, "My edit");
    }
    #[test]
    fn disconnected_or_deleted_playlists_are_not_resurrected() {
        let mut state = state();
        apply(&mut state, &[], remote("Mix"));
        let before = linked_playlists(&state);
        let mut response = remote("Mix");
        response.local_id = Some(before[0].local_id.clone());
        state.playlists.clear();
        apply(&mut state, &before, response);
        assert!(state.playlists.is_empty());
        clear_account(&mut state);
        apply(&mut state, &[], remote("Mix"));
        assert!(state.tracks.is_empty());
        assert!(state.playlists.is_empty());
    }
    #[test]
    fn old_libraries_need_no_credentials_or_migration() {
        let state: State =
            serde_json::from_str(r#"{"playlists":[{"id":"old","name":"Old","tracks":[]}]}"#)
                .unwrap();
        assert!(state.youtube_account.is_none());
        assert!(state.playlists[0].youtube.is_none());
    }
}
