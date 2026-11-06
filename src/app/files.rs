use super::*;

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
}

impl App {
    pub(super) fn setup_files(self: &Rc<Self>) {
        self.string_action("open-location", |app, id| app.open_location(id));
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
        if self.scanning.get() || self.files.importing.get() {
            return;
        }
        let paths = std::mem::take(&mut *self.files.pending.borrow_mut());
        if !paths.is_empty() {
            self.files.importing.set(true);
            self.notice(&self.tr("Finding your music…"), false, true);
            let existing = self.state.borrow().tracks.clone();
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
        match result {
            Ok(import) => {
                let count = import.tracks.len();
                let mut state = self.state.borrow_mut();
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
        let path = self
            .state
            .borrow()
            .track(id)
            .and_then(|t| library::local_path(&t.uri));
        let Some(path) = path else { return };
        let uri = gio::File::for_path(&path).uri().to_string();
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
