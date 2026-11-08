use super::*;
use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
struct CachedJson {
    checked: u64,
    value: Option<Value>,
}

impl Client {
    // Entity caches are shared between songs, so a whole album does not repeatedly
    // fetch the same biography, summary and first discography page.
    fn information_json(
        &mut self,
        url: url::Url,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Value>, Failure> {
        let path = self
            .config
            .cache
            .join(format!("entity-{}.json", stable_id(url.as_str())));
        if !self.refresh_information
            && let Ok(bytes) = fs::read(&path)
            && let Ok(cached) = serde_json::from_slice::<CachedJson>(&bytes)
        {
            // Short-lived search caches must not extend the seven-day
            // negative-result cache through otherwise valid empty JSON.
            let search = url.query_pairs().any(|(key, _)| key == "query");
            if now().saturating_sub(cached.checked)
                < if cached.value.is_some() && !search {
                    30 * 86400
                } else {
                    7 * 86400
                }
            {
                return Ok(cached.value);
            }
        }
        let value = self.json(url.clone(), cancelled)?;
        let musicbrainz = endpoint(&self.config.musicbrainz, "")
            .is_ok_and(|base| url.origin() == base.origin() && url.path().starts_with(base.path()));
        let invalid = value.as_ref().is_some_and(|v| {
            if v.get("error").is_some() {
                return true;
            }
            if musicbrainz {
                match url
                    .path()
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                {
                    "artist" => !v["artists"].is_array(),
                    "release-group" => !v["release-groups"].is_array(),
                    "recording" => !v["recordings"].is_array(),
                    "release" => !v["releases"].is_array(),
                    _ => mbid(v).is_none(),
                }
            } else {
                false
            }
        });
        if invalid {
            return Err(Failure::new(
                "The provider returned an invalid response.",
                30,
            ));
        }
        if cancelled() {
            return Err(Failure::new("Lookup cancelled.", 0));
        }
        fs::create_dir_all(&self.config.cache)
            .map_err(|_| Failure::new("Could not create the metadata cache.", 5))?;
        let cached = CachedJson {
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

    fn mb(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Value>, Failure> {
        let mut url = endpoint(&self.config.musicbrainz, path)?;
        url.query_pairs_mut()
            .extend_pairs(params.iter().copied())
            .append_pair("fmt", "json");
        self.information_json(url, cancelled)
    }

    pub(super) fn information(
        &mut self,
        query: &Query,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Information>, Failure> {
        let mut info = Information {
            key: query.key.clone(),
            checked: now(),
            ..Default::default()
        };
        let release = if let Some(id) = &query.release_id {
            self.mb(
                &format!("release/{id}"),
                &[("inc", "artist-credits+release-groups+recordings")],
                cancelled,
            )?
        } else {
            None
        };
        let mut recording = if let Some(id) = &query.recording_id {
            self.mb(
                &format!("recording/{id}"),
                &[("inc", "artist-credits+releases+release-groups")],
                cancelled,
            )?
        } else if query.artist.is_empty() || query.album.is_empty() {
            if let Some(recording) = release.as_ref().and_then(|r| release_recording(query, r)) {
                Some(recording)
            } else if !query.title_from_filename
                && !query.title.is_empty()
                && query.duration > 0.0
                && (!query.artist.is_empty() || !query.album.is_empty())
            {
                let mut search = format!("recording:\"{}\"", lucene(&query.title));
                if !query.artist.is_empty() {
                    search.push_str(&format!(" AND artist:\"{}\"", lucene(&query.artist)));
                }
                if !query.album.is_empty() {
                    search.push_str(&format!(" AND release:\"{}\"", lucene(&query.album)));
                }
                let found = self.mb(
                    "recording/",
                    &[("query", &search), ("limit", "100")],
                    cancelled,
                )?;
                found.as_ref().and_then(|v| matching_recording(query, v))
            } else {
                None
            }
        } else {
            None
        };

        // Recording search/lookup can embed a truncated release list. Never infer
        // an album until a complete browse page confirms a unique release group.
        if query.album.is_empty()
            && release.is_none()
            && let Some(recording) = &mut recording
            && let Some(id) = mbid(recording)
        {
            let releases = self.mb(
                "release/",
                &[
                    ("recording", &id),
                    ("inc", "release-groups"),
                    ("limit", "100"),
                ],
                cancelled,
            )?;
            let complete = releases.as_ref().filter(|v| {
                v["releases"]
                    .as_array()
                    .zip(v["release-count"].as_u64())
                    .is_some_and(|(items, count)| items.len() as u64 >= count)
            });
            recording["releases"] = complete
                .map(|v| v["releases"].clone())
                .unwrap_or_else(|| Value::Array(vec![]));
        }
        if query.artist.is_empty() {
            info.filled_artist = recording.as_ref().map(credit).filter(|s| !s.is_empty());
        }
        if query.album.is_empty() {
            info.filled_album = release.as_ref().and_then(|r| text(r, "title"));
        }
        let mut group_id = release.as_ref().and_then(|r| mbid(&r["release-group"]));
        if group_id.is_none()
            && let Some(recording) = &recording
            && let Some(release) = unique_recording_release(query, recording)
        {
            group_id = mbid(&release["release-group"]);
            if query.album.is_empty() {
                info.filled_album = text(&release, "title");
            }
        }
        let artist_name = if !query.artist.is_empty() {
            &query.artist
        } else {
            info.filled_artist.as_ref().unwrap_or(&query.album_artist)
        };
        let album_name = info.filled_album.as_ref().unwrap_or(&query.album);
        let album_artist = if query.album_artist.is_empty() {
            artist_name
        } else {
            &query.album_artist
        };
        if group_id.is_none() && !album_name.is_empty() && !album_artist.is_empty() {
            let search = format!(
                "releasegroup:\"{}\" AND artist:\"{}\"",
                lucene(album_name),
                lucene(album_artist)
            );
            if let Some(found) = self.mb(
                "release-group/",
                &[("query", &search), ("limit", "100")],
                cancelled,
            )? {
                group_id = unique_search(&found, "release-groups", |v| {
                    same(v["title"].as_str().unwrap_or(""), album_name)
                        && credit_matches(v, album_artist)
                })
                .and_then(|v| mbid(&v));
            }
        }
        let group = if let Some(id) = &group_id {
            self.mb(
                &format!("release-group/{id}"),
                &[("inc", "artist-credits+url-rels+genres")],
                cancelled,
            )?
        } else {
            None
        };
        if query.album.is_empty()
            && query.release_id.is_none()
            && let Some(group) = &group
        {
            info.filled_album = text(group, "title");
        }
        let mut artist_id = query.artist_id.clone();
        if artist_id.is_none() {
            artist_id = recording
                .as_ref()
                .and_then(|r| credited_artist(r, artist_name))
                .or_else(|| group.as_ref().and_then(|r| credited_artist(r, artist_name)));
        }
        if artist_id.is_none() && !artist_name.is_empty() {
            let search = format!("artist:\"{}\"", lucene(artist_name));
            if let Some(found) = self.mb(
                "artist/",
                &[("query", &search), ("limit", "100")],
                cancelled,
            )? {
                artist_id = unique_search(&found, "artists", |v| {
                    same(v["name"].as_str().unwrap_or(""), artist_name)
                })
                .and_then(|v| mbid(&v));
            }
        }
        if let Some(group) = group
            && let Some(id) = mbid(&group)
        {
            let summary = self.summary(&group, cancelled);
            info.partial |= summary.is_err();
            info.album = Some(Entity {
                id,
                name: text(&group, "title").unwrap_or_default(),
                date: text(&group, "first-release-date").unwrap_or_default(),
                summary: summary.ok().flatten(),
                genres: genres(&group),
            });
        }
        if let Some(id) = artist_id
            && let Some(artist) = self.mb(
                &format!("artist/{id}"),
                &[("inc", "url-rels+genres")],
                cancelled,
            )?
            && mbid(&artist).is_some()
        {
            let summary = self.summary(&artist, cancelled);
            info.partial |= summary.is_err();
            info.artist = Some(Entity {
                id: id.clone(),
                name: text(&artist, "name").unwrap_or_default(),
                summary: summary.ok().flatten(),
                genres: genres(&artist),
                ..Default::default()
            });
            match self.discography(&id, 0, cancelled) {
                Ok(page) => info.discography = page,
                Err(_) => info.partial = true,
            }
        }
        if cancelled() {
            return Err(Failure::new("Lookup cancelled.", 0));
        }
        Ok((info.artist.is_some()
            || info.album.is_some()
            || info.filled_artist.is_some()
            || info.filled_album.is_some())
        .then_some(info))
    }

    pub(super) fn discography(
        &mut self,
        artist_id: &str,
        offset: u32,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Discography, Failure> {
        if !valid_mbid(artist_id) {
            return Err(Failure::new("Invalid artist identifier.", 60));
        }
        let response = self
            .mb(
                "release-group/",
                &[
                    ("artist", artist_id),
                    ("limit", "100"),
                    ("offset", &offset.to_string()),
                ],
                cancelled,
            )?
            .ok_or_else(|| Failure::new("No discography found.", 30))?;
        let releases = response["release-groups"]
            .as_array()
            .ok_or_else(|| Failure::new("MusicBrainz returned an invalid response.", 30))?;
        let total = response["release-group-count"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Failure::new("MusicBrainz returned an invalid response.", 30))?;
        let mut page = Discography {
            artist_id: artist_id.into(),
            offset,
            next_offset: offset.saturating_add(releases.len() as u32),
            total,
            ..Default::default()
        };
        let mut seen = HashSet::new();
        for release in releases {
            if let Some(id) = mbid(release).filter(|id| seen.insert(id.clone())) {
                page.entries.push(Release {
                    id,
                    title: text(release, "title").unwrap_or_default(),
                    date: text(release, "first-release-date").unwrap_or_default(),
                    kind: text(release, "primary-type").unwrap_or_default(),
                });
            }
        }
        page.sort();
        Ok(page)
    }

    fn summary(
        &mut self,
        entity: &Value,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<Summary>, Failure> {
        let relations = entity["relations"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut title = None;
        let mut wikidata = None;
        for relation in relations {
            let Some(link) = relation["url"]["resource"]
                .as_str()
                .and_then(|s| url::Url::parse(s).ok())
            else {
                continue;
            };
            if !matches!(link.scheme(), "https" | "http") {
                continue;
            }
            if link.host_str() == Some("en.wikipedia.org") && relation["type"] == "wikipedia" {
                if let Some(path) = link.path().strip_prefix("/wiki/") {
                    // Decode percent escapes without treating plus signs in titles as spaces.
                    title = Some(
                        url::form_urlencoded::parse(
                            format!("title={}", path.replace('+', "%2B").replace('&', "%26"))
                                .as_bytes(),
                        )
                        .next()
                        .unwrap()
                        .1
                        .into_owned(),
                    );
                }
            } else if link
                .host_str()
                .is_some_and(|s| s == "www.wikidata.org" || s == "wikidata.org")
                && relation["type"] == "wikidata"
                && let Some(id) = link.path().strip_prefix("/wiki/").filter(|id| {
                    id.starts_with('Q')
                        && id.len() > 1
                        && id[1..].bytes().all(|c| c.is_ascii_digit())
                })
            {
                wikidata = Some(id.to_string());
            }
        }
        if title.is_none()
            && let Some(id) = wikidata
        {
            let mut url = crate::services::http_url(&self.config.wikidata)
                .map_err(|_| Failure::new("Invalid metadata provider URL.", 60))?;
            url.query_pairs_mut().extend_pairs([
                ("action", "wbgetentities"),
                ("format", "json"),
                ("ids", &id),
                ("props", "sitelinks"),
                ("sitefilter", "enwiki"),
            ]);
            if let Some(data) = self.information_json(url, cancelled)? {
                title = data["entities"][&id]["sitelinks"]["enwiki"]["title"]
                    .as_str()
                    .map(str::to_string);
            }
        }
        let Some(title) = title.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let mut url = crate::services::http_url(&self.config.wikipedia)
            .map_err(|_| Failure::new("Invalid metadata provider URL.", 60))?;
        url.query_pairs_mut().extend_pairs([
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("prop", "extracts|pageprops|info"),
            ("inprop", "url"),
            ("exintro", "1"),
            ("explaintext", "1"),
            ("exchars", "1200"),
            ("redirects", "1"),
            ("titles", &title),
        ]);
        let Some(data) = self.information_json(url, cancelled)? else {
            return Ok(None);
        };
        let pages = data["query"]["pages"]
            .as_array()
            .ok_or_else(|| Failure::new("The provider returned an invalid response.", 30))?;
        let Some(page) = pages.first().filter(|p| {
            p.get("missing").is_none() && p["pageprops"].get("disambiguation").is_none()
        }) else {
            return Ok(None);
        };
        let Some(text) = text(page, "extract") else {
            return Ok(None);
        };
        let Some(source) = page["fullurl"].as_str().filter(|s| {
            url::Url::parse(s).is_ok_and(|u| {
                u.scheme() == "https"
                    && u.host_str() == Some("en.wikipedia.org")
                    && u.path().starts_with("/wiki/")
            })
        }) else {
            return Ok(None);
        };
        Ok(Some(Summary {
            text: text.chars().take(4000).collect(),
            source: source.into(),
        }))
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
fn mbid(value: &Value) -> Option<String> {
    text(value, "id").filter(|s| valid_mbid(s))
}
fn same(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && normalize(a) == normalize(b)
}
fn credit(value: &Value) -> String {
    value["artist-credit"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|v| {
                    format!(
                        "{}{}",
                        v["name"]
                            .as_str()
                            .or_else(|| v["artist"]["name"].as_str())
                            .unwrap_or(""),
                        v["joinphrase"].as_str().unwrap_or("")
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}
fn credit_matches(value: &Value, name: &str) -> bool {
    same(&credit(value), name)
        || value["artist-credit"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|v| same(v["artist"]["name"].as_str().unwrap_or(""), name))
        })
}
fn credited_artist(value: &Value, name: &str) -> Option<String> {
    let credits = value["artist-credit"].as_array()?;
    let ids: HashSet<_> = credits
        .iter()
        .filter(|v| {
            same(v["artist"]["name"].as_str().unwrap_or(""), name)
                || (credits.len() == 1 && credit_matches(value, name))
        })
        .filter_map(|v| mbid(&v["artist"]))
        .collect();
    (ids.len() == 1).then(|| ids.into_iter().next().unwrap())
}
fn genres(value: &Value) -> Vec<String> {
    value["genres"]
        .as_array()
        .map(|v| v.iter().filter_map(|v| text(v, "name")).take(8).collect())
        .unwrap_or_default()
}
fn unique_search(value: &Value, key: &str, matches: impl Fn(&Value) -> bool) -> Option<Value> {
    let results = value[key].as_array()?;
    if value["count"]
        .as_u64()
        .is_some_and(|n| n > results.len() as u64)
    {
        return None;
    }
    let mut seen = HashSet::new();
    let mut matches = results
        .iter()
        .filter(|v| {
            v["score"]
                .as_u64()
                .or_else(|| v["score"].as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0)
                >= 98
                && matches(v)
        })
        .filter(|v| mbid(v).is_some_and(|id| seen.insert(id)));
    let first = matches.next()?.clone();
    matches.next().is_none().then_some(first)
}
fn recording_matches(query: &Query, recording: &Value) -> bool {
    same(recording["title"].as_str().unwrap_or(""), &query.title)
        && (query.artist.is_empty() || credit_matches(recording, &query.artist))
        && recording["length"]
            .as_f64()
            .is_some_and(|n| (n / 1000.0 - query.duration).abs() <= 2.0)
}
fn matching_recording(query: &Query, value: &Value) -> Option<Value> {
    unique_search(value, "recordings", |r| {
        recording_matches(query, r)
            && (query.album.is_empty()
                || r["releases"].as_array().is_some_and(|releases| {
                    releases
                        .iter()
                        .any(|v| same(v["title"].as_str().unwrap_or(""), &query.album))
                }))
    })
}
fn release_recording(query: &Query, release: &Value) -> Option<Value> {
    let mut recordings: Vec<Value> = release["media"]
        .as_array()?
        .iter()
        .flat_map(|m| m["tracks"].as_array().into_iter().flatten())
        .map(|t| {
            let mut r = t["recording"].clone();
            if r["length"].is_null() {
                r["length"] = t["length"].clone();
            }
            r
        })
        .filter(|r| !query.title_from_filename && recording_matches(query, r))
        .collect();
    let ids: HashSet<_> = recordings.iter().filter_map(mbid).collect();
    (ids.len() == 1).then(|| recordings.remove(0))
}
fn unique_recording_release(query: &Query, recording: &Value) -> Option<Value> {
    let releases: Vec<_> = recording["releases"]
        .as_array()?
        .iter()
        .filter(|r| {
            r["status"].as_str().is_none_or(|s| s == "Official")
                && (query.album.is_empty() || same(r["title"].as_str().unwrap_or(""), &query.album))
        })
        .filter(|r| mbid(&r["release-group"]).is_some())
        .collect();
    let ids: HashSet<_> = releases
        .iter()
        .filter_map(|r| mbid(&r["release-group"]))
        .collect();
    (ids.len() == 1).then(|| releases[0].clone())
}
