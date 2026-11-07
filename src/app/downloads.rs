use super::*;
use crate::downloads::{Manager, Mode, Status};

#[derive(Default)]
pub(super) struct Controls {
    pub manager: RefCell<Manager>,
    rows: RefCell<HashMap<String, gtk::Builder>>,
    pending: RefCell<Option<Pending>>,
    deleting: RefCell<HashSet<String>>,
}

#[derive(Clone)]
struct Pending {
    job: u64,
    track: String,
    generation: u64,
    capture: crate::downloads::Capture,
}

enum SaveAction {
    Start(String, Mode),
    Cancel(u64),
    Stop(u64),
    Resume(u64),
    Show,
    Unavailable,
}

impl App {
    pub(super) fn setup_downloads(self: &Rc<Self>) {
        self.string_action("download", |app, id| app.save_source(id, Mode::Download));
        self.string_action("record", |app, id| app.save_source(id, Mode::Record));
        self.string_action("download-resume", |app, id| {
            if let Ok(id) = id.parse() {
                app.resume_download(id);
            }
        });
        self.string_action("download-delete", |app, id| app.delete_download(id));
        self.action("save-current", |app| {
            match app.current_save().0 {
                SaveAction::Start(id, mode) => app.save_source(&id, mode),
                SaveAction::Cancel(id) => app.downloads.manager.borrow_mut().cancel(id),
                SaveAction::Stop(id) => app.downloads.manager.borrow_mut().stop(id),
                SaveAction::Resume(id) => app.resume_download(id),
                SaveAction::Show => app.navigate("downloads"),
                SaveAction::Unavailable => {}
            }
            app.update_save_control();
        });
        self.string_action("download-cancel", |app, id| {
            if let Ok(id) = id.parse() {
                app.downloads.manager.borrow_mut().cancel(id);
            }
            app.render_downloads();
        });
        self.string_action("download-stop", |app, id| {
            if let Ok(id) = id.parse() {
                app.downloads.manager.borrow_mut().stop(id);
            }
        });
        self.string_action("saved-play", |app, id| {
            let saved = app
                .state
                .borrow()
                .saved_audio
                .iter()
                .find(|s| s.id == id)
                .cloned();
            if let Some(saved) = saved {
                let uri = gio::File::for_path(saved.path).uri();
                let track = Track::new(&uri, &saved.title, Kind::Music);
                let id = track.id.clone();
                {
                    let mut state = app.state.borrow_mut();
                    state.ignored_local.retain(|uri| uri != &track.uri);
                    state.merge(vec![track]);
                }
                app.play_track(&id, None);
            }
        });
        self.string_action("saved-location", |app, id| {
            let path = app
                .state
                .borrow()
                .saved_audio
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.path.clone());
            if let Some(path) = path {
                app.show_saved_file(&path);
            }
        });
        self.action("download-settings", |app| app.settings());
    }

    pub(super) fn download_menu(&self, menu: &gio::Menu, track: &Track) {
        let options: &[(&str, &str)] = match track.kind {
            Kind::Podcast | Kind::YoutubeMusic => &[("Download audio", "app.download")],
            Kind::Radio => &[("Record stream", "app.record")],
            Kind::Stream => &[
                ("Download audio", "app.download"),
                ("Record stream", "app.record"),
            ],
            Kind::Spotify => &[("Play and record", "app.record")],
            Kind::Music => &[],
        };
        for (label, action) in options {
            let item = gio::MenuItem::new(Some(&self.tr(label)), None);
            item.set_action_and_target_value(Some(action), Some(&track.id.to_variant()));
            menu.append_item(&item);
        }
    }

    pub(super) fn download_directory(&self) -> PathBuf {
        self.state
            .borrow()
            .download_directory
            .clone()
            .unwrap_or_else(|| {
                glib::user_special_dir(glib::UserDirectory::Downloads)
                    .unwrap_or_else(|| glib::home_dir().join("Downloads"))
                    .join("Open Audio Player")
            })
    }

    fn resume_download(&self, id: u64) {
        let account = self.state.borrow().youtube_account.clone();
        let result = self.downloads.manager.borrow_mut().resume(id, account);
        if let Err(error) = result {
            self.notice(&self.tr(&error), true, false);
        }
        self.render_downloads();
        self.update_save_control();
    }

    fn delete_download(self: &Rc<Self>, id: &str) {
        if self.downloads.deleting.borrow().contains(id) {
            return;
        }
        let saved = self
            .state
            .borrow()
            .saved_audio
            .iter()
            .find(|s| s.id == id)
            .cloned();
        if let Some(saved) = saved {
            self.downloads.deleting.borrow_mut().insert(id.to_string());
            self.render_downloads();
            let weak = Rc::downgrade(self);
            gio::File::for_path(&saved.path).trash_async(
                glib::Priority::DEFAULT,
                None::<&gio::Cancellable>,
                move |result| {
                    let Some(app) = weak.upgrade() else { return };
                    app.finish_download_delete(saved, result);
                },
            );
        } else if let Ok(id) = id.parse() {
            let result = self.downloads.manager.borrow_mut().remove(id);
            if let Err(error) = result {
                self.notice(&self.tr(&error), true, false);
            }
            self.render_downloads();
            self.update_save_control();
        }
    }

    pub(super) fn finish_download_delete(
        &self,
        saved: model::SavedAudio,
        result: Result<(), glib::Error>,
    ) {
        self.downloads.deleting.borrow_mut().remove(&saved.id);
        if let Err(error) = result {
            if !error.matches(gio::IOErrorEnum::NotFound) {
                self.notice(
                    &format!(
                        "{} {error}",
                        self.tr("Could not move the download to Trash.")
                    ),
                    true,
                    false,
                );
                self.render_downloads();
                return;
            }
        }
        let uri = gio::File::for_path(&saved.path).uri();
        let current_uses_file = self
            .state
            .borrow()
            .current_track()
            .is_some_and(|t| t.uri == uri || t.download_path.as_ref() == Some(&saved.path));
        if current_uses_file {
            self.remember_position();
            self.stop_playback();
        }
        {
            let mut state = self.state.borrow_mut();
            state.saved_audio.retain(|s| s.id != saved.id);
            let local: Vec<_> = state
                .tracks
                .iter()
                .filter(|t| t.uri == uri)
                .map(|t| t.id.clone())
                .collect();
            for id in local {
                state.forget(&id);
            }
            for track in &mut state.tracks {
                if track.download_path.as_ref() == Some(&saved.path) {
                    track.download_path = None;
                }
            }
        }
        if let Ok(id) = saved.id.parse() {
            let _ = self.downloads.manager.borrow_mut().remove(id);
        }
        self.persist();
        self.refresh();
        self.update_player();
    }

    fn current_save(&self) -> (SaveAction, &'static str, &'static str) {
        let state = self.state.borrow();
        let Some(track) = state.current_track() else {
            return (
                SaveAction::Unavailable,
                "folder-download-symbolic",
                "Play an online source to download or record audio.",
            );
        };
        if track.kind == Kind::Music {
            return (
                SaveAction::Unavailable,
                "folder-download-symbolic",
                "Local audio is already on disk.",
            );
        }
        let manager = self.downloads.manager.borrow();
        if let Some(job) = manager.jobs.iter().rev().find(|j| {
            j.status.active()
                && (j.track.id == track.id
                    || (j.mode == Mode::Playback && track.kind == Kind::Spotify))
        }) {
            return if job.mode != Mode::Download && job.status == Status::Running {
                (
                    SaveAction::Stop(job.id),
                    "media-playback-stop-symbolic",
                    "Stop and save recording",
                )
            } else {
                (
                    SaveAction::Cancel(job.id),
                    "process-stop-symbolic",
                    if job.mode == Mode::Download {
                        "Cancel download"
                    } else {
                        "Cancel recording"
                    },
                )
            };
        }
        if track.download_path.is_some() {
            return (
                SaveAction::Show,
                "emblem-ok-symbolic",
                "Show saved download",
            );
        }
        if let Some(job) = manager.jobs.iter().rev().find(|j| {
            j.track.id == track.id
                && j.mode == Mode::Download
                && matches!(j.status, Status::Cancelled | Status::Failed(_))
        }) {
            return if job.can_resume() {
                (
                    SaveAction::Resume(job.id),
                    if job.status == Status::Cancelled {
                        "media-playback-start-symbolic"
                    } else {
                        "view-refresh-symbolic"
                    },
                    if job.status == Status::Cancelled {
                        "Resume download"
                    } else {
                        "Retry download"
                    },
                )
            } else {
                (
                    SaveAction::Unavailable,
                    "folder-download-symbolic",
                    "Finishing cancellation…",
                )
            };
        }
        let (mode, icon, label) = match track.kind {
            Kind::Spotify => (
                Mode::Playback,
                "media-record-symbolic",
                if self.playback_active() {
                    "Record playback"
                } else {
                    "Play and record"
                },
            ),
            Kind::Radio => (Mode::Record, "media-record-symbolic", "Record stream"),
            Kind::Stream
                if track.duration <= 0.0
                    && (!self.audio.borrow().loaded || self.audio.borrow().duration() <= 0.0) =>
            {
                (Mode::Record, "media-record-symbolic", "Record stream")
            }
            _ => (Mode::Download, "folder-download-symbolic", "Download audio"),
        };
        (SaveAction::Start(track.id.clone(), mode), icon, label)
    }

    pub(super) fn update_save_control(&self) {
        let (action, icon, label) = self.current_save();
        let button = self.button("playback_download");
        button.set_icon_name(icon);
        button.set_tooltip_text(Some(&self.tr(label)));
        button.set_sensitive(!matches!(action, SaveAction::Unavailable));
    }

    fn save_source(self: &Rc<Self>, id: &str, mode: Mode) {
        let track = self.state.borrow().track(id).cloned();
        let Some(track) = track else { return };
        if track.kind == Kind::Spotify {
            if self.state.borrow().current.as_deref() != Some(id) || !self.audio.borrow().loaded {
                self.play_track(id, None);
            } else if !self.audio.borrow().playing {
                self.toggle_play();
            }
            let result = self.downloads.manager.borrow_mut().start(
                track,
                Mode::Playback,
                self.download_directory(),
                None,
            );
            match result {
                Ok((job, Some(capture))) => {
                    let pending = Pending {
                        job,
                        track: id.to_string(),
                        generation: self.play_generation.get(),
                        capture,
                    };
                    if let Some(previous) = self.downloads.pending.borrow_mut().replace(pending) {
                        self.downloads.manager.borrow_mut().cancel(previous.job);
                    }
                }
                Err(error) => self.notice(&self.tr(&error), true, false),
                _ => {}
            }
        } else {
            self.begin_download(track, mode);
        }
        self.navigate("downloads");
    }

    fn begin_download(&self, track: Track, mode: Mode) {
        let account = self.state.borrow().youtube_account.clone();
        let result = self.downloads.manager.borrow_mut().start(
            track,
            mode,
            self.download_directory(),
            account,
        );
        if let Err(error) = result {
            self.notice(&self.tr(&error), true, false);
        }
    }

    pub(super) fn poll_downloads(self: &Rc<Self>) {
        let pending = self.downloads.pending.borrow().clone();
        if let Some(pending) = pending {
            if pending.generation != self.play_generation.get()
                || self.state.borrow().current.as_deref() != Some(&pending.track)
                || !self
                    .downloads
                    .manager
                    .borrow()
                    .jobs
                    .iter()
                    .any(|j| j.id == pending.job && j.status.active())
            {
                self.downloads.pending.borrow_mut().take();
                self.downloads.manager.borrow_mut().cancel(pending.job);
            } else if !self.resolving.get() {
                self.downloads.pending.borrow_mut().take();
                let result = self.audio.borrow().record_spotify(pending.capture);
                if let Err(error) = result {
                    self.downloads.manager.borrow_mut().cancel(pending.job);
                    self.notice(&self.tr(&error), true, false);
                }
            }
        }
        self.collect_downloads();
        if *self.page.borrow() == "downloads" {
            self.render_downloads();
        }
        self.update_save_control();
    }

    pub(super) fn collect_downloads(&self) {
        let complete = self.downloads.manager.borrow_mut().poll();
        if complete.is_empty() {
            return;
        }
        let mut state = self.state.borrow_mut();
        for item in complete {
            if item.mode == Mode::Download {
                if let Some(track) = state.tracks.iter_mut().find(|t| t.id == item.track.id) {
                    track.download_path = Some(item.path.clone());
                }
            }
            state.saved_audio.push(model::SavedAudio {
                id: item.id.to_string(),
                title: item.track.title,
                source_id: item.track.id,
                path: item.path,
                recording: item.mode != Mode::Download,
            });
        }
        drop(state);
        self.persist();
    }

    pub(super) fn render_downloads(&self) {
        let manager = self.downloads.manager.borrow();
        let state = self.state.borrow();
        object::<gtk::Box>(&self.ui, "downloads_empty")
            .set_visible(manager.jobs.is_empty() && state.saved_audio.is_empty());
        let list: gtk::ListBox = object(&self.ui, "downloads_list");
        let mut rows = self.downloads.rows.borrow_mut();
        let ids: HashSet<String> = manager
            .jobs
            .iter()
            .map(|j| j.id.to_string())
            .chain(state.saved_audio.iter().map(|s| s.id.clone()))
            .collect();
        rows.retain(|id, ui| {
            if ids.contains(id) {
                return true;
            }
            if let Some(row) = object::<gtk::Box>(ui, "download_row").parent() {
                list.remove(&row);
            }
            false
        });
        for id in manager
            .jobs
            .iter()
            .map(|j| j.id.to_string())
            .chain(state.saved_audio.iter().map(|s| s.id.clone()))
        {
            if rows.contains_key(&id) {
                continue;
            }
            let ui = gtk::Builder::from_string(include_str!("../../resources/download.ui"));
            self.i18n.builder(&ui);
            for (button, action) in [
                ("cancel", "app.download-cancel"),
                ("stop", "app.download-stop"),
                ("play", "app.saved-play"),
                ("location", "app.saved-location"),
                ("resume", "app.download-resume"),
                ("delete", "app.download-delete"),
            ] {
                let button: gtk::Button = object(&ui, button);
                button.set_action_name(Some(action));
                button.set_action_target_value(Some(&id.to_variant()));
            }
            list.prepend(&object::<gtk::Box>(&ui, "download_row"));
            rows.insert(id, ui);
        }
        for (id, ui) in rows.iter() {
            let job = manager.jobs.iter().find(|j| j.id.to_string() == *id);
            let saved = state.saved_audio.iter().find(|s| s.id == *id);
            let title = saved
                .map(|s| s.title.as_str())
                .or_else(|| job.map(|j| j.track.title.as_str()))
                .unwrap_or("");
            let active = job.is_some_and(|j| j.status.active());
            let deleting = self.downloads.deleting.borrow().contains(id);
            object::<gtk::Box>(ui, "download_row").set_sensitive(!deleting);
            let recording = job.map_or_else(
                || saved.is_some_and(|s| s.recording),
                |j| j.mode != Mode::Download,
            );
            let progress: gtk::ProgressBar = object(ui, "progress");
            progress.set_visible(active);
            let text = if deleting {
                self.tr("Moving to Trash…")
            } else if let Some(saved) = saved {
                saved.path.to_string_lossy().into_owned()
            } else if let Some(job) = job {
                let waiting = self
                    .downloads
                    .pending
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| p.job == job.id)
                    && active;
                let label = if waiting {
                    self.tr("Waiting for playback…")
                } else {
                    match &job.status {
                        Status::Queued => self.tr("Queued"),
                        Status::Starting => self.tr("Connecting…"),
                        Status::Running => self.tr(if recording {
                            "Recording"
                        } else {
                            "Downloading"
                        }),
                        Status::Finishing => self.tr("Saving…"),
                        Status::Finished => self.tr("Saved"),
                        Status::Cancelled => self.tr("Cancelled"),
                        Status::Failed(error) => self.tr(error),
                    }
                };
                if let Some(total) = job.total.filter(|n| *n > 0) {
                    progress.set_fraction((job.bytes as f64 / total as f64).min(1.0));
                    format!(
                        "{label} · {:.0}% · {} / {}",
                        progress.fraction() * 100.0,
                        size(job.bytes),
                        size(total)
                    )
                } else {
                    if active {
                        progress.pulse();
                    }
                    if recording {
                        format!(
                            "{label} · {} · {}",
                            model::duration(job.seconds),
                            size(job.bytes)
                        )
                    } else {
                        format!("{label} · {}", size(job.bytes))
                    }
                }
            } else {
                String::new()
            };
            object::<gtk::Label>(ui, "title").set_label(title);
            object::<gtk::Image>(ui, "icon").set_icon_name(Some(if recording {
                "media-record-symbolic"
            } else {
                "folder-download-symbolic"
            }));
            let label: gtk::Label = object(ui, "status");
            label.set_label(&text);
            label.set_tooltip_text(Some(&text));
            if job.is_some_and(|j| matches!(j.status, Status::Failed(_))) {
                label.add_css_class("error");
            } else {
                label.remove_css_class("error");
            }
            let cancel: gtk::Button = object(ui, "cancel");
            cancel.set_visible(active);
            cancel.set_tooltip_text(Some(&self.tr(if recording {
                "Cancel and discard"
            } else {
                "Cancel download"
            })));
            let resume: gtk::Button = object(ui, "resume");
            let failed = job.is_some_and(|j| matches!(j.status, Status::Failed(_)));
            resume.set_label(&self.tr(if failed { "Retry" } else { "Resume" }));
            resume.set_tooltip_text(Some(&self.tr(if failed {
                "Retry download"
            } else {
                "Resume download"
            })));
            resume.set_visible(job.is_some_and(|j| {
                j.mode == Mode::Download
                    && matches!(j.status, Status::Cancelled | Status::Failed(_))
            }));
            resume.set_sensitive(job.is_some_and(|j| j.can_resume()));
            let stop: gtk::Button = object(ui, "stop");
            stop.set_visible(active && recording);
            stop.set_sensitive(job.is_some_and(|j| j.status == Status::Running));
            object::<gtk::Button>(ui, "play").set_visible(saved.is_some());
            object::<gtk::Button>(ui, "location").set_visible(saved.is_some());
        }
    }

    pub(super) fn download_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        let label: gtk::Label = object(ui, "download_directory");
        label.set_label(&self.download_directory().to_string_lossy());
        let weak = Rc::downgrade(self);
        let parent = object::<gtk::Window>(ui, "settings_window").downgrade();
        object::<gtk::Button>(ui, "choose_download_directory").connect_clicked(move |_| {
            let (Some(app), Some(parent)) = (weak.upgrade(), parent.upgrade()) else {
                return;
            };
            let dialog = gtk::FileChooserNative::new(
                Some(&app.tr("Download folder")),
                Some(&parent),
                gtk::FileChooserAction::SelectFolder,
                Some(&app.tr("Select")),
                Some(&app.tr("Cancel")),
            );
            let _ = dialog.set_current_folder(Some(&gio::File::for_path(app.download_directory())));
            let weak = Rc::downgrade(&app);
            let label = label.clone();
            dialog.connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept {
                    if let (Some(app), Some(path)) =
                        (weak.upgrade(), dialog.file().and_then(|f| f.path()))
                    {
                        label.set_label(&path.to_string_lossy());
                        app.state.borrow_mut().download_directory = Some(path);
                        app.persist();
                    }
                }
                dialog.destroy();
            });
            dialog.show();
        });
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    }
}
