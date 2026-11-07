use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub text: String,
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub summary: Option<Summary>,
    pub date: String,
    pub genres: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub date: String,
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Discography {
    pub artist_id: String,
    pub entries: Vec<Release>,
    pub offset: u32,
    pub next_offset: u32,
    pub total: u32,
}

impl Discography {
    pub fn has_more(&self) -> bool {
        self.next_offset > self.offset && self.next_offset < self.total
    }

    pub fn append(&mut self, page: &Self) {
        if self.artist_id != page.artist_id || self.next_offset != page.offset {
            return;
        }
        for release in &page.entries {
            if !self.entries.iter().any(|r| r.id == release.id) {
                self.entries.push(release.clone());
            }
        }
        self.offset = page.offset;
        self.next_offset = page.next_offset;
        self.total = page.total;
        self.sort();
    }

    pub fn sort(&mut self) {
        self.entries.sort_by(|a, b| {
            (a.date.is_empty(), &a.date, &a.title).cmp(&(b.date.is_empty(), &b.date, &b.title))
        });
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Information {
    pub key: String,
    pub checked: u64,
    pub artist: Option<Entity>,
    pub album: Option<Entity>,
    pub discography: Discography,
    pub partial: bool,
    // Provenance for library-only fallbacks, never file-tag writes.
    pub filled_artist: Option<String>,
    pub filled_album: Option<String>,
}

pub(super) fn apply_information(track: &mut Track, information: &Information) {
    let mut info = information.clone();
    if track.artist.trim().is_empty() {
        if let Some(artist) = &info.filled_artist {
            track.artist.clone_from(artist);
        }
    } else {
        info.filled_artist = track
            .music_info
            .as_ref()
            .and_then(|i| i.filled_artist.as_ref())
            .filter(|s| *s == &track.artist)
            .cloned();
    }
    if track.album.trim().is_empty() {
        if let Some(album) = &info.filled_album {
            track.album.clone_from(album);
        }
    } else {
        info.filled_album = track
            .music_info
            .as_ref()
            .and_then(|i| i.filled_album.as_ref())
            .filter(|s| *s == &track.album)
            .cloned();
    }
    if info.partial {
        if let Some(old) = &track.music_info {
            for (new, previous) in [
                (&mut info.artist, &old.artist),
                (&mut info.album, &old.album),
            ] {
                if let (Some(new), Some(previous)) = (new, previous) {
                    if new.id == previous.id && new.summary.is_none() {
                        new.summary.clone_from(&previous.summary);
                    }
                }
            }
        }
    }
    if let Some(query) = Query::for_track(track, Field::Information) {
        info.key = query.key;
    }
    track.music_info = Some(info);
}

pub(super) fn preserve_information(old: &Track, new: &mut Track) {
    let Some(info) = old.music_info.as_ref() else {
        return;
    };
    // A tag edit invalidates fallbacks. Fresh nonempty file/service tags always win.
    if normalize(&old.title) != normalize(&new.title)
        || (!new.artist.trim().is_empty() && normalize(&new.artist) != normalize(&old.artist))
        || (!new.album.trim().is_empty() && normalize(&new.album) != normalize(&old.album))
        || old.musicbrainz_release_id != new.musicbrainz_release_id
        || old.musicbrainz_recording_id != new.musicbrainz_recording_id
        || old.musicbrainz_artist_id != new.musicbrainz_artist_id
    {
        return;
    }
    let mut candidate = new.clone();
    apply_information(&mut candidate, info);
    if Query::for_track(old, Field::Information)
        .zip(Query::for_track(&candidate, Field::Information))
        .is_some_and(|(a, b)| a.key == b.key)
    {
        new.artist = candidate.artist;
        new.album = candidate.album;
        new.music_info = candidate.music_info;
    }
}
