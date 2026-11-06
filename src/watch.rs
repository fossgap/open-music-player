//! Recursive native GIO monitoring lives on its own GLib context, away from GTK.
use gtk::{gio, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

enum Command {
    Roots(u64, Vec<PathBuf>),
    Stop,
}
pub enum Event {
    Ready(u64),
    Changed(u64),
    Error(u64, String),
}

pub struct Watcher {
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Event>,
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}
impl Watcher {
    pub fn new() -> Self {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let context = glib::MainContext::new();
            let _ = context.with_thread_default(|| run(&context, commands, events));
        });
        Self { tx, rx }
    }
    pub fn configure(&self, revision: u64, roots: Vec<PathBuf>) {
        let _ = self.tx.send(Command::Roots(revision, roots));
    }
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn relevant(path: &Path, watched: &Path, roots: &[PathBuf]) -> bool {
    if !roots
        .iter()
        .any(|root| path.starts_with(root) || root.starts_with(path))
    {
        return false;
    }
    path == watched
        || path.is_dir()
        || path.extension().is_none()
        || crate::library::supported(path)
        || path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "jpg" | "jpeg" | "png" | "webp" | "lrc" | "txt"
                )
            })
}

type Dirty = Rc<RefCell<Option<(Instant, Instant)>>>;

fn rebuild(
    roots: &[PathBuf],
    monitors: &mut HashMap<PathBuf, gio::FileMonitor>,
    failed: &mut HashSet<PathBuf>,
    dirty: &Dirty,
    revision: u64,
    tx: &mpsc::Sender<Event>,
) {
    let mut directories = HashSet::new();
    for root in roots {
        // A parent watch lets a removed/recreated or temporarily unavailable root recover.
        if let Some(parent) = root.ancestors().skip(1).find(|p| p.is_dir()) {
            directories.insert(parent.to_path_buf());
        }
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_dir() {
                directories.insert(entry.into_path());
            }
        }
    }
    failed.retain(|path| directories.contains(path));
    monitors.retain(|path, monitor| {
        if directories.contains(path) {
            true
        } else {
            monitor.cancel();
            false
        }
    });
    let mut reported = false;
    for path in directories {
        if monitors.contains_key(&path) {
            continue;
        }
        match gio::File::for_path(&path).monitor_directory(
            gio::FileMonitorFlags::WATCH_MOVES,
            None::<&gio::Cancellable>,
        ) {
            Ok(monitor) => {
                failed.remove(&path);
                monitor.set_rate_limit(200);
                let roots = roots.to_vec();
                let watched = path.clone();
                let dirty = dirty.clone();
                monitor.connect_changed(move |_, file, other, _| {
                    if [Some(file), other]
                        .into_iter()
                        .flatten()
                        .filter_map(|f| f.path())
                        .any(|p| relevant(&p, &watched, &roots))
                    {
                        let now = Instant::now();
                        let mut dirty = dirty.borrow_mut();
                        let first = dirty.as_ref().map_or(now, |(first, _)| *first);
                        *dirty = Some((first, now));
                    }
                });
                monitors.insert(path, monitor);
            }
            Err(error) => {
                if failed.insert(path.clone()) && !reported {
                    reported = true;
                    let _ = tx.send(Event::Error(
                        revision,
                        format!("{}: {error}", path.display()),
                    ));
                }
            }
        }
    }
}

fn run(
    context: &glib::MainContext,
    commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<Event>,
) {
    let mut roots = Vec::new();
    let mut revision = 0;
    let mut monitors = HashMap::<PathBuf, gio::FileMonitor>::new();
    let mut failed = HashSet::new();
    let dirty = Rc::new(RefCell::new(None));
    let mut maintenance = Instant::now();
    loop {
        for _ in 0..200 {
            if !context.pending() {
                break;
            }
            context.iteration(false);
        }
        match commands.recv_timeout(Duration::from_millis(75)) {
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Roots(version, paths)) => {
                for monitor in monitors.values() {
                    monitor.cancel();
                }
                monitors.clear();
                failed.clear();
                *dirty.borrow_mut() = None;
                revision = version;
                roots = paths;
                rebuild(
                    &roots,
                    &mut monitors,
                    &mut failed,
                    &dirty,
                    revision,
                    &events,
                );
                maintenance = Instant::now();
                let _ = events.send(Event::Ready(revision));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let changed = dirty.borrow().is_some_and(|(first, last)| {
            last.elapsed() >= Duration::from_millis(650)
                || first.elapsed() >= Duration::from_secs(3)
        });
        if changed {
            *dirty.borrow_mut() = None;
            rebuild(
                &roots,
                &mut monitors,
                &mut failed,
                &dirty,
                revision,
                &events,
            );
            maintenance = Instant::now();
            let _ = events.send(Event::Changed(revision));
        } else if !roots.is_empty() && maintenance.elapsed() >= Duration::from_secs(15) {
            // Retry failed/missing registrations without launching periodic full media scans.
            if !failed.is_empty() || roots.iter().any(|root| !monitors.contains_key(root)) {
                rebuild(
                    &roots,
                    &mut monitors,
                    &mut failed,
                    &dirty,
                    revision,
                    &events,
                );
            }
            maintenance = Instant::now();
        }
    }
    for monitor in monitors.values() {
        monitor.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(watcher: &Watcher, ready: bool, revision: u64) {
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            if watcher.poll().iter().any(|event| {
                matches!(event, Event::Ready(v) if ready && *v == revision)
                    || matches!(event, Event::Changed(v) if !ready && *v == revision)
            }) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("Folder monitor did not deliver the expected event");
    }
    #[test]
    fn recursive_watch_coalesces_changes_and_recovers_recreated_folders() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("music");
        std::fs::create_dir(&root).unwrap();
        let watcher = Watcher::new();
        watcher.configure(1, vec![root.clone()]);
        wait(&watcher, true, 1);
        let album = root.join("new album");
        std::fs::create_dir(&album).unwrap();
        std::fs::write(album.join("song.wav"), b"first").unwrap();
        wait(&watcher, false, 1);
        std::fs::write(album.join("song.wav"), b"changed").unwrap();
        wait(&watcher, false, 1);
        std::fs::remove_file(album.join("song.wav")).unwrap();
        wait(&watcher, false, 1);
        std::fs::remove_dir_all(&root).unwrap();
        wait(&watcher, false, 1);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("again.flac"), b"new").unwrap();
        wait(&watcher, false, 1);
        watcher.configure(2, vec![]);
        wait(&watcher, true, 2);
        std::fs::write(root.join("ignored.wav"), b"new").unwrap();
        std::thread::sleep(Duration::from_millis(900));
        assert!(
            !watcher
                .poll()
                .iter()
                .any(|e| matches!(e, Event::Changed(_)))
        );
    }
}
