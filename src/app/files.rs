use super::*;

type TrashResults = Vec<(PathBuf, Result<(), String>)>;

#[derive(Default)]
pub(super) struct Controls {
    pub watcher: crate::watch::Watcher,
    revision: Cell<u64>,
    roots: RefCell<Vec<PathBuf>>,
    enabled: Cell<bool>,
    pub importing: Cell<bool>,
    pending: RefCell<Vec<PathBuf>>,
    pub scan_pending: Cell<bool>,
    pub scan_manual: Cell<bool>,
    pub unavailable: RefCell<Vec<PathBuf>>,
    pub deleting: Cell<bool>,
    trash_worker: RefCell<Option<std::thread::JoinHandle<TrashResults>>>,
    restoring: RefCell<HashSet<String>>,
}

impl App {
    pub(super) fn setup_files(self: &Rc<Self>) {
        self.string_action("open-location", |app, id| app.open_location(id));
        self.string_action("remove-local", |app, id| {
            app.remove_local_tracks(&[id.to_string()])
        });
        self.string_action("trash-local", |app, id| {
            app.trash_local_tracks(vec![id.to_string()])
        });
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let weak = Rc::downgrade(self);
        target.connect_drop(move |_, value, _, _| {
            let Some(app) = weak.upgrade() else {
                return false;
            };
            object::<gtk::Box>(&app.ui, "content").remove_css_class("drop-library");
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            let paths: Vec<_> = files.files().iter().filter_map(|f| f.path()).collect();
            if paths.is_empty() {
                app.notice(&app.tr("Drop local audio files or folders."), true, false);
                return false;
            }
            app.navigate("library");
            app.import_local(paths);
            true
        });
        let weak = Rc::downgrade(self);
        target.connect_enter(move |_, _, _| {
            if let Some(app) = weak.upgrade() {
                object::<gtk::Box>(&app.ui, "content").add_css_class("drop-library");
            }
            gdk::DragAction::COPY
        });
        let weak = Rc::downgrade(self);
        target.connect_leave(move |_| {
            if let Some(app) = weak.upgrade() {
                object::<gtk::Box>(&app.ui, "content").remove_css_class("drop-library");
            }
        });
        self.window.add_controller(target);
        self.sync_folder_watch();
    }

    pub(super) fn import_local(self: &Rc<Self>, paths: Vec<PathBuf>) {
        let mut pending = self.files.pending.borrow_mut();
        for path in paths {
            if !pending.contains(&path) {
                pending.push(path);
            }
        }
        drop(pending);
        self.poll_files();
    }

    pub(super) fn sync_folder_watch(&self) {
        let state = self.state.borrow();
        if *self.files.roots.borrow() != state.folders
            || self.files.enabled.get() != state.watch_music
        {
            *self.files.roots.borrow_mut() = state.folders.clone();
            self.files.enabled.set(state.watch_music);
            let revision = self.files.revision.get() + 1;
            self.files.revision.set(revision);
            self.files.watcher.configure(
                revision,
                if state.watch_music {
                    state.folders.clone()
                } else {
                    Vec::new()
                },
            );
        }
    }

    pub(super) fn poll_files(self: &Rc<Self>) {
        if self
            .files
            .trash_worker
            .borrow()
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            self.finish_trash();
        }
        self.sync_folder_watch();
        for event in self.files.watcher.poll() {
            match event {
                crate::watch::Event::Ready(version) | crate::watch::Event::Changed(version)
                    if version == self.files.revision.get() && self.files.enabled.get() =>
                {
                    self.files.scan_pending.set(true)
                }
                crate::watch::Event::Error(version, message)
                    if version == self.files.revision.get() =>
                {
                    self.notice(
                        &format!(
                            "{} {message}",
                            self.tr("Could not watch a music folder. Use Refresh to update it.")
                        ),
                        true,
                        false,
                    )
                }
                _ => {}
            }
        }
        if self.scanning.get() || self.files.importing.get() || self.tags.busy.get() {
            return;
        }
        let paths = std::mem::take(&mut *self.files.pending.borrow_mut());
        if !paths.is_empty() {
            self.files.importing.set(true);
            self.notice(&self.tr("Finding your music…"), false, true);
            let existing = self.state.borrow().tracks.clone();
            *self.files.restoring.borrow_mut() =
                self.state.borrow().ignored_local.iter().cloned().collect();
            let cache = self.cache.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Message::LocalImport(library::import_paths(
                    &paths, &existing, &cache,
                )));
            });
        } else if self.files.scan_pending.replace(false) {
            let background = !self.files.scan_manual.replace(false);
            let roots = self.state.borrow().folders.clone();
            if !roots.is_empty() {
                self.scan_folders(roots, background);
            }
        }
    }

    pub(super) fn imported_local(&self, result: Result<library::Import, String>) {
        self.files.importing.set(false);
        let restoring = std::mem::take(&mut *self.files.restoring.borrow_mut());
        match result {
            Ok(import) => {
                let count = import.tracks.len();
                let mut state = self.state.borrow_mut();
                state.ignored_local.retain(|uri| {
                    !restoring.contains(uri) || !import.tracks.iter().any(|t| &t.uri == uri)
                });
                state.merge(import.tracks);
                for root in import.folders {
                    if !state.folders.contains(&root) {
                        state.folders.push(root);
                    }
                }
                drop(state);
                self.sync_folder_watch();
                self.refresh();
                self.update_player();
                self.persist();
                self.notice(
                    &format!(
                        "{count} {} · {} {}",
                        self.tr("tracks found"),
                        import.skipped,
                        self.tr("files skipped")
                    ),
                    false,
                    false,
                );
            }
            Err(error) => self.notice(&error, true, false),
        }
    }

    pub(super) fn remove_local_tracks(&self, ids: &[String]) {
        if self.tags.busy.get() {
            self.notice(&self.tr("Wait for tag changes to finish."), false, false);
            return;
        }
        let ids: Vec<_> =
            ids.iter()
                .filter(|id| {
                    self.state.borrow().track(id).is_some_and(|t| {
                        t.kind == Kind::Music && library::local_path(&t.uri).is_some()
                    })
                })
                .cloned()
                .collect();
        if ids.is_empty() {
            return;
        }
        if self
            .state
            .borrow()
            .current
            .as_ref()
            .is_some_and(|id| ids.contains(id))
        {
            self.stop_playback();
        }
        self.state.borrow_mut().remove_from_library(&ids);
        self.audio.borrow_mut().prepare_next(None, 0.0);
        *self.preparation.borrow_mut() = Default::default();
        self.refresh();
        self.update_player();
        self.persist();
        self.notice(&self.tr("Removed from the library. Your files are unchanged. Import them again to restore them."), false, false);
    }

    pub(super) fn trash_local_tracks(&self, ids: Vec<String>) {
        if self.tags.busy.get() {
            self.notice(&self.tr("Wait for tag changes to finish."), false, false);
            return;
        }
        if self.files.deleting.get() {
            return;
        }
        let paths: HashSet<_> = ids
            .iter()
            .filter_map(|id| {
                self.state
                    .borrow()
                    .track(id)
                    .filter(|t| t.kind == Kind::Music)
                    .and_then(|t| library::local_path(&t.uri))
            })
            .collect();
        if paths.is_empty() {
            return;
        }
        self.files.deleting.set(true);
        self.update_selection();
        self.notice(&self.tr("Moving files to Trash…"), false, true);
        *self.files.trash_worker.borrow_mut() = Some(std::thread::spawn(move || {
            paths
                .into_iter()
                .map(|path| {
                    let result = gio::File::for_path(&path).trash(None::<&gio::Cancellable>);
                    let result = match result {
                        Err(error) if error.matches(gio::IOErrorEnum::NotFound) => Ok(()),
                        result => result.map_err(|e| e.to_string()),
                    };
                    (path, result)
                })
                .collect()
        }));
    }

    pub(super) fn finish_trash(&self) {
        let worker = self.files.trash_worker.borrow_mut().take();
        let Some(worker) = worker else {
            return;
        };
        self.files.deleting.set(false);
        match worker.join() {
            Ok(results) => self.local_trashed(results),
            Err(_) => self.notice(&self.tr("Could not move files to Trash."), true, false),
        }
        self.update_selection();
    }

    pub(super) fn local_trashed(&self, results: TrashResults) {
        let paths: HashSet<_> = results
            .iter()
            .filter(|(_, result)| result.is_ok())
            .map(|(path, _)| path.clone())
            .collect();
        let uses_path = |track: &Track| {
            library::local_path(&track.uri).is_some_and(|p| paths.contains(&p))
                || track
                    .download_path
                    .as_ref()
                    .is_some_and(|p| paths.contains(p))
        };
        if self.state.borrow().current_track().is_some_and(uses_path) {
            self.stop_playback();
        }
        let mut state = self.state.borrow_mut();
        let ids: Vec<_> = state
            .tracks
            .iter()
            .filter(|t| library::local_path(&t.uri).is_some_and(|p| paths.contains(&p)))
            .map(|t| t.id.clone())
            .collect();
        for path in &paths {
            if let Ok(uri) = url::Url::from_file_path(path) {
                if !state.ignored_local.contains(&uri.to_string()) {
                    state.ignored_local.push(uri.to_string());
                }
            }
        }
        state.forget_many(ids.iter().map(String::as_str));
        for track in &mut state.tracks {
            if track
                .download_path
                .as_ref()
                .is_some_and(|p| paths.contains(p))
            {
                track.download_path = None;
            }
        }
        for saved in state.saved_audio.iter().filter(|s| paths.contains(&s.path)) {
            if let Ok(id) = saved.id.parse() {
                let _ = self.downloads.manager.borrow_mut().remove(id);
            }
        }
        state.saved_audio.retain(|s| !paths.contains(&s.path));
        drop(state);
        self.audio.borrow_mut().prepare_next(None, 0.0);
        *self.preparation.borrow_mut() = Default::default();
        self.refresh();
        self.update_player();
        self.persist();
        let failures: Vec<_> = results
            .iter()
            .filter_map(|(path, result)| {
                result
                    .as_ref()
                    .err()
                    .map(|e| format!("{}: {e}", path.display()))
            })
            .take(3)
            .collect();
        if failures.is_empty() {
            self.notice(
                &self.tr("Files moved to Trash and removed from the library."),
                false,
                false,
            );
        } else {
            self.notice(
                &format!(
                    "{} {}",
                    self.tr(
                        "Some files could not be moved to Trash. Their library entries were kept."
                    ),
                    failures.join("\n")
                ),
                true,
                false,
            );
        }
    }

    pub(super) fn file_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        let watch: gtk::Switch = object(ui, "watch_music");
        watch.set_active(self.state.borrow().watch_music);
        let weak = Rc::downgrade(self);
        watch.connect_active_notify(move |switch| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().watch_music = switch.is_active();
                app.sync_folder_watch();
                app.persist();
            }
        });
    }

    fn open_location(self: &Rc<Self>, id: &str) {
        let path = self.state.borrow().track(id).and_then(|t| {
            t.download_path
                .clone()
                .or_else(|| library::local_path(&t.uri))
        });
        let Some(path) = path else { return };
        self.show_saved_file(&path);
    }

    pub(super) fn show_saved_file(self: &Rc<Self>, path: &std::path::Path) {
        let uri = gio::File::for_path(path).uri().to_string();
        let Some(parent) = path.parent() else { return };
        let parent = gio::File::for_path(parent).uri().to_string();
        let weak = Rc::downgrade(self);
        gio::bus_get(
            gio::BusType::Session,
            None::<&gio::Cancellable>,
            move |connection| {
                let Some(app) = weak.upgrade() else { return };
                let Ok(connection) = connection else {
                    app.open_directory(&parent);
                    return;
                };
                let weak = Rc::downgrade(&app);
                connection.call(
                    Some("org.freedesktop.FileManager1"),
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1",
                    "ShowItems",
                    Some(&(vec![uri], "").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    4000,
                    None::<&gio::Cancellable>,
                    move |result| {
                        if result.is_err() {
                            if let Some(app) = weak.upgrade() {
                                app.open_directory(&parent);
                            }
                        }
                    },
                );
            },
        );
    }

    fn open_directory(self: &Rc<Self>, uri: &str) {
        let weak = Rc::downgrade(self);
        gio::AppInfo::launch_default_for_uri_async(
            uri,
            None::<&gio::AppLaunchContext>,
            None::<&gio::Cancellable>,
            move |result| {
                if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                    app.notice(
                        &format!("{} {error}", app.tr("Could not open the file manager.")),
                        true,
                        false,
                    );
                }
            },
        );
    }
}
