use super::{auth, track_id};
use crate::{
    model::{Kind, SpotifyAccount, SpotifyPlaylistLink, State, Track},
    youtube_music::snapshot,
};
use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

pub struct Api {
    pub http: Client,
    pub stored: auth::Stored,
    access: String,
    expires: Instant,
    #[cfg(test)]
    test_origin: Option<url::Url>,
}
impl Api {
    pub fn new(http: Client, stored: auth::Stored, token: auth::Token) -> Self {
        let mut api = Self {
            http,
            stored,
            access: String::new(),
            expires: Instant::now(),
            #[cfg(test)]
            test_origin: None,
        };
        api.set_token(token);
        api
    }
    fn set_token(&mut self, token: auth::Token) {
        self.access = token.access_token;
        self.expires =
            Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(60).min(86400));
        if let Some(refresh) = token.refresh_token {
            self.stored.refresh_token = refresh;
        }
    }
    pub fn access(&self) -> String {
        self.access.clone()
    }
    pub fn request(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
        job: &auth::Job,
    ) -> Result<Value, String> {
        job.check()?;
        let url = api_url(path)?;
        #[cfg(test)]
        let url = if let Some(origin) = &self.test_origin {
            let mut target = origin.clone();
            target.set_path(url.path());
            target.set_query(url.query());
            target
        } else {
            url
        };
        for attempt in 0..2 {
            if Instant::now() >= self.expires || attempt == 1 {
                let token = auth::refresh(&self.http, &self.stored)?;
                self.set_token(token);
                job.check()?;
                auth::save(&self.stored)?;
            }
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(&self.access);
            if let Some(body) = body.as_ref() {
                request = request.json(body);
            }
            let response = request
                .send()
                .map_err(|_| "Could not reach Spotify. Check your connection and try again.")?;
            job.check()?;
            let status = response.status().as_u16();
            if status == 401 && attempt == 0 {
                continue;
            }
            if status == 429 {
                let seconds = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(30);
                return Err(format!(
                    "Spotify is limiting requests. Try again in {seconds} seconds."
                ));
            }
            if !(200..300).contains(&status) {
                return Err(match status {
                    401 => "Spotify sign-in expired. Reconnect your account.".into(),
                    403 => "Spotify denied this request. Check account permissions; a developer Client ID may be needed under Advanced connection options.".into(),
                    404 => "This Spotify playlist or track is no longer available to your account.".into(),
                    _ => format!("Spotify returned HTTP {status}. Try again later."),
                });
            }
            if status == 204 {
                return Ok(Value::Null);
            }
            let bytes = response
                .bytes()
                .map_err(|_| "Could not read Spotify's response.")?;
            if bytes.is_empty() {
                return Ok(Value::Null);
            }
            if bytes.len() > 16 * 1024 * 1024 {
                return Err("Spotify response is too large.".into());
            }
            return serde_json::from_slice(&bytes)
                .map_err(|_| "Invalid response from Spotify.".into());
        }
        Err("Sign in to Spotify again.".into())
    }
    pub fn profile(&mut self, job: &auth::Job) -> Result<SpotifyAccount, String> {
        let me = self.request(Method::GET, "me", None, job)?;
        if me["product"].as_str().is_some_and(|p| p != "premium") {
            return Err("Spotify playback requires a Premium account with librespot.".into());
        }
        let id = me["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Spotify did not return your account ID.")?;
        Ok(SpotifyAccount {
            id: id.into(),
            name: me["display_name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(id)
                .into(),
        })
    }
    fn pages(
        &mut self,
        path: &str,
        root: Option<&str>,
        maximum: usize,
        job: &auth::Job,
    ) -> Result<Vec<Value>, String> {
        self.pages_cancellable(path, root, maximum, job, &|| false)
    }

    fn pages_cancellable(
        &mut self,
        path: &str,
        root: Option<&str>,
        maximum: usize,
        job: &auth::Job,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<Value>, String> {
        let mut next = Some(path.to_string());
        let mut visited = HashSet::new();
        let mut items = Vec::new();
        while let Some(path) = next {
            if cancelled() {
                return Err("Cancelled".into());
            }
            if !visited.insert(path.clone()) {
                return Err("Spotify returned a repeating pagination link.".into());
            }
            let response = self.request(Method::GET, &path, None, job)?;
            let page = root.map_or(&response, |root| &response[root]);
            let batch = page["items"]
                .as_array()
                .ok_or("Spotify did not return a complete list. No playlists were replaced.")?;
            items.extend(batch.iter().cloned());
            next = page["next"].as_str().map(str::to_string);
            if items.len() > maximum || (items.len() == maximum && next.is_some()) {
                return Err(format!(
                    "This Spotify collection exceeds the {maximum}-item sync limit. No local copy was replaced."
                ));
            }
        }
        Ok(items)
    }
    pub fn search(&mut self, query: &str, job: &auth::Job) -> Result<Vec<Track>, String> {
        let mut url = api_url("search")?;
        url.query_pairs_mut().extend_pairs([
            ("q", query),
            ("type", "track"),
            ("limit", "10"),
            ("market", "from_token"),
        ]);
        let value = self.request(Method::GET, url.as_str(), None, job)?;
        let items = value["tracks"]["items"]
            .as_array()
            .ok_or("Spotify did not return search results.")?;
        Ok(items.iter().filter_map(parse_track).collect())
    }
    pub fn album(
        &mut self,
        query: &crate::albums::Query,
        job: &auth::Job,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<Track>, String> {
        if cancelled() {
            return Err("Cancelled".into());
        }
        let mut url = api_url("search")?;
        let search = format!(
            "album:\"{}\" artist:\"{}\"",
            query.title.replace('"', " "),
            query.artist.replace('"', " ")
        );
        url.query_pairs_mut().extend_pairs([
            ("q", search.as_str()),
            ("type", "album"),
            ("limit", "10"),
            ("market", "from_token"),
        ]);
        let response = self.request(Method::GET, url.as_str(), None, job)?;
        let items = response["albums"]["items"]
            .as_array()
            .ok_or("Spotify did not return search results.")?;
        let mut albums: Vec<_> = items
            .iter()
            .filter(|a| {
                a["id"].as_str().is_some_and(valid_id)
                    && query.matches(
                        a["name"].as_str().unwrap_or(""),
                        a["artists"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|a| a["name"].as_str()),
                    )
            })
            .collect();
        albums.sort_by_key(|a| {
            !query.date.get(..4).is_some_and(|year| {
                a["release_date"]
                    .as_str()
                    .is_some_and(|date| date.starts_with(year))
            })
        });
        for album in albums.into_iter().take(3) {
            let id = album["id"].as_str().unwrap();
            let mut items = self.pages_cancellable(
                &format!("albums/{id}/tracks?limit=50&market=from_token"),
                None,
                1000,
                job,
                &cancelled,
            )?;
            items.sort_by_key(|v| {
                (
                    v["disc_number"].as_u64().unwrap_or(1),
                    v["track_number"].as_u64().unwrap_or(0),
                )
            });
            let album_artist = album["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let tracks: Vec<_> = items
                .into_iter()
                .filter_map(|mut item| {
                    item["album"] = album.clone();
                    let mut track = parse_track(&item)?;
                    track.album_artist.clone_from(&album_artist);
                    Some(track)
                })
                .collect();
            if !tracks.is_empty() {
                return Ok(tracks);
            }
        }
        Ok(vec![])
    }

    pub fn playlist(&mut self, id: &str, job: &auth::Job) -> Result<RemotePlaylist, String> {
        if !valid_id(id) {
            return Err("Invalid Spotify playlist ID.".into());
        }
        let details = self.request(Method::GET, &format!("playlists/{id}"), None, job)?;
        let name = details["name"]
            .as_str()
            .ok_or("Spotify did not return the playlist name.")?
            .to_string();
        let revision = details["snapshot_id"]
            .as_str()
            .ok_or("Spotify did not return the playlist revision.")?
            .to_string();
        let items = self.pages(
            &format!("playlists/{id}/items?limit=50&market=from_token"),
            None,
            5000,
            job,
        )?;
        let tracks: Vec<_> = items
            .iter()
            .filter_map(|v| {
                parse_track(
                    v.get("item")
                        .filter(|i| !i.is_null())
                        .or_else(|| v.get("track"))
                        .unwrap_or(&Value::Null),
                )
            })
            .collect();
        let unavailable = items.len() - tracks.len();
        // Check again so a playlist changed while paginating cannot become a destructive baseline.
        let after = self.request(
            Method::GET,
            &format!("playlists/{id}?fields=snapshot_id,name"),
            None,
            job,
        )?;
        if after["snapshot_id"].as_str() != Some(&revision) || after["name"].as_str() != Some(&name)
        {
            return Err(format!("{name} changed while being read. Sync again."));
        }
        let uris: Vec<_> = tracks.iter().map(|t| t.uri.clone()).collect();
        Ok(RemotePlaylist {
            id: id.into(),
            snapshot: snapshot(&name, &uris),
            name,
            tracks,
            editable: details["owner"]["id"].as_str() == Some(&self.stored.account_id)
                && unavailable == 0,
            unavailable,
            local_id: None,
            revision,
        })
    }
    pub fn synchronize(
        &mut self,
        before: &[LocalPlaylist],
        ignored: &[String],
        job: &auth::Job,
    ) -> Result<SyncResult, String> {
        let playlists = self.pages("me/playlists?limit=50", None, 500, job)?;
        let mut result = SyncResult::default();
        for playlist in playlists {
            job.check()?;
            let Some(id) = playlist["id"].as_str().filter(|id| valid_id(id)) else {
                continue;
            };
            if ignored.iter().any(|i| i == id) {
                continue;
            }
            let name = playlist["name"].as_str().unwrap_or("Playlist");
            let local = before.iter().find(|p| {
                p.link
                    .as_ref()
                    .is_some_and(|l| l.id == id && l.account_id == self.stored.account_id)
            });
            match self
                .playlist(id, job)
                .and_then(|remote| self.reconcile(local, remote, job))
            {
                Ok((remote, uploaded)) => {
                    result.playlists.push(remote);
                    result.uploaded += usize::from(uploaded);
                }
                Err(e) => result.failed.push(format!("{name}: {e}")),
            }
        }
        Ok(result)
    }
    fn reconcile(
        &mut self,
        local: Option<&LocalPlaylist>,
        mut remote: RemotePlaylist,
        job: &auth::Job,
    ) -> Result<(RemotePlaylist, bool), String> {
        let Some(local) = local else {
            return Ok((remote, false));
        };
        remote.local_id = Some(local.local_id.clone());
        match decision(local, &remote) {
            Decision::Pull => Ok((remote, false)),
            Decision::Conflict => Err("Both copies changed, or the online copy is read-only. Local edits were kept. Use Replace local copy from Spotify to discard them.".into()),
            Decision::Push => {
                self.write_playlist(local, &remote, job)?;
                let mut updated = self.playlist(&remote.id, job)?;
                updated.local_id = Some(local.local_id.clone());
                Ok((updated, true))
            }
        }
    }
    fn write_playlist(
        &mut self,
        local: &LocalPlaylist,
        remote: &RemotePlaylist,
        job: &auth::Job,
    ) -> Result<(), String> {
        if !local.valid || !remote.editable {
            return Err(
                "Only your own playlists containing Spotify tracks can be uploaded.".into(),
            );
        }
        let current = self.request(
            Method::GET,
            &format!("playlists/{}?fields=snapshot_id,name", remote.id),
            None,
            job,
        )?;
        if current["snapshot_id"].as_str() != Some(&remote.revision)
            || current["name"].as_str() != Some(&remote.name)
        {
            return Err("This playlist changed online. Sync again before uploading.".into());
        }
        let path = format!("playlists/{}/items", remote.id);
        // Spotify replaces at most 100 items per request. Never retry a failed mutation blindly.
        self.request(
            Method::PUT,
            &path,
            Some(json!({"uris": local.uris.iter().take(100).collect::<Vec<_>>() })),
            job,
        )?;
        for chunk in local.uris.get(100..).unwrap_or(&[]).chunks(100) {
            self.request(Method::POST, &path, Some(json!({"uris": chunk})), job)
                .map_err(|e| format!("Upload stopped partway through; the local playlist is preserved. Check the online playlist before syncing again. {e}"))?;
        }
        if local.name != remote.name {
            self.request(
                Method::PUT,
                &format!("playlists/{}", remote.id),
                Some(json!({"name": local.name})),
                job,
            )?;
        }
        Ok(())
    }
    pub fn publish(
        &mut self,
        local: &LocalPlaylist,
        job: &auth::Job,
    ) -> Result<RemotePlaylist, String> {
        if !local.valid || local.uris.is_empty() {
            return Err("Only playlists containing Spotify tracks can be published.".into());
        }
        if let Some(link) = &local.link {
            if link.account_id != self.stored.account_id {
                return Err("This playlist belongs to another Spotify account.".into());
            }
            let remote = self.playlist(&link.id, job)?;
            return self.reconcile(Some(local), remote, job).map(|(r, _)| r);
        }
        let created = self.request(
            Method::POST,
            "me/playlists",
            Some(json!({"name": local.name, "public": false})),
            job,
        )?;
        let id = created["id"]
            .as_str()
            .ok_or("Spotify did not return the created playlist ID.")?;
        let remote = self.playlist(id, job)?;
        self.write_playlist(local, &remote, job).map_err(|e| format!("Playlist was created at https://open.spotify.com/playlist/{id}, but upload did not finish. Sync your playlists before retrying. {e}"))?;
        let mut remote = self.playlist(id, job)?;
        remote.local_id = Some(local.local_id.clone());
        Ok(remote)
    }
}

fn api_url(path: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(if path.starts_with("https://") {
        path
    } else {
        "https://api.spotify.com/v1/"
    })
    .and_then(|base| {
        if path.starts_with("https://") {
            Ok(base)
        } else {
            base.join(path)
        }
    })
    .map_err(|_| "Invalid Spotify API URL.")?;
    if url.scheme() != "https"
        || url.host_str() != Some("api.spotify.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/v1/")
        || url.fragment().is_some()
    {
        return Err("Spotify returned an unsafe pagination URL.".into());
    }
    Ok(url)
}
pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}
pub fn parse_track(item: &Value) -> Option<Track> {
    if item["type"] != "track" || item["is_local"] == true || item["is_playable"] == false {
        return None;
    }
    let id = item["id"].as_str().filter(|id| valid_id(id))?;
    let mut track = Track::new(
        &format!("spotify:track:{id}"),
        item["name"].as_str()?,
        Kind::Spotify,
    );
    track.artist = item["artists"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|a| a["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    track.album = item["album"]["name"].as_str().unwrap_or("").into();
    track.artwork = item["album"]["images"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|i| i["url"].as_str())
        .filter(|u| crate::services::http_url(u).is_ok())
        .map(str::to_string);
    track.duration = item["duration_ms"].as_u64().unwrap_or(0) as f64 / 1000.0;
    track.number = item["track_number"].as_u64().unwrap_or(0) as u32;
    track.source = "Spotify".into();
    Some(track)
}

#[derive(Clone, Debug)]
pub struct LocalPlaylist {
    pub local_id: String,
    pub name: String,
    pub uris: Vec<String>,
    pub snapshot: String,
    pub valid: bool,
    pub link: Option<SpotifyPlaylistLink>,
}
#[derive(Debug)]
pub struct RemotePlaylist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Track>,
    pub editable: bool,
    pub snapshot: String,
    pub unavailable: usize,
    pub local_id: Option<String>,
    pub revision: String,
}
#[derive(Default, Debug)]
pub struct SyncResult {
    pub playlists: Vec<RemotePlaylist>,
    pub failed: Vec<String>,
    pub uploaded: usize,
}
#[derive(Debug, PartialEq)]
enum Decision {
    Pull,
    Push,
    Conflict,
}
fn decision(local: &LocalPlaylist, remote: &RemotePlaylist) -> Decision {
    let Some(link) = &local.link else {
        return Decision::Pull;
    };
    if local.snapshot == link.local_snapshot || local.snapshot == remote.snapshot {
        Decision::Pull
    } else if local.valid && remote.editable && remote.snapshot == link.remote_snapshot {
        Decision::Push
    } else {
        Decision::Conflict
    }
}
pub fn local_playlist(state: &State, id: &str) -> Option<LocalPlaylist> {
    let p = state.playlist(id)?;
    let uris: Vec<_> = p
        .tracks
        .iter()
        .map(|id| state.track(id).map(|t| t.uri.clone()).unwrap_or_default())
        .collect();
    Some(LocalPlaylist {
        local_id: id.into(),
        name: p.name.clone(),
        snapshot: snapshot(&p.name, &uris),
        valid: uris.len() <= 5000 && uris.iter().all(|u| track_id(u).is_some()),
        uris,
        link: p.spotify.clone(),
    })
}
pub fn linked_playlists(state: &State) -> Vec<LocalPlaylist> {
    state
        .playlists
        .iter()
        .filter(|p| {
            p.spotify.as_ref().is_some_and(|l| {
                state
                    .spotify_account
                    .as_ref()
                    .is_some_and(|a| a.id == l.account_id)
            })
        })
        .filter_map(|p| local_playlist(state, &p.id))
        .collect()
}

/// Do not overwrite a playlist edited/deleted while the network job was running.
pub fn apply_sync(
    state: &mut State,
    account: &str,
    before: &[LocalPlaylist],
    result: SyncResult,
) -> (usize, usize, usize) {
    if !state
        .spotify_account
        .as_ref()
        .is_some_and(|a| a.id == account)
    {
        return (0, 0, 0);
    }
    let mut applied = 0;
    let mut conflicts = 0;
    let mut unavailable = 0;
    for remote in result.playlists {
        if state.spotify_ignored_playlists.contains(&remote.id) {
            continue;
        }
        let existing = remote.local_id.clone().or_else(|| {
            state
                .playlists
                .iter()
                .find(|p| {
                    p.spotify
                        .as_ref()
                        .is_some_and(|l| l.id == remote.id && l.account_id == account)
                })
                .map(|p| p.id.clone())
        });
        let captured = existing
            .as_ref()
            .and_then(|id| before.iter().find(|p| &p.local_id == id));
        let current = existing.as_ref().and_then(|id| local_playlist(state, id));
        if existing.is_some() && current.is_none() {
            continue;
        }
        if current
            .as_ref()
            .zip(captured)
            .is_some_and(|(now, old)| now.snapshot != old.snapshot)
        {
            conflicts += 1;
            continue;
        }
        let ids = remote.tracks.iter().map(|t| t.id.clone()).collect();
        state.merge(remote.tracks);
        let id = if let Some(id) = existing {
            let unchanged = current.as_ref().is_some_and(|p| {
                p.link.as_ref().is_some_and(|l| {
                    l.remote_snapshot == remote.snapshot && l.local_snapshot == p.snapshot
                })
            });
            if !unchanged {
                let name = state
                    .playlist_name(&remote.name, Some(&id))
                    .unwrap_or_else(|_| state.unique_playlist_name(&remote.name));
                let p = state.playlists.iter_mut().find(|p| p.id == id).unwrap();
                p.name = name;
                p.tracks = ids;
            }
            id
        } else {
            let name = state.unique_playlist_name(&remote.name);
            let Ok(id) = state.create_playlist(&name, ids) else {
                conflicts += 1;
                continue;
            };
            id
        };
        let local_snapshot = local_playlist(state, &id).unwrap().snapshot;
        state
            .playlists
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .spotify = Some(SpotifyPlaylistLink {
            id: remote.id,
            account_id: account.into(),
            editable: remote.editable,
            remote_snapshot: remote.snapshot,
            local_snapshot,
        });
        applied += 1;
        unavailable += remote.unavailable;
    }
    (applied, conflicts, unavailable)
}
pub fn clear_account(state: &mut State) {
    state.spotify_account = None;
    state.spotify_ignored_playlists.clear();
    state.playlists.retain(|p| p.spotify.is_none());
    state.tracks.retain(|t| t.kind != Kind::Spotify);
    state.sanitize();
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "4uLU6hMCjMI75M1A2tKUQC";
    fn track() -> Track {
        parse_track(&json!({"id": ID, "type": "track", "name": "Fixture song", "artists": [{"name": "Artist"}], "album": {"name": "Album", "images": [{"url": "https://i.scdn.co/image/fixture"}]}, "duration_ms": 153250, "track_number": 2})).unwrap()
    }
    fn remote(name: &str) -> RemotePlaylist {
        let track = track();
        RemotePlaylist {
            id: ID.into(),
            name: name.into(),
            snapshot: snapshot(name, std::slice::from_ref(&track.uri)),
            tracks: vec![track],
            editable: true,
            unavailable: 0,
            local_id: None,
            revision: "revision1".into(),
        }
    }
    fn state() -> State {
        State {
            spotify_account: Some(SpotifyAccount {
                id: "owner".into(),
                name: "Owner".into(),
            }),
            ..State::default()
        }
    }
    fn apply(
        state: &mut State,
        before: &[LocalPlaylist],
        remote: RemotePlaylist,
    ) -> (usize, usize, usize) {
        apply_sync(
            state,
            "owner",
            before,
            SyncResult {
                playlists: vec![remote],
                ..SyncResult::default()
            },
        )
    }
    struct Fixture {
        requests: std::thread::JoinHandle<Vec<(String, Value)>>,
    }
    fn fixture(responses: Vec<(u16, Value)>) -> (Api, auth::Job, Fixture) {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = url::Url::parse(&format!("http://{}/", server.local_addr().unwrap())).unwrap();
        server.set_nonblocking(true).unwrap();
        let requests = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, response) in responses {
                let until = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    if let Ok((socket, _)) = server.accept() {
                        break socket;
                    }
                    assert!(
                        Instant::now() < until,
                        "Expected another Spotify fixture request"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let (headers, body) = loop {
                    let n = socket.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                        let length = headers
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break (headers, bytes[end + 4..end + 4 + length].to_vec());
                        }
                    }
                };
                assert!(
                    headers
                        .to_lowercase()
                        .contains("authorization: bearer fixture-access")
                );
                requests.push((
                    headers.lines().next().unwrap().to_string(),
                    if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&body).unwrap()
                    },
                ));
                let body = serde_json::to_string(&response).unwrap();
                write!(socket, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nRetry-After: 17\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let mut api = Api::new(
            Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            auth::Stored {
                refresh_token: "fixture-refresh".into(),
                client_id: "fixture-client".into(),
                account_id: "owner".into(),
            },
            auth::Token {
                access_token: "fixture-access".into(),
                refresh_token: None,
                expires_in: 3600,
            },
        );
        api.test_origin = Some(origin);
        (
            api,
            auth::Job {
                epoch: 1,
                current: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            },
            Fixture { requests },
        )
    }
    #[test]
    fn catalog_requests_follow_all_pages_and_report_errors() {
        let (mut api, job, server) = fixture(vec![
            (
                200,
                json!({"items": [{"id": "first"}], "next": "https://api.spotify.com/v1/me/playlists?offset=50"}),
            ),
            (200, json!({"items": [{"id": "second"}], "next": null})),
            (403, json!({"error": "not permitted"})),
            (429, json!({"error": "limited"})),
        ]);
        let items = api.pages("me/playlists?limit=50", None, 500, &job).unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            api.pages("me/playlists", None, 500, &job)
                .unwrap_err()
                .contains("denied")
        );
        assert!(
            api.search("a & b", &job)
                .unwrap_err()
                .contains("17 seconds")
        );
        let requests = server.requests.join().unwrap();
        assert!(requests[1].0.contains("/v1/me/playlists?offset=50"));
        assert!(requests[3].0.contains("q=a+%26+b&type=track&limit=10"));
    }
    #[test]
    fn albums_match_artist_and_title_and_load_every_disc_in_order() {
        let second = "5uLU6hMCjMI75M1A2tKUQC";
        let wrong = "6uLU6hMCjMI75M1A2tKUQC";
        let (mut api, job, server) = fixture(vec![
            (
                200,
                json!({"albums":{"items":[
                    {"id":wrong,"name":"Small Hours","artists":[{"name":"Someone else"}]},
                    {"id":ID,"name":"Small Hours","artists":[{"name":"Ensemble"}],"release_date":"2020-01-01","images":[{"url":"https://example.org/cover.png"}]}
                ]}}),
            ),
            (
                200,
                json!({"items":[{"id":ID,"type":"track","name":"Disc two","artists":[{"name":"Guest"}],"disc_number":2,"track_number":1}],"next":format!("https://api.spotify.com/v1/albums/{ID}/tracks?offset=50")}),
            ),
            (
                200,
                json!({"items":[{"id":second,"type":"track","name":"Disc one","artists":[{"name":"Ensemble"}],"disc_number":1,"track_number":1},{"id":wrong,"type":"track","name":"Unavailable","is_playable":false}],"next":null}),
            ),
            (
                200,
                json!({"albums":{"items":[{"id":ID,"name":"Small Hours Live","artists":[{"name":"Ensemble"}]}]}}),
            ),
        ]);
        let query = crate::albums::Query {
            id: "group".into(),
            artist_id: String::new(),
            artist: "Ensemble".into(),
            title: "Small Hours".into(),
            date: "2020".into(),
        };
        let tracks = api.album(&query, &job, || false).unwrap();
        assert_eq!(
            tracks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            vec!["Disc one", "Disc two"]
        );
        assert!(tracks.iter().all(|t| t.kind == Kind::Spotify
            && t.album == "Small Hours"
            && t.album_artist == "Ensemble"
            && t.artwork.is_some()));
        assert!(api.album(&query, &job, || false).unwrap().is_empty());
        let requests = server.requests.join().unwrap();
        assert!(requests[0].0.contains("type=album"));
        assert!(requests[1].0.contains(&format!("/albums/{ID}/tracks")));
        assert!(requests[2].0.contains("offset=50"));
        assert_eq!(
            requests.len(),
            4,
            "Non-matching albums must never be fetched"
        );
    }
    #[test]
    fn uploads_batch_without_losing_order_and_refuse_changed_revisions() {
        let (mut api, job, server) = fixture(vec![
            (200, json!({"snapshot_id": "revision1", "name": "Mix"})),
            (200, json!({"snapshot_id": "revision2"})),
            (200, json!({"snapshot_id": "revision3"})),
            (200, Value::Null),
            (200, json!({"snapshot_id": "changed", "name": "Mix"})),
        ]);
        let uris: Vec<_> = (0..150).map(|_| format!("spotify:track:{ID}")).collect();
        let local = LocalPlaylist {
            local_id: "local".into(),
            name: "Renamed".into(),
            snapshot: snapshot("Renamed", &uris),
            uris: uris.clone(),
            valid: true,
            link: None,
        };
        api.write_playlist(&local, &remote("Mix"), &job).unwrap();
        assert!(
            api.write_playlist(&local, &remote("Mix"), &job)
                .unwrap_err()
                .contains("changed online")
        );
        let requests = server.requests.join().unwrap();
        assert!(
            requests[1]
                .0
                .starts_with(&format!("PUT /v1/playlists/{ID}/items "))
        );
        assert!(
            requests[2]
                .0
                .starts_with(&format!("POST /v1/playlists/{ID}/items "))
        );
        assert_eq!(requests[1].1["uris"].as_array().unwrap().len(), 100);
        assert_eq!(requests[2].1["uris"].as_array().unwrap().len(), 50);
        assert_eq!(requests[3].1["name"], "Renamed");
        assert_eq!(
            requests.len(),
            5,
            "No mutation may follow the changed revision"
        );
    }
    #[test]
    fn partial_playlist_uploads_keep_the_local_copy_and_report_partial_failure() {
        let (mut api, job, server) = fixture(vec![
            (200, json!({"snapshot_id": "revision1", "name": "Mix"})),
            (200, json!({"snapshot_id": "revision2"})),
            (503, json!({"error": "offline"})),
        ]);
        let uris = vec![format!("spotify:track:{ID}"); 101];
        let local = LocalPlaylist {
            local_id: "local".into(),
            name: "Mix".into(),
            snapshot: snapshot("Mix", &uris),
            uris: uris.clone(),
            valid: true,
            link: None,
        };
        assert!(
            api.write_playlist(&local, &remote("Mix"), &job)
                .unwrap_err()
                .contains("partway")
        );
        assert_eq!(local.uris, uris);
        assert_eq!(server.requests.join().unwrap().len(), 3);
    }
    #[test]
    fn playlist_reads_understand_both_item_shapes_and_reject_mid_read_changes() {
        let song = json!({"type": "track", "id": ID, "name": "Song"});
        let details = json!({"name": "Mix", "snapshot_id": "revision1", "owner": {"id": "owner"}});
        let items =
            json!({"items": [{"item": song}, {"track": song}, {"item": null}], "next": null});
        let (mut api, job, server) = fixture(vec![
            (200, details.clone()),
            (200, items.clone()),
            (200, details.clone()),
            (200, details),
            (200, items),
            (200, json!({"name": "Mix", "snapshot_id": "revision2"})),
        ]);
        let playlist = api.playlist(ID, &job).unwrap();
        assert_eq!(playlist.tracks.len(), 2);
        assert_eq!(playlist.unavailable, 1);
        assert!(!playlist.editable);
        assert!(
            api.playlist(ID, &job)
                .unwrap_err()
                .contains("changed while being read")
        );
        assert_eq!(server.requests.join().unwrap().len(), 6);
    }
    #[test]
    fn spotify_metadata_preserves_identity_and_ignores_unplayable_items() {
        let t = track();
        assert_eq!(t.uri, format!("spotify:track:{ID}"));
        assert_eq!(t.duration, 153.25);
        assert_eq!(t.kind, Kind::Spotify);
        for item in [
            json!(null),
            json!({"type": "episode", "id": ID, "name": "Episode"}),
            json!({"type": "track", "id": "bad", "name": "Track"}),
            json!({"type": "track", "id": ID, "name": "Track", "is_playable": false}),
            json!({"type": "track", "id": ID, "name": "Track", "is_local": true}),
        ] {
            assert!(parse_track(&item).is_none());
        }
    }
    #[test]
    fn pagination_cannot_send_credentials_to_another_origin() {
        assert!(api_url("https://api.spotify.com/v1/me/playlists?offset=50").is_ok());
        for url in [
            "http://api.spotify.com/v1/me",
            "https://api.spotify.com.evil.test/v1/me",
            "//evil.test/v1/me",
            "https://user:password@api.spotify.com/v1/me",
            "https://api.spotify.com:8080/v1/me",
            "https://api.spotify.com/not-v1",
            "https://api.spotify.com/v1/me#fragment",
            "../outside",
        ] {
            assert!(api_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn sync_decisions_preserve_conflicts_and_read_only_playlists() {
        let mut state = state();
        apply(&mut state, &[], remote("Mix"));
        let id = state.playlists[0].id.clone();
        let original = local_playlist(&state, &id).unwrap();
        assert_eq!(decision(&original, &remote("Mix")), Decision::Pull);
        state.rename_playlist(&id, "Local rename").unwrap();
        let mut edited = local_playlist(&state, &id).unwrap();
        assert_eq!(decision(&edited, &remote("Mix")), Decision::Push);
        assert_eq!(
            decision(&edited, &remote("Online rename")),
            Decision::Conflict
        );
        let mut readonly = remote("Mix");
        readonly.editable = false;
        assert_eq!(decision(&edited, &readonly), Decision::Conflict);
        edited.valid = false;
        assert_eq!(decision(&edited, &remote("Mix")), Decision::Conflict);
    }
    #[test]
    fn sync_preserves_order_duplicates_collisions_and_in_flight_edits() {
        let mut state = state();
        state.create_playlist("Mix", vec![]).unwrap();
        let mut r = remote("Mix");
        r.tracks.push(track());
        r.snapshot = snapshot(
            "Mix",
            &r.tracks.iter().map(|t| t.uri.clone()).collect::<Vec<_>>(),
        );
        assert_eq!(apply(&mut state, &[], r).0, 1);
        let id = state.playlists[1].id.clone();
        assert_eq!(state.playlist(&id).unwrap().name, "Mix (2)");
        assert_eq!(state.playlist(&id).unwrap().tracks.len(), 2);
        let before = linked_playlists(&state);
        state.rename_playlist(&id, "Edited while syncing").unwrap();
        assert_eq!(apply(&mut state, &before, remote("Online edit")).1, 1);
        assert_eq!(state.playlist(&id).unwrap().name, "Edited while syncing");
        state.playlists.retain(|p| p.id != id);
        let mut response = remote("Mix");
        response.local_id = Some(id);
        assert_eq!(apply(&mut state, &before, response).0, 0);
        state.spotify_ignored_playlists.push(ID.into());
        assert_eq!(apply(&mut state, &[], remote("Mix")).0, 0);
    }
    #[test]
    fn account_switches_and_disconnect_leave_other_sources_intact() {
        let mut state = state();
        let local = Track::new("file:///song.ogg", "Local", Kind::Music);
        let local_id = local.id.clone();
        state.merge(vec![local]);
        apply(&mut state, &[], remote("Mix"));
        state.queue = state.tracks.iter().map(|t| t.id.clone()).collect();
        state.current = Some(track().id);
        assert_eq!(
            apply_sync(
                &mut state,
                "different-account",
                &[],
                SyncResult {
                    playlists: vec![remote("Wrong account")],
                    ..SyncResult::default()
                }
            )
            .0,
            0
        );
        clear_account(&mut state);
        assert_eq!(state.queue, [local_id]);
        assert!(state.current.is_none());
        assert!(state.playlists.is_empty());
        assert!(state.spotify_account.is_none());
    }
    #[test]
    fn mixed_playlists_cannot_be_published_and_legacy_state_still_loads() {
        let mut state: State =
            serde_json::from_str(r#"{"playlists":[{"id":"old","name":"Old","tracks":[]}]}"#)
                .unwrap();
        assert!(state.spotify_account.is_none());
        assert!(state.playlists[0].spotify.is_none());
        let local = Track::new("file:///test.ogg", "Local", Kind::Music);
        let ids = vec![track().id, local.id.clone()];
        state.merge(vec![track(), local]);
        let id = state.create_playlist("Mixed", ids).unwrap();
        assert!(!local_playlist(&state, &id).unwrap().valid);
    }
}
