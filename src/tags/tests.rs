use super::*;
use crate::{
    library,
    model::{Kind, State, Track},
};
use std::{path::Path, process::Command, sync::atomic::AtomicBool};

fn fixture(directory: &Path, extension: &str) -> Track {
    let path = directory.join(format!("song.{extension}"));
    let status = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.4",
            "-metadata",
            "title=Original",
            "-metadata",
            "artist=First artist",
            "-metadata",
            "album=Compilation",
            "-metadata",
            "album_artist=Various Artists",
            "-metadata",
            "comment=Keep this comment",
            "-metadata",
            "replaygain_track_gain=-4.00 dB",
            "-metadata",
            "musicbrainz_albumid=12345678-1234-1234-1234-123456789abc",
            "-metadata",
            "private_note=Keep this custom tag",
            "-metadata",
            "track=2/9",
            "-metadata",
            "disc=2/3",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success(), "{extension}");
    library::probe(&path, &directory.join("artwork")).unwrap()
}

fn pcm(path: &Path) -> Vec<u8> {
    let result = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "hash", "-hash", "sha256", "-"])
        .output()
        .unwrap();
    assert!(result.status.success());
    result.stdout
}

fn raw_tags(path: &Path) -> std::collections::BTreeMap<String, serde_json::Value> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    std::iter::once(&data["format"]["tags"])
        .chain(
            data["streams"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| &s["tags"]),
        )
        .filter_map(|t| t.as_object())
        .flat_map(|t| t.iter().map(|(k, v)| (k.to_lowercase(), v.clone())))
        .collect()
}

fn patch(fields: &[(Field, &str)]) -> Patch {
    Patch(fields.iter().map(|(k, v)| (*k, (*v).into())).collect())
}

#[test]
fn library_edits_survive_rescans_and_preserve_collection_identity() {
    let dir = tempfile::tempdir().unwrap();
    let original = fixture(dir.path(), "flac");
    let path = library::local_path(&original.uri).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let patch = patch(&[
        (Field::Artists, "One; Two"),
        (Field::Genres, "Jazz; Ambient"),
        (Field::Disc, "1"),
        (Field::Date, "2024-06-15"),
        (Field::Cover, ""),
        (Field::Lyrics, ""),
    ]);
    let mut result = save(
        vec![original.clone()],
        patch,
        Destination::Library,
        &dir.path().join("cache"),
        &AtomicBool::new(false),
        |_, _| {},
    );
    let edited = result.remove(0).updated.unwrap();
    assert_eq!(edited.artists, ["One", "Two"]);
    assert_eq!(edited.track_total, 9);
    assert_eq!(edited.disc_total, 3);
    let mut state = State {
        tracks: vec![edited.clone()],
        queue: vec![edited.id.clone()],
        current: Some(edited.id.clone()),
        ..State::default()
    };
    state.sanitize();
    state.tracks[0].favorite = true;
    state.merge(vec![
        library::probe(&path, &dir.path().join("cache")).unwrap(),
    ]);
    assert_eq!(state.tracks[0].artist, "One; Two");
    assert_eq!(state.tracks[0].disc, 1);
    assert!(state.tracks[0].favorite && state.tracks[0].matches("jazz 2024"));
    assert_eq!(state.current.as_deref(), Some(edited.id.as_str()));
    assert!(!crate::enrichment::missing(
        &state.tracks[0],
        crate::enrichment::Field::Cover
    ));
    let restored: State = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(restored.tracks[0].tag_overrides, edited.tag_overrides);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn file_edits_preserve_decoded_audio_and_unrelated_tags_across_formats() {
    for extension in ["flac", "mp3", "ogg", "opus", "m4a", "wav", "aiff"] {
        let dir = tempfile::tempdir().unwrap();
        let track = fixture(dir.path(), extension);
        let path = library::local_path(&track.uri).unwrap();
        let before = pcm(&path);
        let original_tags = raw_tags(&path);
        let patch = patch(&[
            (Field::Title, "Corrected"),
            (Field::Artists, "First; Second"),
            (Field::AlbumArtists, "Various Artists"),
            (Field::Disc, "1"),
            (Field::DiscTotal, "2"),
            (Field::Number, "3"),
            (Field::TrackTotal, "12"),
            (Field::Date, "2024-06-15"),
            (Field::Genres, "Jazz; Ambient"),
            (Field::Composers, "Composer"),
            (Field::Lyrics, "First line\nSecond line"),
        ]);
        let result = save(
            vec![track],
            patch,
            Destination::Files,
            &dir.path().join("cache"),
            &AtomicBool::new(false),
            |_, _| {},
        )
        .remove(0);
        let edited = result
            .updated
            .unwrap_or_else(|e| panic!("{extension}: {e}"));
        assert_eq!(edited.title, "Corrected", "{extension}");
        assert_eq!(edited.artists, ["First", "Second"], "{extension}");
        assert_eq!(edited.disc, 1);
        assert_eq!(edited.track_total, 12);
        assert_eq!(pcm(&path), before, "audio changed in {extension}");
        let preserved_tags = raw_tags(&path);
        for key in [
            "comment",
            "replaygain_track_gain",
            "musicbrainz_albumid",
            "private_note",
        ] {
            if let Some(value) = original_tags.get(key) {
                assert_eq!(
                    preserved_tags.get(key),
                    Some(value),
                    "{extension}: {key} was lost"
                );
            }
        }
        assert_eq!(edited.uri, result.original.uri);
        assert_eq!(edited.id, result.original.id);
    }
}

#[test]
fn artwork_lyrics_and_cleared_tags_round_trip_without_rewriting_sidecars() {
    for extension in ["flac", "mp3", "m4a", "ogg"] {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let original = fixture(dir.path(), extension);
        let path = library::local_path(&original.uri).unwrap();
        let before = pcm(&path);
        let cover_path = dir.path().join("cover.png");
        let image = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=32x32",
                "-frames:v",
                "1",
                "-threads",
                "1",
            ])
            .arg(&cover_path)
            .output()
            .unwrap();
        assert!(
            image.status.success(),
            "{}",
            String::from_utf8_lossy(&image.stderr)
        );
        let image_bytes = std::fs::read(&cover_path).unwrap();
        let sidecar = path.with_extension("lrc");
        std::fs::write(&sidecar, "[00:01.00]Sidecar lyrics").unwrap();
        let write = |track, edit| {
            save(
                vec![track],
                edit,
                Destination::Files,
                &cache,
                &AtomicBool::new(false),
                |_, _| {},
            )
            .remove(0)
            .updated
            .unwrap_or_else(|error| panic!("{extension}: {error}"))
        };
        let edited = write(original, patch(&[(Field::Genres, "Jazz")]));
        assert_eq!(edited.artwork.as_deref(), cover_path.to_str());
        assert!(edited.lyrics.as_deref().unwrap().contains("Sidecar lyrics"));
        let edited = write(
            edited,
            patch(&[
                (Field::Cover, cover_path.to_str().unwrap()),
                (Field::Lyrics, "[00:02.00]Edited lyrics"),
            ]),
        );
        let embedded = io::parse(&path).unwrap();
        use lofty::file::TaggedFileExt;
        assert_eq!(
            embedded.primary_tag().unwrap().pictures()[0].data(),
            image_bytes
        );
        assert!(edited.lyrics.as_deref().unwrap().contains("Edited lyrics"));
        let edited = write(
            edited,
            patch(&[
                (Field::Title, ""),
                (Field::Artists, ""),
                (Field::Number, ""),
                (Field::TrackTotal, ""),
                (Field::Cover, ""),
                (Field::Lyrics, ""),
            ]),
        );
        assert_eq!(edited.title, "song");
        assert!(edited.title_from_filename && edited.artists.is_empty());
        assert_eq!((edited.number, edited.track_total), (0, 0));
        assert!(edited.artwork.is_none() && edited.lyrics.is_none());
        assert!(
            io::parse(&path)
                .unwrap()
                .primary_tag()
                .unwrap()
                .pictures()
                .is_empty()
        );
        assert_eq!(std::fs::read(&cover_path).unwrap(), image_bytes);
        assert_eq!(
            std::fs::read_to_string(&sidecar).unwrap(),
            "[00:01.00]Sidecar lyrics"
        );
        assert_eq!(pcm(&path), before);
    }
}

#[test]
fn cancelling_a_batch_keeps_completed_edits_and_leaves_remaining_files_intact() {
    use std::sync::atomic::Ordering;
    let dir = tempfile::tempdir().unwrap();
    let first = fixture(dir.path(), "flac");
    let second = fixture(dir.path(), "mp3");
    let second_path = library::local_path(&second.uri).unwrap();
    let original_bytes = std::fs::read(&second_path).unwrap();
    let cancel = AtomicBool::new(false);
    let results = save(
        vec![first, second],
        patch(&[(Field::Title, "Changed")]),
        Destination::Files,
        dir.path(),
        &cancel,
        |done, _| {
            if done == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    );
    assert_eq!(results[0].updated.as_ref().unwrap().title, "Changed");
    assert_eq!(results[1].updated.as_ref().unwrap_err(), "Cancelled");
    assert_eq!(std::fs::read(second_path).unwrap(), original_bytes);
}

#[test]
fn failed_and_cancelled_writes_keep_original_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mut track = fixture(dir.path(), "flac");
    let path = library::local_path(&track.uri).unwrap();
    let before = std::fs::read(&path).unwrap();
    let edit = patch(&[(Field::Title, "New")]);
    let cancelled = save(
        vec![track.clone()],
        edit.clone(),
        Destination::Files,
        dir.path(),
        &AtomicBool::new(true),
        |_, _| {},
    );
    assert!(cancelled[0].updated.is_err());
    track.size += 1;
    let stale = save(
        vec![track.clone()],
        edit.clone(),
        Destination::Files,
        dir.path(),
        &AtomicBool::new(false),
        |_, _| {},
    );
    assert!(stale[0].updated.is_err());
    track.size -= 1;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let denied = save(
        vec![track],
        edit,
        Destination::Files,
        dir.path(),
        &AtomicBool::new(false),
        |_, _| {},
    );
    assert!(denied[0].updated.is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn id3v23_recording_and_private_identifiers_survive_tag_edits() {
    use lofty::{
        config::{ParseOptions, WriteOptions},
        file::AudioFile,
        id3::v2::{Frame, UniqueFileIdentifierFrame},
        mpeg::MpegFile,
        tag::TagExt,
    };
    let dir = tempfile::tempdir().unwrap();
    let original = fixture(dir.path(), "mp3");
    let path = library::local_path(&original.uri).unwrap();
    let native = MpegFile::read_from(
        &mut std::fs::File::open(&path).unwrap(),
        ParseOptions::new(),
    )
    .unwrap();
    let mut tag = native.id3v2().unwrap().clone();
    let recording = b"12345678-1234-1234-1234-123456789abc".to_vec();
    let private = vec![0, 254, 1, 255, 0];
    for (owner, payload) in [
        ("http://musicbrainz.org", recording.clone()),
        ("test.example", private.clone()),
    ] {
        tag.insert(Frame::UniqueFileIdentifier(UniqueFileIdentifierFrame::new(
            owner, payload,
        )));
    }
    tag.save_to_path(&path, WriteOptions::new().use_id3v23(true))
        .unwrap();
    let original = library::probe(&path, dir.path()).unwrap();
    let before = pcm(&path);
    let result = save(
        vec![original],
        patch(&[(Field::Title, "Changed")]),
        Destination::Files,
        dir.path(),
        &AtomicBool::new(false),
        |_, _| {},
    )
    .remove(0)
    .updated
    .unwrap();
    assert_eq!(
        result.musicbrainz_recording_id.as_deref(),
        Some("12345678-1234-1234-1234-123456789abc")
    );
    let saved = MpegFile::read_from(
        &mut std::fs::File::open(&path).unwrap(),
        ParseOptions::new(),
    )
    .unwrap();
    let identifiers: std::collections::BTreeMap<_, _> = saved
        .id3v2()
        .unwrap()
        .into_iter()
        .filter_map(|frame| match frame {
            Frame::UniqueFileIdentifier(id) => Some((id.owner.to_string(), id.identifier.to_vec())),
            _ => None,
        })
        .collect();
    assert_eq!(identifiers.get("http://musicbrainz.org"), Some(&recording));
    assert_eq!(identifiers.get("test.example"), Some(&private));
    assert_eq!(pcm(&path), before);
}

#[test]
fn album_keys_group_compilations_and_order_discs_before_tracks() {
    let mut tracks: Vec<_> = [(2, 1, "Guest"), (1, 2, "Another artist"), (1, 1, "Artist")]
        .into_iter()
        .map(|(disc, number, artist)| {
            let mut t = Track::new(
                &format!("file:///{disc}-{number}.flac"),
                "Song",
                Kind::Music,
            );
            t.album = "Compilation".into();
            t.artist = artist.into();
            t.album_artist = "Various Artists".into();
            t.disc = disc;
            t.number = number;
            t
        })
        .collect();
    assert!(
        tracks
            .iter()
            .all(|t| t.album_key() == tracks[0].album_key())
    );
    tracks.sort_by(|a, b| a.album_order().cmp(&b.album_order()));
    assert_eq!(
        tracks
            .iter()
            .map(|t| (t.disc, t.number))
            .collect::<Vec<_>>(),
        [(1, 1), (1, 2), (2, 1)]
    );
    assert!(
        patch(&[(Field::Number, "13"), (Field::TrackTotal, "12")])
            .validate(&tracks)
            .is_err()
    );
    assert!(patch(&[(Field::Disc, "-1")]).validate(&tracks).is_err());
}
