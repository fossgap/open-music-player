use crate::model::State;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPEN_MUSIC_DATA_DIR") {
        return dir.into();
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("open-music-player")
}

pub fn load(path: &Path) -> Result<State, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut state: State = serde_json::from_slice(&bytes).map_err(|e| {
                format!(
                    "Cannot read {}: {e}. The original file has been left untouched.",
                    path.display()
                )
            })?;
            if state.version > 1 {
                return Err(
                    "This library was written by a newer version of Open Music Player.".into(),
                );
            }
            state.sanitize();
            Ok(state)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(format!("Cannot open {}: {e}", path.display())),
    }
}

pub fn save(path: &Path, state: &State) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid data directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let mut file = fs::File::create(&temp)?;
        serde_json::to_writer(&mut file, state)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    };
    write().map_err(|e| format!("Could not save your library: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_atomic_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/library.json");
        let mut s = State {
            position: 72.0,
            ..State::default()
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap().position, 72.0);
        s.position = 84.0;
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap().position, 84.0);
        assert!(!path.with_extension("json.tmp").exists());
    }
    #[test]
    fn corrupted_state_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        fs::write(&path, "broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "broken");
    }

    #[test]
    fn window_state_migrates_old_libraries_and_round_trips_normal_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        fs::write(&path, "{}").unwrap();
        let mut state = load(&path).unwrap();
        assert_eq!(state.window, crate::model::WindowState::default());
        state.window = crate::model::WindowState {
            width: 1120,
            height: 720,
            maximized: true,
        };
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap().window, state.window);

        fs::write(
            &path,
            r#"{"window":{"width":0,"height":-1,"maximized":true}}"#,
        )
        .unwrap();
        let restored = load(&path).unwrap();
        assert_eq!((restored.window.width, restored.window.height), (1240, 800));
        assert!(restored.window.maximized);
        fs::write(&path, r#"{"window":{"width":1,"height":2147483647}}"#).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(
            (restored.window.width, restored.window.height),
            (760, 16384)
        );
    }
}
