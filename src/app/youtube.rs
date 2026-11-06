use super::*;
use crate::model::YoutubeAccount;
use youtube_music::{LocalPlaylist, RemotePlaylist, SyncResult};

#[derive(Default)]
pub(super) struct Controls {
    pub playlists: Cell<bool>,
    pub results: RefCell<Vec<String>>,
    pub generation: Cell<u64>,
    pub searching: Cell<bool>,
    pub failed: Cell<bool>,
    pub busy: Cell<bool>,
    dialog: RefCell<Option<AccountDialog>>,
}

struct AccountDialog {
    window: gtk::Window,
    status: gtk::Label,
    connect: gtk::Button,
    disconnect: gtk::Button,
}

pub(super) enum Reply {
    Search(u64, Result<Vec<Track>, String>),
    Connected(Result<YoutubeAccount, String>),
    Disconnected(Result<(), String>),
    Synced(String, Vec<LocalPlaylist>, Result<SyncResult, String>),
    Published(Box<(String, LocalPlaylist, Result<RemotePlaylist, String>)>),
}

impl App {
    pub(super) fn youtube_playlist_menu(&self, menu: &gio::Menu, playlist: &model::Playlist) {
        let item = gio::MenuItem::new(
            Some(&self.tr(if playlist.youtube.is_some() {
                "Sync with YouTube Music"
            } else {
                "Publish privately to YouTube Music"
            })),
            None,
        );
        item.set_action_and_target_value(
            Some("app.youtube-publish"),
            Some(&playlist.id.to_variant()),
        );
        menu.append_item(&item);
        if playlist.youtube.is_some() {
            let reload = gio::MenuItem::new(
                Some(&self.tr("Replace local copy from YouTube Music")),
                None,
            );
            reload.set_action_and_target_value(
                Some("app.youtube-reload"),
                Some(&playlist.id.to_variant()),
            );
            menu.append_item(&reload);
        }
    }

    pub(super) fn setup_youtube_controls(self: &Rc<Self>) {
        self.action("youtube-account", |a| a.youtube_primary());
        self.action("youtube-sync", |a| a.sync_youtube());
        self.string_action("youtube-publish", |a, id| a.publish_youtube(id));
        self.string_action("youtube-reload", |a, id| a.reload_youtube(id));
        self.on("youtube_search", |a| a.search_youtube());
        self.on("youtube_sync", |a| a.sync_youtube());
        let weak = Rc::downgrade(self);
        object::<gtk::ToggleButton>(&self.ui, "youtube_playlists").connect_toggled(move |button| {
            if let Some(app) = weak.upgrade() {
                app.youtube.playlists.set(button.is_active());
                app.playlist.borrow_mut().take();
                object::<gtk::SearchEntry>(&app.ui, "search").set_text("");
                app.refresh();
            }
        });
    }

    pub(super) fn youtube_primary(self: &Rc<Self>) {
        if let Some(dialog) = self.youtube.dialog.borrow().as_ref() {
            dialog.window.present();
            return;
        }
        let window = gtk::Window::builder()
            .title(self.tr("YouTube Music account"))
            .transient_for(&self.window)
            .modal(true)
            .default_width(480)
            .resizable(false)
            .build();
        close_on_escape(&window);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.set_margin_top(24);
        content.set_margin_bottom(24);
        content.set_margin_start(24);
        content.set_margin_end(24);
        let label = |text: &str| {
            let label = gtk::Label::new(Some(text));
            label.set_wrap(true);
            label.set_xalign(0.0);
            label.set_max_width_chars(52);
            label
        };
        let heading = label(&self.tr("Connect your account"));
        heading.add_css_class("title-2");
        content.append(&heading);
        content.append(&label(&self.tr("Sign in to YouTube Music in your browser, then connect that browser profile. Free accounts are supported; no Google password or API key is entered here.")));
        let open = gtk::Button::with_label(&self.tr("Open YouTube Music"));
        let weak = Rc::downgrade(self);
        open.connect_clicked(move |_| {
            if let Err(error) = gio::AppInfo::launch_default_for_uri(
                "https://music.youtube.com/",
                None::<&gio::AppLaunchContext>,
            ) {
                if let Some(app) = weak.upgrade() {
                    app.notice(&error.to_string(), true, false);
                }
            }
        });
        content.append(&open);
        let browser_label = label(&self.tr("Signed-in browser"));
        content.append(&browser_label);
        let browsers = [
            "Firefox", "Chrome", "Chromium", "Brave", "Edge", "Opera", "Vivaldi",
        ];
        let browser = gtk::DropDown::from_strings(&browsers);
        browser.set_tooltip_text(Some(&self.tr("Signed-in browser")));
        content.append(&browser);
        let advanced = gtk::Expander::new(Some(&self.tr("Advanced connection options")));
        let advanced_content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        advanced.set_child(Some(&advanced_content));
        content.append(&advanced);
        let profile = gtk::Entry::new();
        profile.set_placeholder_text(Some(&self.tr("Browser profile path (optional)")));
        advanced_content.append(&profile);
        advanced_content.append(&label(&self.tr("Uses the first signed-in account in this profile. Use a separate browser profile to choose a different account.")));
        let file_button = gtk::Button::with_label(&self.tr("Import browser cookie file…"));
        let cookie_file = Rc::new(RefCell::new(None::<String>));
        let file_label = label("");
        file_label.set_visible(false);
        file_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let weak = Rc::downgrade(self);
        let chosen = cookie_file.clone();
        let filename_label = file_label.clone();
        let parent = window.downgrade();
        file_button.connect_clicked(move |_| {
            let (Some(app), Some(parent)) = (weak.upgrade(), parent.upgrade()) else {
                return;
            };
            let chooser = gtk::FileChooserNative::new(
                Some(&app.tr("Choose a YouTube cookie file")),
                Some(&parent),
                gtk::FileChooserAction::Open,
                Some(&app.tr("Open")),
                Some(&app.tr("Cancel")),
            );
            let chosen = chosen.clone();
            let file_label = filename_label.clone();
            chooser.connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = dialog.file().and_then(|f| f.path()) {
                        file_label
                            .set_label(&path.file_name().unwrap_or_default().to_string_lossy());
                        file_label.set_visible(true);
                        *chosen.borrow_mut() = Some(path.to_string_lossy().into_owned());
                    }
                }
                dialog.destroy();
            });
            chooser.show();
        });
        advanced_content.append(&file_button);
        advanced_content.append(&file_label);
        content.append(&label(&self.tr("Only YouTube cookies are saved, in your desktop keyring. This unofficial integration may need updates when YouTube changes. Imported cookie files remain on your computer.")));
        let status = label("");
        status.set_selectable(true);
        content.append(&status);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let disconnect = gtk::Button::with_label(&self.tr("Disconnect"));
        disconnect.add_css_class("destructive-action");
        let connect = gtk::Button::with_label(&self.tr("Connect account"));
        connect.add_css_class("suggested-action");
        connect.set_hexpand(true);
        actions.append(&disconnect);
        actions.append(&connect);
        content.append(&actions);
        let weak = Rc::downgrade(self);
        connect.connect_clicked(move |_| {
            let Some(app) = weak.upgrade() else { return };
            if app.youtube.busy.replace(true) {
                return;
            }
            let browser = browsers[browser.selected() as usize].to_ascii_lowercase();
            let profile = profile.text().to_string();
            let file = cookie_file.borrow().clone();
            app.youtube_dialog_status(&app.tr("Connecting to YouTube Music…"));
            app.notice(&app.tr("Connecting to YouTube Music…"), false, true);
            app.refresh();
            let tx = app.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Message::Music(Reply::Connected(youtube_music::connect(
                    &browser,
                    &profile,
                    file.as_deref(),
                ))));
            });
        });
        let weak = Rc::downgrade(self);
        disconnect.connect_clicked(move |_| {
            let Some(app) = weak.upgrade() else { return };
            if app.youtube.busy.get() { return; }
            let confirmation = gtk::MessageDialog::builder().transient_for(&app.window).modal(true)
                .text(app.tr("Disconnect YouTube Music?"))
                .secondary_text(app.tr("This removes the saved session, YouTube Music tracks and linked playlists from this computer. Your online library stays unchanged."))
                .build();
            confirmation.add_button(&app.tr("Cancel"), gtk::ResponseType::Cancel);
            confirmation.add_button(&app.tr("Disconnect"), gtk::ResponseType::Accept);
            let weak = Rc::downgrade(&app);
            confirmation.connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(app) = weak.upgrade() {
                        if !app.youtube.busy.replace(true) {
                            app.youtube_dialog_status(&app.tr("Disconnecting…")); app.refresh();
                            let tx = app.tx.clone(); std::thread::spawn(move || { let _ = tx.send(Message::Music(Reply::Disconnected(youtube_music::disconnect()))); });
                        }
                    }
                }
                dialog.close();
            }); confirmation.present();
        });
        window.set_child(Some(&content));
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| {
            if let Some(app) = weak.upgrade() {
                app.youtube.dialog.borrow_mut().take();
            }
            glib::Propagation::Proceed
        });
        *self.youtube.dialog.borrow_mut() = Some(AccountDialog {
            window: window.clone(),
            status,
            connect,
            disconnect,
        });
        let account = self.state.borrow().youtube_account.clone();
        self.youtube_dialog_status(
            &account
                .map(|a| format!("{}: {}", self.tr("Connected account"), a.name))
                .unwrap_or_else(|| self.tr("Not connected")),
        );
        window.present();
    }

    fn youtube_dialog_status(&self, text: &str) {
        if let Some(dialog) = self.youtube.dialog.borrow().as_ref() {
            dialog.status.set_label(text);
            dialog.connect.set_sensitive(!self.youtube.busy.get());
            dialog.disconnect.set_sensitive(
                !self.youtube.busy.get() && self.state.borrow().youtube_account.is_some(),
            );
        }
    }

    pub(super) fn search_youtube(self: &Rc<Self>) {
        if self.youtube.busy.get() {
            return;
        }
        let query = object::<gtk::SearchEntry>(&self.ui, "search")
            .text()
            .trim()
            .to_string();
        if query.is_empty() {
            object::<gtk::SearchEntry>(&self.ui, "search").grab_focus();
            return;
        }
        let generation = self.youtube.generation.get() + 1;
        self.youtube.generation.set(generation);
        self.youtube.searching.set(true);
        self.youtube.failed.set(false);
        self.youtube.results.borrow_mut().clear();
        let account = self.state.borrow().youtube_account.clone();
        self.notice(&self.tr("Searching YouTube Music…"), false, true);
        self.refresh();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Message::Music(Reply::Search(
                generation,
                youtube_music::search(account.as_ref(), &query),
            )));
        });
    }

    pub(super) fn refresh_youtube(self: &Rc<Self>) {
        if self.youtube.playlists.get() {
            self.sync_youtube();
        } else {
            self.search_youtube();
        }
    }

    pub(super) fn sync_youtube(self: &Rc<Self>) {
        if self.youtube.busy.get() {
            return;
        }
        let state = self.state.borrow();
        let Some(account) = state.youtube_account.clone() else {
            drop(state);
            self.youtube_primary();
            return;
        };
        let playlists = youtube_music::linked_playlists(&state);
        let ignored = state.youtube_ignored_playlists.clone();
        drop(state);
        self.youtube.busy.set(true);
        self.notice(
            &self.tr("Synchronizing YouTube Music playlists…"),
            false,
            true,
        );
        self.refresh();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = youtube_music::synchronize(&account, &playlists, &ignored);
            let _ = tx.send(Message::Music(Reply::Synced(account.id, playlists, result)));
        });
    }

    pub(super) fn publish_youtube(self: &Rc<Self>, id: &str) {
        if self.youtube.busy.get() {
            return;
        }
        let state = self.state.borrow();
        let Some(account) = state.youtube_account.clone() else {
            drop(state);
            self.youtube_primary();
            return;
        };
        let Some(playlist) = youtube_music::local_playlist(&state, id) else {
            return;
        };
        if playlist.link.is_some() {
            drop(state);
            self.sync_youtube();
            return;
        }
        if !playlist.valid || playlist.video_ids.is_empty() {
            drop(state);
            self.notice(
                &self.tr("Only playlists containing YouTube Music tracks can be published."),
                true,
                false,
            );
            return;
        }
        drop(state);
        self.youtube.busy.set(true);
        self.notice(
            &self.tr("Creating a private YouTube Music playlist…"),
            false,
            true,
        );
        self.refresh();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = youtube_music::publish(&account, &playlist);
            let _ = tx.send(Message::Music(Reply::Published(Box::new((
                account.id, playlist, result,
            )))));
        });
    }

    fn reload_youtube(self: &Rc<Self>, id: &str) {
        if self.youtube.busy.get() {
            return;
        }
        let state = self.state.borrow();
        let Some(account) = state.youtube_account.clone() else {
            return;
        };
        let Some(playlist) = youtube_music::local_playlist(&state, id).filter(|p| p.link.is_some())
        else {
            return;
        };
        drop(state);
        let dialog = gtk::MessageDialog::builder().transient_for(&self.window).modal(true)
            .text(self.tr("Replace local copy from YouTube Music"))
            .secondary_text(self.tr("Local edits to this playlist will be replaced by the online version. Your online playlist stays unchanged."))
            .build();
        dialog.add_button(&self.tr("Cancel"), gtk::ResponseType::Cancel);
        dialog.add_button(&self.tr("Replace local copy"), gtk::ResponseType::Accept);
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(app) = weak.upgrade() {
                    if !app.youtube.busy.replace(true) {
                        let account = account.clone();
                        let before =
                            youtube_music::local_playlist(&app.state.borrow(), &playlist.local_id);
                        if let Some(before) = before {
                            app.notice(
                                &app.tr("Synchronizing YouTube Music playlists…"),
                                false,
                                true,
                            );
                            app.refresh();
                            let tx = app.tx.clone();
                            std::thread::spawn(move || {
                                let result =
                                    youtube_music::reload(&account, &before).map(|p| SyncResult {
                                        playlists: vec![p],
                                        ..SyncResult::default()
                                    });
                                let _ = tx.send(Message::Music(Reply::Synced(
                                    account.id,
                                    vec![before],
                                    result,
                                )));
                            });
                        } else {
                            app.youtube.busy.set(false);
                        }
                    }
                }
            }
            dialog.close();
        });
        dialog.present();
    }

    fn invalidate_youtube_session(&self) {
        self.youtube
            .generation
            .set(self.youtube.generation.get() + 1);
        self.youtube.searching.set(false);
        self.youtube.failed.set(false);
        self.youtube.results.borrow_mut().clear();
        if self
            .state
            .borrow()
            .current_track()
            .is_some_and(|t| services::youtube_url(&t.uri))
        {
            self.stop_playback();
        }
    }

    pub(super) fn handle_youtube(self: &Rc<Self>, reply: Reply) {
        match reply {
            Reply::Search(generation, result) => {
                if generation != self.youtube.generation.get() {
                    return;
                }
                self.youtube.searching.set(false);
                match result {
                    Ok(tracks) => {
                        *self.youtube.results.borrow_mut() =
                            tracks.iter().map(|t| t.id.clone()).collect();
                        self.state.borrow_mut().merge(tracks);
                        object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                    }
                    Err(error) => {
                        self.youtube.failed.set(true);
                        self.notice(&error, true, false);
                    }
                }
            }
            Reply::Connected(result) => {
                self.youtube.busy.set(false);
                match result {
                    Ok(account) => {
                        self.invalidate_youtube_session();
                        if self
                            .state
                            .borrow()
                            .youtube_account
                            .as_ref()
                            .is_some_and(|a| a.id != account.id)
                        {
                            youtube_music::clear_account(&mut self.state.borrow_mut());
                        }
                        let message = format!("{}: {}", self.tr("Connected account"), account.name);
                        self.state.borrow_mut().youtube_account = Some(account);
                        self.youtube_dialog_status(&message);
                        self.notice(&message, false, false);
                    }
                    Err(error) => {
                        self.youtube_dialog_status(&error);
                        self.notice(&error, true, false);
                    }
                }
            }
            Reply::Disconnected(result) => {
                self.youtube.busy.set(false);
                match result {
                    Ok(()) => {
                        self.invalidate_youtube_session();
                        youtube_music::clear_account(&mut self.state.borrow_mut());
                        self.playlist.borrow_mut().take();
                        self.youtube_dialog_status(&self.tr("Not connected"));
                        self.notice(&self.tr("YouTube Music disconnected"), false, false);
                    }
                    Err(error) => {
                        self.youtube_dialog_status(&error);
                        self.notice(&error, true, false);
                    }
                }
            }
            Reply::Synced(account, before, result) => {
                self.finish_youtube_sync(&account, &before, result);
            }
            Reply::Published(payload) => {
                let (account, before, result) = *payload;
                self.finish_youtube_sync(
                    &account,
                    &[before],
                    result.map(|p| SyncResult {
                        playlists: vec![p],
                        uploaded: 1,
                        ..SyncResult::default()
                    }),
                );
            }
        }
        self.refresh();
        self.ensure_artwork();
        self.update_player();
        self.persist();
    }

    fn finish_youtube_sync(
        &self,
        account: &str,
        before: &[LocalPlaylist],
        result: Result<SyncResult, String>,
    ) {
        self.youtube.busy.set(false);
        match result {
            Ok(result) => {
                let affected = result
                    .conflicts
                    .iter()
                    .chain(result.failed.iter())
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let applied = youtube_music::apply_sync(
                    &mut self.state.borrow_mut(),
                    account,
                    before,
                    result,
                );
                let mut message =
                    format!("{}: {}", self.tr("Playlists synchronized"), applied.synced);
                for (label, count) in [
                    ("Uploaded", applied.uploaded),
                    ("Conflicts kept unchanged", applied.conflicts),
                    ("Failed", applied.failed),
                    ("Unavailable tracks skipped", applied.unavailable),
                ] {
                    if count > 0 {
                        message.push_str(&format!(" · {}: {count}", self.tr(label)));
                    }
                }
                if !affected.is_empty() {
                    message.push_str(&format!("\n{affected}"));
                }
                if applied.conflicts > 0 {
                    message.push_str(&format!("\n{}", self.tr("Keep your local edits, or choose Replace local copy from YouTube Music in the playlist menu.")));
                }
                self.notice(&message, applied.conflicts + applied.failed > 0, false);
                self.youtube_dialog_status(&message);
            }
            Err(error) => {
                self.notice(&error, true, false);
                self.youtube_dialog_status(&error);
            }
        }
    }
}
