//! Conservative album matching shared by the library and streaming catalogues.
use crate::{
    enrichment::normalize,
    model::{Kind, State, Track},
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Query {
    pub id: String,
    pub artist_id: String,
    pub artist: String,
    pub title: String,
    pub date: String,
}

impl Query {
    pub fn matches(&self, title: &str, artists: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
        let artist = normalize(&self.artist);
        let album = normalize(&self.title);
        !artist.is_empty()
            && !album.is_empty()
            && normalize(title) == album
            && artists.into_iter().any(|a| normalize(a.as_ref()) == artist)
    }

    pub fn local_tracks(&self, state: &State) -> Vec<String> {
        let mut tracks: Vec<&Track> = state
            .tracks
            .iter()
            .filter(|t| {
                if t.kind != Kind::Music || crate::library::local_path(&t.uri).is_none() {
                    return false;
                }
                if let Some(album) = t.music_info.as_ref().and_then(|i| i.album.as_ref()) {
                    if !album.id.is_empty() {
                        return album.id == self.id;
                    }
                }
                let artist = if t.album_artist.is_empty() {
                    &t.artist
                } else {
                    &t.album_artist
                };
                if let Some(id) = &t.musicbrainz_artist_id {
                    if !self.artist_id.is_empty()
                        && (t.album_artist.is_empty()
                            || normalize(&t.album_artist) == normalize(&t.artist))
                    {
                        return id == &self.artist_id
                            && normalize(&t.album) == normalize(&self.title);
                    }
                }
                self.matches(&t.album, [artist])
            })
            .collect();
        tracks.sort_by_key(|t| (t.number == 0, t.number, t.title.to_lowercase(), &t.id));
        tracks.into_iter().map(|t| t.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_matching_requires_artist_and_title_and_orders_local_tracks() {
        let query = Query {
            id: "group".into(),
            artist_id: "artist".into(),
            artist: "The Ensemble".into(),
            title: "Small Hours".into(),
            date: "2020".into(),
        };
        assert!(query.matches("SMALL HOURS!", ["The Ensemble"]));
        assert!(!query.matches("Small Hours", ["Other Ensemble"]));
        assert!(!query.matches("Small Hours Live", ["The Ensemble"]));
        let mut state = State::default();
        for (number, artist, kind) in [
            (2, "The Ensemble", Kind::Music),
            (1, "Guest", Kind::Music),
            (3, "Other Ensemble", Kind::Music),
            (4, "The Ensemble", Kind::Spotify),
        ] {
            let mut track = Track::new(&format!("file:///music/{number}.flac"), "Song", kind);
            track.artist = artist.into();
            if number == 1 {
                track.album_artist = "The Ensemble".into();
            }
            track.album = "Small Hours".into();
            track.number = number;
            state.tracks.push(track);
        }
        assert_eq!(
            query.local_tracks(&state),
            vec![state.tracks[1].id.clone(), state.tracks[0].id.clone()]
        );
        state.tracks[0].musicbrainz_artist_id = Some("another-artist".into());
        assert_eq!(query.local_tracks(&state), vec![state.tracks[1].id.clone()]);
        state.tracks[0].music_info = Some(crate::enrichment::Information {
            album: Some(crate::enrichment::Entity {
                id: "group".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(
            query.local_tracks(&state).len(),
            2,
            "Matched release groups take priority over spelling differences"
        );
    }
}
