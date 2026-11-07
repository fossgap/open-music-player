use crate::{
    model::{Kind, Track},
    process,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, UNIX_EPOCH},
};

pub struct Scan {
    pub tracks: Vec<Track>,
    pub skipped: usize,
    pub removed: Vec<String>,
    pub unavailable: Vec<PathBuf>,
}

pub fn local_path(uri: &str) -> Option<PathBuf> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok().filter(|p| p.is_absolute())
}

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_lowercase().as_str(),
                "mp3"
                    | "flac"
                    | "ogg"
                    | "opus"
                    | "wav"
                    | "m4a"
                    | "aac"
                    | "aiff"
                    | "aif"
                    | "wma"
                    | "ape"
                    | "alac"
                    | "mka"
                    | "mp4"
            )
        })
}

pub fn scan(folders: &[PathBuf], existing: &[Track], cache: &Path) -> Result<Scan, String> {
    scan_filtered(folders, existing, cache, &[])
}

pub fn scan_filtered(
    folders: &[PathBuf],
    existing: &[Track],
    cache: &Path,
    ignored: &[String],
) -> Result<Scan, String> {
    let ignored: std::collections::HashSet<_> = ignored.iter().map(String::as_str).collect();
    process::output(
        Command::new("ffprobe").arg("-version"),
        Duration::from_secs(5),
        128_000,
    )
    .map_err(|_| "Install FFmpeg (ffprobe) to read your music library.".to_string())?;
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let old: HashMap<&str, &Track> = existing.iter().map(|t| (t.uri.as_str(), t)).collect();
    let mut result = Scan {
        tracks: vec![],
        skipped: 0,
        removed: vec![],
        unavailable: vec![],
    };
    let mut found = std::collections::HashSet::new();
    for folder in folders {
        if !folder.is_dir() {
            result.unavailable.push(folder.clone());
            continue;
        }
        for entry in walkdir::WalkDir::new(folder).follow_links(false) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    result.skipped += 1;
                    continue;
                }
            };
            if !entry.file_type().is_file() || !supported(entry.path()) {
                continue;
            }
            let path = match fs::canonicalize(entry.path()) {
                Ok(p) => p,
                Err(_) => {
                    result.skipped += 1;
                    continue;
                }
            };
            let Ok(uri) = url::Url::from_file_path(&path) else {
                continue;
            };
            if !found.insert(uri.to_string()) {
                continue;
            }
            if ignored.contains(uri.as_str()) {
                continue;
            }
            let Ok(meta) = path.metadata() else {
                result.skipped += 1;
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|m| m.as_nanos().min(u128::from(u64::MAX)) as u64)
                .unwrap_or(0);
            if let Some(t) = old.get(uri.as_str()).filter(|t| {
                t.modified == modified
                    && t.size == meta.len()
                    && t.audio_details.codec.is_some()
                    && t.metadata_version == 1
            }) {
                let mut cached = (*t).clone();
                refresh_sidecars(&path, &mut cached);
                result.tracks.push(cached);
                continue;
            }
            match probe(&path, cache) {
                Ok(mut t) => {
                    t.modified = modified;
                    t.size = meta.len();
                    result.tracks.push(t);
                }
                Err(_) => result.skipped += 1,
            }
        }
    }
    result.tracks.sort_by(|a, b| {
        (&a.artist, &a.album, a.number, &a.title).cmp(&(&b.artist, &b.album, b.number, &b.title))
    });
    // Check removals on the worker, only inside roots that remain available.
    // A failed probe or permission error never means the file was deleted.
    result.removed = existing
        .iter()
        .filter(|t| t.kind == Kind::Music && !found.contains(&t.uri))
        .filter(|t| {
            local_path(&t.uri).is_some_and(|p| {
                folders
                    .iter()
                    .any(|f| !result.unavailable.contains(f) && f.is_dir() && p.starts_with(f))
                    && fs::metadata(&p).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            })
        })
        .map(|t| t.id.clone())
        .collect();
    Ok(result)
}

pub struct Import {
    pub tracks: Vec<Track>,
    pub folders: Vec<PathBuf>,
    pub skipped: usize,
}

pub fn import_paths(paths: &[PathBuf], existing: &[Track], cache: &Path) -> Result<Import, String> {
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut skipped = 0;
    for path in paths {
        let Ok(path) = fs::canonicalize(path) else {
            skipped += 1;
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        if path.is_dir() {
            folders.push(path);
        } else if path.is_file() && supported(&path) {
            files.push(path);
        } else {
            skipped += 1;
        }
    }
    let mut scan = scan(&folders, existing, cache)?;
    for path in files {
        if folders.iter().any(|f| path.starts_with(f)) {
            continue;
        }
        match probe(&path, cache) {
            Ok(track) => scan.tracks.push(track),
            Err(_) => skipped += 1,
        }
    }
    Ok(Import {
        tracks: scan.tracks,
        folders,
        skipped: skipped + scan.skipped,
    })
}

fn probe_data(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = process::output(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_format",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(path),
        Duration::from_secs(12),
        2_000_000,
    )?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub fn audio_details(path: &Path) -> Result<crate::audio_info::Details, String> {
    Ok(crate::audio_info::Details::from_probe(&probe_data(path)?))
}

pub fn probe(path: &Path, cache: &Path) -> Result<Track, String> {
    let data = probe_data(path)?;
    let streams = data["streams"].as_array().ok_or("No audio stream")?;
    if !streams.iter().any(|s| s["codec_type"] == "audio") {
        return Err("No audio stream".into());
    }
    let mut tags = HashMap::new();
    for source in std::iter::once(&data["format"]["tags"]).chain(streams.iter().map(|s| &s["tags"]))
    {
        if let Some(object) = source.as_object() {
            for (key, value) in object {
                if let Some(value) = value.as_str() {
                    tags.entry(key.to_lowercase()).or_insert(value.to_string());
                }
            }
        }
    }
    let uri = url::Url::from_file_path(path)
        .map_err(|_| "Invalid file path")?
        .to_string();
    let fallback = path.file_stem().unwrap_or_default().to_string_lossy();
    let mut t = Track::new(
        &uri,
        tags.get("title").map(String::as_str).unwrap_or(&fallback),
        Kind::Music,
    );
    t.audio_details = crate::audio_info::Details::from_probe(&data);
    t.metadata_version = 1;
    t.title_from_filename = tags.get("title").is_none_or(|s| s.trim().is_empty());
    t.artist = tags.get("artist").cloned().unwrap_or_default();
    t.album = tags.get("album").cloned().unwrap_or_default();
    t.album_artist = tags
        .get("album_artist")
        .or_else(|| tags.get("albumartist"))
        .cloned()
        .unwrap_or_default();
    t.musicbrainz_release_id = tags
        .get("musicbrainz_albumid")
        .or_else(|| tags.get("musicbrainz album id"))
        .filter(|id| crate::enrichment::valid_mbid(id))
        .cloned();
    t.musicbrainz_recording_id = tags
        .get("musicbrainz_trackid")
        .or_else(|| tags.get("musicbrainz track id"))
        .filter(|id| crate::enrichment::valid_mbid(id))
        .cloned();
    t.musicbrainz_artist_id = tags
        .get("musicbrainz_artistid")
        .or_else(|| tags.get("musicbrainz artist id"))
        .filter(|id| crate::enrichment::valid_mbid(id))
        .cloned();
    t.duration = data["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    t.number = tags
        .get("track")
        .and_then(|n| n.split('/').next()?.parse().ok())
        .unwrap_or(0);
    t.source = path
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .into();
    t.lyrics = tags
        .get("lyrics")
        .or_else(|| tags.get("unsyncedlyrics"))
        .cloned();
    refresh_sidecars(path, &mut t);
    if t.artwork.is_none()
        && streams
            .iter()
            .any(|s| s["disposition"]["attached_pic"] == 1)
    {
        let cover = cache.join(format!("{}.jpg", t.id));
        if process::output(
            Command::new("ffmpeg")
                .args(["-v", "error", "-y", "-i"])
                .arg(path)
                .args([
                    "-map",
                    "0:v:0",
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=600:600:force_original_aspect_ratio=decrease",
                ])
                .arg(&cover),
            Duration::from_secs(15),
            128_000,
        )
        .is_ok()
        {
            t.artwork = Some(cover.to_string_lossy().into());
        }
    }
    Ok(t)
}

fn refresh_sidecars(path: &Path, t: &mut Track) {
    for ext in ["lrc", "txt"] {
        let sidecar = path.with_extension(ext);
        if sidecar.metadata().is_ok_and(|m| m.len() < 512_000) {
            if let Ok(text) = fs::read_to_string(sidecar) {
                t.lyrics = Some(text);
                t.synced_lyrics = None;
                t.lyrics_timing_checked = true;
                t.lyrics_source = None;
                t.instrumental = false;
                break;
            }
        }
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    for name in [
        "cover.jpg",
        "cover.png",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "Cover.jpg",
        "Folder.jpg",
    ] {
        let image = parent.join(name);
        if image.is_file() {
            t.artwork = Some(image.to_string_lossy().into());
            t.artwork_source = None;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wav(path: &Path) {
        let size = 8000u32;
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((size + 36).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(8000u32.to_le_bytes());
        bytes.extend(16000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(size.to_le_bytes());
        bytes.resize(size as usize + 44, 0);
        fs::write(path, bytes).unwrap();
    }
    #[test]
    fn file_import_does_not_import_siblings_and_folder_import_is_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let picked = dir.path().join("picked song.wav");
        wav(&picked);
        wav(&dir.path().join("not selected.wav"));
        let album = dir.path().join("album");
        fs::create_dir_all(album.join("disc 2")).unwrap();
        wav(&album.join("one.wav"));
        wav(&album.join("disc 2/two.wav"));
        fs::write(dir.path().join("notes.txt"), "not audio").unwrap();
        let result = import_paths(
            &[
                picked.clone(),
                picked.clone(),
                album.clone(),
                dir.path().join("notes.txt"),
            ],
            &[],
            &cache,
        )
        .unwrap();
        assert_eq!(result.tracks.len(), 3);
        assert_eq!(result.folders, [album]);
        assert_eq!(result.skipped, 1);
        assert!(
            result
                .tracks
                .iter()
                .any(|t| local_path(&t.uri).as_ref() == Some(&picked))
        );
        assert!(
            result
                .tracks
                .iter()
                .all(|t| !t.uri.contains("selected.wav"))
        );
        assert!(local_path("https://example.test/song.wav").is_none());
    }
    #[test]
    fn scans_detect_metadata_edits_and_only_prune_available_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("music");
        let offline = dir.path().join("offline");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&offline).unwrap();
        let path = root.join("song.wav");
        let away = offline.join("keep.wav");
        wav(&path);
        wav(&away);
        let cache = dir.path().join("cache");
        let folders = vec![root.clone(), offline.clone()];
        let original = scan(&folders, &[], &cache).unwrap();
        assert_eq!(original.tracks.len(), 2);
        let edited = root.join("edited.wav");
        assert!(
            Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&path)
                .args(["-metadata", "title=Changed title"])
                .arg(&edited)
                .status()
                .unwrap()
                .success()
        );
        fs::rename(&edited, &path).unwrap();
        fs::remove_dir_all(&offline).unwrap();
        let updated = scan(&folders, &original.tracks, &cache).unwrap();
        assert_eq!(updated.tracks.len(), 1);
        assert_eq!(updated.tracks[0].title, "Changed title");
        assert!(updated.removed.is_empty());
        assert_eq!(updated.unavailable, [offline]);
        fs::remove_file(&path).unwrap();
        let removed = scan(&folders, &original.tracks, &cache).unwrap();
        assert_eq!(removed.removed, [model_id(&path)]);
        assert!(!removed.removed.contains(&model_id(&away)));
    }
    fn model_id(path: &Path) -> String {
        crate::model::stable_id(url::Url::from_file_path(path).unwrap().as_str())
    }
    #[test]
    fn new_sidecars_replace_downloaded_metadata_during_cached_scans() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("song.flac");
        let mut track = Track::new("file:///song.flac", "Song", Kind::Music);
        track.lyrics_source = Some("https://lrclib.net/api/get/1".into());
        track.instrumental = true;
        track.artwork = Some("downloaded.png".into());
        track.artwork_source = Some("https://musicbrainz.org/".into());
        fs::write(path.with_extension("lrc"), "[00:01]A local fixture line").unwrap();
        fs::write(directory.path().join("cover.png"), b"local cover fixture").unwrap();
        refresh_sidecars(&path, &mut track);
        assert_eq!(track.lyrics.as_deref(), Some("[00:01]A local fixture line"));
        assert!(track.lyrics_source.is_none());
        assert!(!track.instrumental);
        assert!(track.artwork.as_deref().unwrap().ends_with("cover.png"));
        assert!(track.artwork_source.is_none());
    }

    #[test]
    fn recognizes_formats_without_case_sensitivity() {
        assert!(supported(Path::new("music.FLAC")));
        assert!(supported(Path::new("episode.opus")));
        assert!(!supported(Path::new("cover.jpg")));
        assert!(!supported(Path::new("music")));
    }
}
