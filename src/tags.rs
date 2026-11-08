//! Local tag editing, with explicit field patches and durable library-only overrides.
mod io;
#[cfg(test)]
mod tests;

use crate::model::Track;
pub use io::{modified, read, save, writable};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Title,
    Artists,
    Album,
    AlbumArtists,
    Number,
    TrackTotal,
    Disc,
    DiscTotal,
    Date,
    Genres,
    Composers,
    Cover,
    Lyrics,
}

impl Field {
    pub const TEXT: [Self; 11] = [
        Self::Title,
        Self::Artists,
        Self::Album,
        Self::AlbumArtists,
        Self::Number,
        Self::TrackTotal,
        Self::Disc,
        Self::DiscTotal,
        Self::Date,
        Self::Genres,
        Self::Composers,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Artists => "Artists",
            Self::Album => "Album",
            Self::AlbumArtists => "Album artists",
            Self::Number => "Track number",
            Self::TrackTotal => "Total tracks",
            Self::Disc => "Disc number",
            Self::DiscTotal => "Total discs",
            Self::Date => "Date",
            Self::Genres => "Genres",
            Self::Composers => "Composers",
            Self::Cover => "Artwork",
            Self::Lyrics => "Lyrics",
        }
    }

    pub fn numeric(self) -> bool {
        matches!(
            self,
            Self::Number | Self::TrackTotal | Self::Disc | Self::DiscTotal
        )
    }

    pub fn value(self, track: &Track) -> String {
        let number = |n: u32| if n == 0 { String::new() } else { n.to_string() };
        match self {
            Self::Title => track.title.clone(),
            Self::Artists => {
                if track.artists.is_empty() {
                    track.artist.clone()
                } else {
                    track.artists.join("; ")
                }
            }
            Self::Album => track.album.clone(),
            Self::AlbumArtists => {
                if track.album_artists.is_empty() {
                    track.album_artist.clone()
                } else {
                    track.album_artists.join("; ")
                }
            }
            Self::Number => number(track.number),
            Self::TrackTotal => number(track.track_total),
            Self::Disc => number(track.disc),
            Self::DiscTotal => number(track.disc_total),
            Self::Date => track.date.clone(),
            Self::Genres => track.genres.join("; "),
            Self::Composers => track.composers.join("; "),
            Self::Cover => track.artwork.clone().unwrap_or_default(),
            Self::Lyrics => track
                .synced_lyrics
                .as_ref()
                .or(track.lyrics.as_ref())
                .cloned()
                .unwrap_or_default(),
        }
    }
}

pub fn values(text: &str) -> Vec<String> {
    text.split(';')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(String::from)
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Patch(pub BTreeMap<Field, String>);

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn contains(&self, field: Field) -> bool {
        self.0.contains_key(&field)
    }

    pub fn validate(&self, tracks: &[Track]) -> Result<(), &'static str> {
        for (&field, value) in &self.0 {
            if value.contains('\0') {
                return Err("Tags cannot contain NUL characters.");
            }
            if value.len()
                > if field == Field::Lyrics {
                    512_000
                } else {
                    16_384
                }
            {
                return Err("This tag is too long.");
            }
            if field.numeric() && !value.trim().is_empty() && value.trim().parse::<u16>().is_err() {
                return Err("Enter whole numbers between 0 and 65535.");
            }
            if field == Field::Date
                && !value.trim().is_empty()
                && value
                    .trim()
                    .parse::<lofty::tag::items::Timestamp>()
                    .is_err()
            {
                return Err("Enter a valid date, such as 2024 or 2024-06-15.");
            }
        }
        for track in tracks {
            let mut edited = track.clone();
            self.apply(&mut edited);
            if (self.contains(Field::Number) || self.contains(Field::TrackTotal))
                && edited.track_total != 0
                && edited.number > edited.track_total
            {
                return Err("Track number cannot exceed total tracks.");
            }
            if (self.contains(Field::Disc) || self.contains(Field::DiscTotal))
                && edited.disc_total != 0
                && edited.disc > edited.disc_total
            {
                return Err("Disc number cannot exceed total discs.");
            }
        }
        Ok(())
    }

    pub fn apply(&self, track: &mut Track) {
        for (&field, value) in &self.0 {
            match field {
                Field::Title => {
                    track.title = value.trim().to_string();
                    track.title_from_filename = false;
                    if track.title.is_empty() {
                        track.title = crate::library::local_path(&track.uri)
                            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                            .unwrap_or_default();
                        track.title_from_filename = true;
                    }
                }
                Field::Artists => {
                    track.artists = values(value);
                    track.artist = track.artists.join("; ");
                }
                Field::Album => track.album = value.trim().to_string(),
                Field::AlbumArtists => {
                    track.album_artists = values(value);
                    track.album_artist = track.album_artists.join("; ");
                }
                Field::Number => track.number = value.trim().parse().unwrap_or(0),
                Field::TrackTotal => track.track_total = value.trim().parse().unwrap_or(0),
                Field::Disc => track.disc = value.trim().parse().unwrap_or(0),
                Field::DiscTotal => track.disc_total = value.trim().parse().unwrap_or(0),
                Field::Date => track.date = value.trim().to_string(),
                Field::Genres => track.genres = values(value),
                Field::Composers => track.composers = values(value),
                Field::Cover => {
                    track.artwork = (!value.is_empty()).then(|| value.clone());
                    track.artwork_source = None;
                }
                Field::Lyrics => {
                    track.lyrics = (!value.is_empty()).then(|| value.clone());
                    track.synced_lyrics = None;
                    track.lyrics_source = None;
                    track.lyrics_timing_checked = true;
                    track.instrumental = false;
                }
            }
        }
    }

    pub fn changes_identity(&self) -> bool {
        [
            Field::Title,
            Field::Artists,
            Field::Album,
            Field::AlbumArtists,
        ]
        .iter()
        .any(|f| self.contains(*f))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    Library,
    Files,
    Reload,
}

pub struct Saved {
    pub original: Track,
    pub updated: Result<Track, String>,
}
