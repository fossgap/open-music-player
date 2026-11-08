use super::{Destination, Field, Patch, Saved, values};
use crate::{library, model::Track};
use lofty::{
    config::{ParseOptions, WriteOptions},
    file::{AudioFile, FileType, TaggedFile, TaggedFileExt},
    picture::{MimeType, Picture, PictureType},
    probe::Probe,
    tag::{ItemKey, ItemValue, MergeTag, SplitTag, Tag, TagExt, TagItem, TagType},
};
use std::{
    fs,
    hash::{Hash, Hasher},
    io::Cursor,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};

pub(super) fn parse(path: &Path) -> Result<TaggedFile, String> {
    Probe::open(path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new().read_properties(false))
        .read()
        .map_err(|e| e.to_string())
}

fn key(field: Field, kind: TagType) -> Option<ItemKey> {
    Some(match field {
        Field::Title => ItemKey::TrackTitle,
        Field::Artists => ItemKey::TrackArtist,
        Field::Album => ItemKey::AlbumTitle,
        Field::AlbumArtists => ItemKey::AlbumArtist,
        Field::Number => ItemKey::TrackNumber,
        Field::TrackTotal => ItemKey::TrackTotal,
        Field::Disc => ItemKey::DiscNumber,
        Field::DiscTotal => ItemKey::DiscTotal,
        Field::Date => ItemKey::RecordingDate,
        Field::Genres => ItemKey::Genre,
        Field::Composers => ItemKey::Composer,
        Field::Lyrics => {
            if kind == TagType::Id3v2 {
                ItemKey::UnsyncLyrics
            } else {
                ItemKey::Lyrics
            }
        }
        Field::Cover => return None,
    })
}

fn text(tag: &Tag, field: Field) -> String {
    let Some(key) = key(field, tag.tag_type()) else {
        return String::new();
    };
    let strings: Vec<_> = tag.get_strings(key).collect();
    let result = strings.join("; ");
    if result.is_empty() && field == Field::Date {
        tag.get_string(ItemKey::Year)
            .unwrap_or_default()
            .to_string()
    } else {
        result
    }
}

/// Read native multi-value tags when possible; FFprobe remains the fallback for other containers.
pub fn read(path: &Path, track: &mut Track, cache: &Path) -> Result<(), String> {
    let file = parse(path)?;
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return Ok(());
    };
    let patch = Patch(
        Field::TEXT
            .into_iter()
            .filter_map(|field| {
                let value = std::iter::once(tag)
                    .chain(
                        file.tags()
                            .iter()
                            .filter(|t| t.tag_type() != tag.tag_type()),
                    )
                    .map(|t| text(t, field))
                    .find(|s| !s.is_empty())?;
                Some((field, value))
            })
            .collect(),
    );
    patch.apply(track);
    if let Some(lyrics) = tag
        .get_string(ItemKey::Lyrics)
        .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
    {
        track.lyrics = Some(lyrics.into());
    }
    for (key, value) in [
        (
            ItemKey::MusicBrainzReleaseId,
            &mut track.musicbrainz_release_id,
        ),
        (
            ItemKey::MusicBrainzRecordingId,
            &mut track.musicbrainz_recording_id,
        ),
        (
            ItemKey::MusicBrainzArtistId,
            &mut track.musicbrainz_artist_id,
        ),
    ] {
        if let Some(id) = tag
            .get_string(key)
            .filter(|id| crate::enrichment::valid_mbid(id))
        {
            *value = Some(id.to_string());
        }
    }
    if let Some(picture) = tag
        .get_picture_type(PictureType::CoverFront)
        .or_else(|| tag.pictures().first())
    {
        if picture.data().len() <= 20_000_000 {
            track.artwork = Some(cache_picture(picture, cache)?);
        }
    }
    Ok(())
}

fn cache_picture(picture: &Picture, cache: &Path) -> Result<String, String> {
    let extension = match picture.mime_type() {
        Some(MimeType::Jpeg) => "jpg",
        Some(MimeType::Png) => "png",
        _ => return Err("Choose a PNG or JPEG cover.".into()),
    };
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    picture.data().hash(&mut hash);
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let path = cache.join(format!("tags-{:016x}.{extension}", hash.finish()));
    if !path.is_file() {
        let mut temp = tempfile::NamedTempFile::new_in(cache).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut temp, picture.data()).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(&path).map_err(|e| e.to_string())?;
    }
    Ok(path.to_string_lossy().into_owned())
}

fn picture(path: &str) -> Result<Picture, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("Choose a local PNG or JPEG cover.".into());
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 20_000_000 {
        return Err("Choose a cover smaller than 20 MB.".into());
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, 20_000_001), &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 20_000_000 {
        return Err("Choose a cover smaller than 20 MB.".into());
    }
    let mut picture = Picture::from_reader(&mut Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if !matches!(picture.mime_type(), Some(MimeType::Png | MimeType::Jpeg)) {
        return Err("Choose a PNG or JPEG cover.".into());
    }
    picture.set_pic_type(PictureType::CoverFront);
    Ok(picture)
}

pub fn writable(track: &Track) -> bool {
    library::local_path(&track.uri)
        .and_then(|p| p.extension().map(|s| s.to_string_lossy().to_lowercase()))
        .is_some_and(|ext| {
            matches!(
                ext.as_str(),
                "mp3"
                    | "flac"
                    | "ogg"
                    | "opus"
                    | "m4a"
                    | "mp4"
                    | "alac"
                    | "wav"
                    | "aiff"
                    | "aif"
                    | "ape"
                    | "aac"
            )
        })
}

#[derive(PartialEq, Eq)]
struct Stamp {
    dev: u64,
    inode: u64,
    size: u64,
    modified: u64,
    changed: (i64, i64),
}
impl Stamp {
    fn read(path: &Path) -> Result<Self, String> {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.file_type().is_file() {
            return Err("Only regular local files can be tagged.".into());
        }
        Ok(Self {
            dev: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified: modified(&meta),
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}

pub fn modified(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|t| t.as_nanos().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn update_tag(
    tag: &mut Tag,
    patch: &Patch,
    cover: Option<&Picture>,
    primary: bool,
) -> Result<(), String> {
    for (&field, value) in &patch.0 {
        if field == Field::Cover {
            if primary {
                // Replacing the album cover leaves booklet/back-cover pictures intact.
                tag.remove_picture_type(PictureType::CoverFront);
                tag.remove_picture_type(PictureType::Other);
                if let Some(picture) = cover {
                    tag.push_picture(picture.clone());
                }
            }
            continue;
        }
        let item_key = key(field, tag.tag_type()).unwrap();
        tag.remove_key(item_key);
        // Do not leave alternate representations to resurrect a cleared/changed field.
        match field {
            Field::Artists => tag.remove_key(ItemKey::TrackArtists),
            Field::AlbumArtists => tag.remove_key(ItemKey::AlbumArtists),
            Field::Date => tag.remove_key(ItemKey::Year),
            Field::Lyrics => {
                tag.remove_key(ItemKey::Lyrics);
                tag.remove_key(ItemKey::UnsyncLyrics);
            }
            _ => {}
        }
        let list = if matches!(
            field,
            Field::Artists | Field::AlbumArtists | Field::Genres | Field::Composers
        ) {
            values(value)
        } else if value.trim().is_empty() || (field.numeric() && value.trim() == "0") {
            Vec::new()
        } else {
            vec![if field == Field::Lyrics {
                value.clone()
            } else {
                value.trim().to_string()
            }]
        };
        for value in list {
            if !tag.push(TagItem::new(item_key, ItemValue::Text(value))) && primary {
                return Err(format!(
                    "{}: This file format cannot store this tag.",
                    field.label()
                ));
            }
        }
    }
    Ok(())
}

fn native_file<T: AudioFile>(path: &Path) -> Result<T, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    T::read_from(&mut file, ParseOptions::new().read_properties(false)).map_err(|e| e.to_string())
}

fn merge_native<T>(original: T, edited: &Tag, path: &Path) -> Result<(), String>
where
    T: SplitTag,
    <T::Remainder as MergeTag>::Merged: TagExt<Err = lofty::error::LoftyError>,
{
    // Generic tags do not retain unfamiliar Vorbis, APE or RIFF keys. Keep the native remainder.
    original
        .split_tag()
        .0
        .merge_tag(edited.clone())
        .save_to_path(path, WriteOptions::new())
        .map_err(|e| e.to_string())
}

fn save_preserving(path: &Path, format: FileType, edited: &Tag) -> Result<(), String> {
    match (format, edited.tag_type()) {
        (FileType::Flac, TagType::VorbisComments) => merge_native(
            native_file::<lofty::flac::FlacFile>(path)?
                .vorbis_comments()
                .cloned()
                .unwrap_or_default(),
            edited,
            path,
        ),
        (FileType::Vorbis, TagType::VorbisComments) => merge_native(
            native_file::<lofty::ogg::VorbisFile>(path)?
                .vorbis_comments()
                .clone(),
            edited,
            path,
        ),
        (FileType::Opus, TagType::VorbisComments) => merge_native(
            native_file::<lofty::ogg::OpusFile>(path)?
                .vorbis_comments()
                .clone(),
            edited,
            path,
        ),
        (FileType::Speex, TagType::VorbisComments) => merge_native(
            native_file::<lofty::ogg::SpeexFile>(path)?
                .vorbis_comments()
                .clone(),
            edited,
            path,
        ),
        (FileType::Ape, TagType::Ape) => merge_native(
            native_file::<lofty::ape::ApeFile>(path)?
                .ape()
                .cloned()
                .unwrap_or_default(),
            edited,
            path,
        ),
        (FileType::Mpeg, TagType::Ape) => merge_native(
            native_file::<lofty::mpeg::MpegFile>(path)?
                .ape()
                .cloned()
                .unwrap_or_default(),
            edited,
            path,
        ),
        (FileType::Wav, TagType::RiffInfo) => merge_native(
            native_file::<lofty::iff::wav::WavFile>(path)?
                .riff_info()
                .cloned()
                .unwrap_or_default(),
            edited,
            path,
        ),
        (FileType::Aiff, TagType::AiffText) => merge_native(
            native_file::<lofty::iff::aiff::AiffFile>(path)?
                .text_chunks()
                .cloned()
                .unwrap_or_default(),
            edited,
            path,
        ),
        // ID3v2 and MP4 retain format-specific frames/atoms inside their generic tag companion.
        _ => edited
            .save_to_path(path, WriteOptions::new())
            .map_err(|e| e.to_string()),
    }
}

fn write_file(
    track: &Track,
    patch: &Patch,
    cover: Option<&Picture>,
    cache: &Path,
    cancel: &AtomicBool,
) -> Result<Track, String> {
    if !writable(track) {
        return Err("This format supports library-only edits.".into());
    }
    let path = library::local_path(&track.uri).ok_or("Only local music files can be tagged.")?;
    let original = Stamp::read(&path)?;
    if (track.modified != 0 && track.modified != original.modified)
        || (track.size != 0 && track.size != original.size)
    {
        return Err(
            "The file changed since it was scanned. Refresh the library and try again.".into(),
        );
    }
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    if metadata.permissions().mode() & 0o222 == 0 {
        return Err("This file is read-only. Use library-only edits.".into());
    }
    if metadata.nlink() > 1 {
        return Err("Files with multiple hard links support library-only edits.".into());
    }
    let parent = path.parent().ok_or("Invalid file location.")?;
    let directory = tempfile::Builder::new()
        .prefix(".open-audio-tags-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    let staged = directory.path().join("edited");
    let backup = directory.path().join("original");
    fs::copy(&path, &staged).map_err(|e| e.to_string())?;
    if Stamp::read(&path)? != original {
        return Err("The file changed while preparing the edit. Try again.".into());
    }
    let file = parse(&staged)?;
    let primary_type = file.primary_tag_type();
    let mut primary = file
        .primary_tag()
        .cloned()
        .unwrap_or_else(|| Tag::new(primary_type));
    update_tag(&mut primary, patch, cover, true)?;
    save_preserving(&staged, file.file_type(), &primary)?;
    for tag in file.tags().iter().filter(|t| t.tag_type() != primary_type) {
        // Legacy secondary tags only receive the fields they support. Never remove unrelated tags.
        if file.file_type().tag_support(tag.tag_type()).is_writable() {
            let mut secondary = tag.clone();
            update_tag(&mut secondary, patch, None, false)?;
            save_preserving(&staged, file.file_type(), &secondary)?;
        }
    }
    let checked = parse(&staged)?;
    let tag = checked
        .primary_tag()
        .ok_or("Could not verify the saved tags.")?;
    for (&field, wanted) in &patch.0 {
        if field == Field::Cover {
            if let Some(cover) = cover {
                if !tag.pictures().iter().any(|p| {
                    p.data() == cover.data()
                        && (p.pic_type() == PictureType::CoverFront
                            || tag.tag_type() == TagType::Mp4Ilst)
                }) {
                    return Err("Could not verify the saved cover.".into());
                }
            } else if tag.get_picture_type(PictureType::CoverFront).is_some()
                || tag.get_picture_type(PictureType::Other).is_some()
            {
                return Err("Could not remove the embedded cover.".into());
            }
        } else {
            let actual = text(tag, field);
            let equal = if field.numeric() {
                actual.parse::<u32>().unwrap_or(0) == wanted.trim().parse::<u32>().unwrap_or(0)
            } else if matches!(
                field,
                Field::Artists | Field::AlbumArtists | Field::Genres | Field::Composers
            ) {
                values(&actual) == values(wanted)
            } else {
                actual.trim() == wanted.trim()
            };
            if !equal {
                return Err(format!(
                    "{}: This format did not preserve the requested value. The original file was kept.",
                    field.label()
                ));
            }
        }
    }
    // Probe the replacement before committing; a failed parse must never replace the original.
    let mut updated = library::probe(&staged, cache)?;
    if updated.audio_details.codec != track.audio_details.codec
        || (track.duration > 0.0 && (updated.duration - track.duration).abs() > 0.1)
    {
        return Err(
            "The edited file did not pass audio validation. The original file was kept.".into(),
        );
    }
    updated.id = track.id.clone();
    updated.uri = track.uri.clone();
    updated.source = track.source.clone();
    library::refresh_sidecars(&path, &mut updated);
    if Stamp::read(&path)? != original {
        return Err("The file changed while preparing the edit. Try again.".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    if fs::hard_link(&path, &backup).is_err() {
        // FAT/exFAT music drives cannot hard-link a recovery copy.
        fs::copy(&path, &backup).map_err(|e| e.to_string())?;
        fs::File::open(&backup)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    // Hard-link creation changes ctime, so compare the original inode, content timestamp and size.
    let final_stamp = Stamp::read(&path)?;
    if final_stamp.inode != original.inode
        || final_stamp.dev != original.dev
        || final_stamp.size != original.size
        || final_stamp.modified != original.modified
    {
        return Err("The file changed while preparing the edit. Try again.".into());
    }
    fs::set_permissions(&staged, metadata.permissions()).map_err(|e| e.to_string())?;
    fs::File::open(&staged)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    let staged_metadata = fs::metadata(&staged).map_err(|e| e.to_string())?;
    fs::rename(&staged, &path).map_err(|e| e.to_string())?;
    if let Err(error) = fs::File::open(parent).and_then(|f| f.sync_all()) {
        if fs::rename(&backup, &path).is_err() {
            let recovery = directory.keep();
            return Err(format!(
                "{error}. The original file is retained at {}",
                recovery.join("original").display()
            ));
        }
        return Err(error.to_string());
    }
    updated.modified = modified(&staged_metadata);
    updated.size = staged_metadata.len();
    updated.tag_overrides = track.tag_overrides.clone();
    for &field in patch.0.keys() {
        updated.tag_overrides.0.remove(&field);
    }
    // Keep deliberate clears from being filled again by automatic metadata lookup.
    for field in [Field::Artists, Field::Album] {
        if patch
            .0
            .get(&field)
            .is_some_and(|value| value.trim().is_empty())
        {
            updated.tag_overrides.0.insert(field, String::new());
        }
    }
    // Explicit cover/lyrics choices outrank existing sidecars, without modifying shared files.
    for field in [Field::Cover, Field::Lyrics] {
        if let Some(value) = patch.0.get(&field) {
            updated.tag_overrides.0.insert(field, value.clone());
        }
    }
    updated.tag_overrides.clone().apply(&mut updated);
    crate::enrichment::preserve_downloaded(track, &mut updated);
    if updated.title_from_filename {
        updated.title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    Ok(updated)
}

pub fn save(
    tracks: Vec<Track>,
    mut patch: Patch,
    destination: Destination,
    cache: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Vec<Saved> {
    let prepared = (|| {
        patch.validate(&tracks).map_err(String::from)?;
        let cover = patch
            .0
            .get(&Field::Cover)
            .filter(|s| !s.is_empty())
            .map(|p| picture(p))
            .transpose()?;
        if let Some(cover) = &cover {
            patch.0.insert(Field::Cover, cache_picture(cover, cache)?);
        }
        Ok::<_, String>(cover)
    })();
    let total = tracks.len();
    tracks
        .into_iter()
        .enumerate()
        .map(|(index, track)| {
            let updated = (|| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Cancelled".into());
                }
                let cover = prepared.as_ref().map_err(Clone::clone)?;
                if track.kind != crate::model::Kind::Music
                    || library::local_path(&track.uri).is_none()
                {
                    return Err("Only local music files can be tagged.".into());
                }
                let mut updated = match destination {
                    Destination::Files => {
                        write_file(&track, &patch, cover.as_ref(), cache, cancel)?
                    }
                    Destination::Library => {
                        let mut updated = track.clone();
                        updated.tag_overrides.0.extend(patch.0.clone());
                        updated.tag_overrides.clone().apply(&mut updated);
                        updated
                    }
                    Destination::Reload => {
                        library::probe(&library::local_path(&track.uri).unwrap(), cache)?
                    }
                };
                if patch.changes_identity() {
                    updated.music_info = None;
                }
                updated.id.clone_from(&track.id);
                updated.uri.clone_from(&track.uri);
                updated.tag_revision = track.tag_revision + 1;
                Ok(updated)
            })();
            progress(index + 1, total);
            Saved {
                original: track,
                updated,
            }
        })
        .collect()
}
