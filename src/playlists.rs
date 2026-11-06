use crate::{
    library,
    model::{Kind, Track},
    services,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
};

const MAX_BYTES: u64 = 4_000_000;
const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Default)]
pub struct Entry {
    pub location: String,
    pub title: String,
    pub duration: f64,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub title: String,
    pub entries: Vec<Entry>,
}

pub struct Import {
    pub name: String,
    pub tracks: Vec<Track>,
    pub skipped: usize,
}

fn duration(value: &str) -> f64 {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(0.0)
}

pub fn parse(text: &str, extension: &str) -> Result<Parsed, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut parsed = Parsed::default();
    match extension.to_ascii_lowercase().as_str() {
        "m3u" | "m3u8" => {
            let mut pending = Entry::default();
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if let Some(title) = line.strip_prefix("#PLAYLIST:") {
                    parsed.title = title.trim().into();
                } else if let Some(info) = line.strip_prefix("#EXTINF:") {
                    let (duration, title) = info.split_once(',').unwrap_or((info, ""));
                    pending.title = title.trim().into();
                    pending.duration = self::duration(duration);
                } else if !line.starts_with('#') {
                    pending.location = line.into();
                    parsed.entries.push(std::mem::take(&mut pending));
                }
            }
        }
        "pls" => {
            let mut entries: BTreeMap<usize, Entry> = BTreeMap::new();
            for line in text.lines().map(str::trim) {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let key = key.trim().to_ascii_lowercase();
                let field = ["file", "title", "length"].into_iter().find_map(|prefix| {
                    key.strip_prefix(prefix)
                        .and_then(|n| n.parse::<usize>().ok())
                        .map(|n| (prefix, n))
                });
                if let Some((field, number)) = field {
                    let entry = entries.entry(number).or_default();
                    match field {
                        "file" => entry.location = value.trim().into(),
                        "title" => entry.title = value.trim().into(),
                        _ => entry.duration = duration(value.trim()),
                    }
                }
            }
            parsed.entries = entries
                .into_values()
                .filter(|e| !e.location.is_empty())
                .collect();
        }
        "xspf" => {
            let doc = roxmltree::Document::parse(text)
                .map_err(|e| format!("Invalid XSPF playlist: {e}"))?;
            let playlist = doc.root_element();
            if playlist.tag_name().name() != "playlist" {
                return Err("This file is not an XSPF playlist".into());
            }
            let child = |node: roxmltree::Node<'_, '_>, name: &str| {
                node.children()
                    .find(|n| n.is_element() && n.tag_name().name() == name)
                    .and_then(|n| n.text())
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            parsed.title = child(playlist, "title");
            for track in playlist
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "track")
            {
                let location = child(track, "location");
                if !location.is_empty() {
                    parsed.entries.push(Entry {
                        location,
                        title: child(track, "title"),
                        duration: duration(&child(track, "duration")) / 1000.0,
                    });
                }
            }
        }
        _ => return Err("Choose an M3U, M3U8, PLS, or XSPF playlist".into()),
    }
    if parsed.entries.len() > MAX_ENTRIES {
        return Err("A playlist may contain up to 10,000 entries".into());
    }
    if parsed.entries.is_empty() {
        return Err("This playlist contains no tracks".into());
    }
    Ok(parsed)
}

pub fn import(path: &Path, existing: &[Track], cache: &Path) -> Result<Import, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Playlist files must be smaller than 4 MB".into());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| "Save the playlist as UTF-8 before importing it")?;
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let parsed = parse(&text, extension)?;
    let name = if parsed.title.is_empty() {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        parsed.title
    };
    let base = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let base = url::Url::from_file_path(base).map_err(|_| "Invalid playlist path")?;
    let known: HashMap<&str, &Track> = existing.iter().map(|t| (t.uri.as_str(), t)).collect();
    let mut imported = Import {
        name,
        tracks: vec![],
        skipped: 0,
    };
    let mut seen = HashSet::new();
    for entry in parsed.entries {
        let track = (|| -> Result<Track, String> {
            let location = entry.location.trim().trim_matches('"');
            let uri = if Path::new(location).is_absolute() {
                url::Url::from_file_path(location).map_err(|_| "Invalid path")?
            } else if extension.eq_ignore_ascii_case("xspf") || url::Url::parse(location).is_ok() {
                base.join(location).map_err(|e| e.to_string())?
            } else {
                let parent = path.parent().unwrap_or(Path::new("."));
                if let Ok(local) = fs::canonicalize(parent.join(location)) {
                    url::Url::from_file_path(local).map_err(|_| "Invalid path")?
                } else {
                    // Some players URI-encode relative paths, even in M3U/PLS files.
                    base.join(location).map_err(|e| e.to_string())?
                }
            };
            if uri.scheme() == "file" {
                let local = uri.to_file_path().map_err(|_| "Invalid file URL")?;
                let local = fs::canonicalize(local).map_err(|e| e.to_string())?;
                if !local.is_file() || !library::supported(&local) {
                    return Err("Unsupported audio file".into());
                }
                let canonical = url::Url::from_file_path(&local).map_err(|_| "Invalid path")?;
                if let Some(track) = known.get(canonical.as_str()) {
                    return Ok((*track).clone());
                }
                library::probe(&local, cache)
            } else {
                let uri = services::http_url(uri.as_str())?;
                if let Some(track) = known.get(uri.as_str()) {
                    return Ok((*track).clone());
                }
                let title = if entry.title.is_empty() {
                    uri.host_str().unwrap_or("Stream")
                } else {
                    &entry.title
                };
                let mut track = Track::new(uri.as_str(), title, Kind::Stream);
                track.duration = entry.duration;
                Ok(track)
            }
        })();
        match track {
            Ok(track) if seen.insert(track.id.clone()) => imported.tracks.push(track),
            _ => imported.skipped += 1,
        }
    }
    if imported.tracks.is_empty() {
        return Err(
            "No playable entries were found. Check the file paths and stream addresses.".into(),
        );
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bom_m3u_and_extended_titles() {
        let p = parse("\u{feff}#EXTM3U\r\n#PLAYLIST:Evening\r\n#EXTINF:12,First\r\nMusic/song.flac\r\n#EXTINF:-1,Radio\nhttps://example.org/live\n", "M3U8").unwrap();
        assert_eq!(p.title, "Evening");
        assert_eq!(p.entries[0].title, "First");
        assert_eq!(p.entries[1].duration, 0.0);
        for value in ["NaN", "inf", "-1", "unknown"] {
            let p = parse(
                &format!("#EXTINF:{value},Radio\nhttps://example.org/live"),
                "m3u",
            )
            .unwrap();
            assert_eq!(p.entries[0].duration, 0.0);
        }
    }
    #[test]
    fn pls_uses_numeric_order() {
        let p = parse("[playlist]\nFile10=https://example.org/ten\nTitle10=Tenth\nFile2=https://example.org/two\nLength2=42\n", "pls").unwrap();
        assert!(p.entries[0].location.ends_with("two"));
        assert_eq!(p.entries[1].title, "Tenth");
    }
    #[test]
    fn xspf_namespaces_and_milliseconds() {
        let p = parse(r#"<playlist xmlns="http://xspf.org/ns/0/"><title>Mix</title><trackList><track><location>song%20one.flac</location><duration>12300</duration></track></trackList></playlist>"#, "xspf").unwrap();
        assert_eq!(p.title, "Mix");
        assert_eq!(p.entries[0].duration, 12.3);
        assert!(parse("<html/>", "xspf").is_err());
    }
    #[test]
    fn imports_relative_paths_streams_and_skips_missing_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("song one.flac");
        fs::write(&audio, b"fixture indexed already").unwrap();
        let uri = url::Url::from_file_path(&audio).unwrap();
        let known = Track::new(uri.as_str(), "Existing title", Kind::Music);
        let file = dir.path().join("mix.m3u");
        fs::write(&file, "song%20one.flac\nmissing.flac\nsong one.flac\n#EXTINF:-1,Radio\nhttps://example.org/live\nfile:///does-not-exist.flac\njavascript:alert(1)\n").unwrap();
        let result = import(&file, &[known], dir.path()).unwrap();
        assert_eq!(result.tracks.len(), 2);
        assert_eq!(result.skipped, 4);
        assert_eq!(result.tracks[0].title, "Existing title");
        assert_eq!(result.tracks[1].kind, Kind::Stream);
    }
}
