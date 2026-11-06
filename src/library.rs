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
    pub seen: Vec<String>,
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
        seen: vec![],
    };
    let mut found = std::collections::HashSet::new();
    for folder in folders {
        if !folder.is_dir() {
            return Err(format!("Folder unavailable: {}", folder.display()));
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
            result.seen.push(uri.to_string());
            let Ok(meta) = path.metadata() else {
                result.skipped += 1;
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|m| m.as_secs())
                .unwrap_or(0);
            if let Some(t) = old
                .get(uri.as_str())
                .filter(|t| t.modified == modified && t.size == meta.len())
            {
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
    Ok(result)
}

pub fn probe(path: &Path, cache: &Path) -> Result<Track, String> {
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
    let data: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
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
