use super::*;
use crate::spotify::{Command, Reply, Service, catalog};

#[derive(Default)]
pub(super) struct Controls {
    pub service: Service,
    pub playlists: Cell<bool>,
    pub results: RefCell<Vec<String>>,
    pub generation: Cell<u64>,
    pub searching: Cell<bool>,
    pub failed: Cell<bool>,
    pub busy: Cell<bool>,
    connecting: Cell<bool>,
    pub(super) pending_play: Cell<Option<u64>>,
    dialog: RefCell<Option<AccountDialog>>,
}
struct AccountDialog {
    window: gtk::Window,
    status: gtk::Label,
    connect: gtk::Button,
    disconnect: gtk::Button,
    cancel: gtk::Button,
}
impl App {
    pub(super) fn setup_spotify_controls(self: &Rc<Self>) {
        self.on("spotify_search", |a| a.search_spotify());
        self.on("spotify_sync", |a| a.sync_spotify());
        self.action("spotify-account", |a| a.spotify_primary());
        self.action("streaming", |a| {
            let page = a.streaming_service.borrow().clone();
            a.navigate(&page);
        });
        self.string_action("spotify-publish", |a, id| a.publish_spotify(id, false));
        self.string_action("spotify-reload", |a, id| a.publish_spotify(id, true));
        self.string_action("spotify-playlist-open", |a, id| {
            let remote = a
                .state
                .borrow()
                .playlist(id)
                .and_then(|p| p.spotify.as_ref())
                .map(|l| l.id.clone());
            if let Some(id) =
                remote.filter(|id| id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
            {
                if let Err(e) = gio::AppInfo::launch_default_for_uri(
                    &format!("https://open.spotify.com/playlist/{id}"),
                    None::<&gio::AppLaunchContext>,
                ) {
                    a.notice(&e.to_string(), true, false);
                }
            }
        });
        self.string_action("spotify-open", |a, id| {
            let url = a
                .state
                .borrow()
                .track(id)
                .and_then(|t| crate::spotify::track_url(&t.uri));
            if let Some(url) = url {
                if let Err(e) =
                    gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
                {
                    a.notice(&e.to_string(), true, false);
                }
            }
        });
        for page in ["youtube", "spotify", "streams"] {
            let weak = Rc::downgrade(self);
            object::<gtk::ToggleButton>(&self.ui, &format!("service_{page}")).connect_toggled(
                move |button| {
                    if let Some(app) = weak.upgrade() {
                        if button.is_active() && !app.updating.get() {
                            app.navigate(page);
                        }
                    }
                },
            );
        }
        let weak = Rc::downgrade(self);
        object::<gtk::ToggleButton>(&self.ui, "spotify_playlists").connect_toggled(move |button| {
            if let Some(app) = weak.upgrade() {
                app.spotify.playlists.set(button.is_active());
                app.playlist.borrow_mut().take();
                object::<gtk::SearchEntry>(&app.ui, "search").set_text("");
                app.refresh();
            }
        });
        let account = self.state.borrow().spotify_account.clone();
        if let Some(account) = account {
            self.restore_spotify(&account.id);
        }
    }
    fn restore_spotify(&self, account: &str) {
        self.spotify.busy.set(true);
        self.spotify.connecting.set(true);
        self.spotify.service.send(Command::Restore(account.into()));
    }
    pub(super) fn spotify_primary(self: &Rc<Self>) {
        if let Some(dialog) = self.spotify.dialog.borrow().as_ref() {
            dialog.window.present();
            return;
        }
        let window = gtk::Window::builder()
            .title(self.tr("Spotify account"))
            .transient_for(&self.window)
            .modal(true)
            .default_width(480)
            .resizable(false)
            .build();
        close_on_escape(&window);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        for side in [
            gtk::PositionType::Top,
            gtk::PositionType::Bottom,
            gtk::PositionType::Left,
            gtk::PositionType::Right,
        ] {
            match side {
                gtk::PositionType::Top => content.set_margin_top(24),
                gtk::PositionType::Bottom => content.set_margin_bottom(24),
                gtk::PositionType::Left => content.set_margin_start(24),
                _ => content.set_margin_end(24),
            }
        }
        let label = |text: &str| {
            let label = gtk::Label::new(Some(text));
            label.set_wrap(true);
            label.set_xalign(0.0);
            label.set_max_width_chars(50);
            label
        };
        let heading = label(&self.tr("Connect your Spotify account"));
        heading.add_css_class("title-2");
        content.append(&heading);
        content.append(&label(&self.tr("Spotify Premium is required. Sign in securely in your browser; your password is never entered in Open Music Player.")));
        content.append(&label(&self.tr("Sign-in is saved in your desktop keyring. Search music, play tracks and sync your playlists from Streaming.")));
        let advanced = gtk::Expander::new(Some(&self.tr("Advanced connection options")));
        let options = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let client = gtk::Entry::new();
        client.set_placeholder_text(Some(&self.tr("Spotify Client ID (optional)")));
        if let Ok(value) = std::env::var("OPEN_MUSIC_SPOTIFY_CLIENT_ID") {
            client.set_text(&value);
        }
        options.append(&client);
        options.append(&label(&self.tr("Leave empty to use librespot's public client. For your own Spotify developer app, register this redirect URI: http://127.0.0.1:5588/login")));
        advanced.set_child(Some(&options));
        content.append(&advanced);
        let status = label("");
        status.set_selectable(true);
        content.append(&status);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let connect = gtk::Button::with_label(&self.tr("Connect with Spotify"));
        connect.add_css_class("suggested-action");
        let disconnect = gtk::Button::with_label(&self.tr("Disconnect"));
        let cancel = gtk::Button::with_label(&self.tr("Cancel sign-in"));
        buttons.append(&connect);
        buttons.append(&disconnect);
        content.append(&buttons);
        content.append(&cancel);
        let weak = Rc::downgrade(self);
        connect.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                if app.spotify.busy.replace(true) {
                    return;
                }
                app.spotify.connecting.set(true);
                app.spotify.searching.set(false);
                app.spotify.generation.set(app.spotify.generation.get() + 1);
                app.spotify
                    .service
                    .send(Command::Connect(client.text().trim().into()));
                app.spotify_dialog_status(&app.tr("Complete Spotify sign-in in your browser…"));
                app.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.spotify.service.cancel();
                app.spotify.busy.set(false);
                app.spotify.connecting.set(false);
                if app.spotify.pending_play.take() == Some(app.play_generation.get()) {
                    app.stop_playback();
                }
                app.spotify_dialog_status(&app.tr("Sign-in cancelled"));
                app.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        disconnect.connect_clicked(move |_| { if let Some(app) = weak.upgrade() {
            let dialog = gtk::MessageDialog::builder().transient_for(&app.window).modal(true).text(app.tr("Disconnect Spotify?"))
                .secondary_text(app.tr("This removes Spotify sign-in, tracks and linked playlists from this computer. Your online library stays unchanged.")).build();
            dialog.add_button(&app.tr("Cancel"), gtk::ResponseType::Cancel); dialog.add_button(&app.tr("Disconnect"), gtk::ResponseType::Accept);
            let weak = Rc::downgrade(&app);
            dialog.connect_response(move |dialog, response| { if response == gtk::ResponseType::Accept { if let Some(app) = weak.upgrade() {
                app.spotify.busy.set(true); app.spotify.pending_play.set(None);
                if app.state.borrow().current_track().is_some_and(|t| t.kind == Kind::Spotify) { app.stop_playback(); }
                app.audio.borrow_mut().detach_spotify();
                app.spotify.service.send(Command::Disconnect); app.spotify_dialog_status(&app.tr("Disconnecting…")); app.refresh();
            } } dialog.close(); }); dialog.present();
        } });
        window.set_child(Some(&content));
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| {
            if let Some(app) = weak.upgrade() {
                app.spotify.dialog.borrow_mut().take();
            }
            glib::Propagation::Proceed
        });
        *self.spotify.dialog.borrow_mut() = Some(AccountDialog {
            window: window.clone(),
            status,
            connect,
            disconnect,
            cancel,
        });
        let message = if self.spotify.connecting.get() {
            self.tr("Connecting to Spotify…")
        } else if let Some(account) = self
            .state
            .borrow()
            .spotify_account
            .as_ref()
            .filter(|_| self.audio.borrow().spotify_ready())
        {
            format!("{}: {}", self.tr("Connected account"), account.name)
        } else {
            self.tr("Not connected")
        };
        self.spotify_dialog_status(&message);
        window.present();
    }
    fn spotify_dialog_status(&self, message: &str) {
        if let Some(dialog) = self.spotify.dialog.borrow().as_ref() {
            dialog.status.set_label(message);
            dialog.connect.set_sensitive(!self.spotify.busy.get());
            dialog.disconnect.set_sensitive(
                !self.spotify.busy.get() && self.state.borrow().spotify_account.is_some(),
            );
            dialog.cancel.set_visible(self.spotify.connecting.get());
        }
    }
    fn spotify_connected(self: &Rc<Self>) -> bool {
        if self.audio.borrow().spotify_ready() {
            return true;
        }
        self.spotify_primary();
        false
    }
    pub(super) fn search_spotify(self: &Rc<Self>) {
        if self.spotify.busy.get() || !self.spotify_connected() {
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
        let generation = self.spotify.generation.get() + 1;
        self.spotify.generation.set(generation);
        self.spotify.searching.set(true);
        self.spotify.failed.set(false);
        self.spotify.results.borrow_mut().clear();
        self.spotify
            .service
            .send(Command::Search(generation, query));
        self.notice(&self.tr("Searching Spotify…"), false, true);
        self.refresh();
    }
    pub(super) fn sync_spotify(self: &Rc<Self>) {
        if self.spotify.busy.get() || self.spotify.searching.get() || !self.spotify_connected() {
            return;
        }
        let state = self.state.borrow();
        self.spotify.service.send(Command::Sync(
            catalog::linked_playlists(&state),
            state.spotify_ignored_playlists.clone(),
        ));
        drop(state);
        self.spotify.busy.set(true);
        self.notice(&self.tr("Synchronizing Spotify playlists…"), false, true);
        self.refresh();
    }
    pub(super) fn refresh_spotify(self: &Rc<Self>) {
        if self.spotify.playlists.get() {
            self.sync_spotify();
        } else {
            self.search_spotify();
        }
    }
    pub(super) fn spotify_playlist_menu(&self, menu: &gio::Menu, playlist: &model::Playlist) {
        if playlist.youtube.is_some() {
            return;
        }
        if playlist.spotify.is_some() {
            let item = gio::MenuItem::new(Some(&self.tr("Open in Spotify")), None);
            item.set_action_and_target_value(
                Some("app.spotify-playlist-open"),
                Some(&playlist.id.to_variant()),
            );
            menu.append_item(&item);
        }
        for (label, action) in [
            (
                if playlist.spotify.is_some() {
                    "Sync with Spotify"
                } else {
                    "Publish privately to Spotify"
                },
                "app.spotify-publish",
            ),
            ("Replace local copy from Spotify", "app.spotify-reload"),
        ] {
            if action.ends_with("reload") && playlist.spotify.is_none() {
                continue;
            }
            let item = gio::MenuItem::new(Some(&self.tr(label)), None);
            item.set_action_and_target_value(Some(action), Some(&playlist.id.to_variant()));
            menu.append_item(&item);
        }
    }
    fn publish_spotify(self: &Rc<Self>, id: &str, reload: bool) {
        if self.spotify.busy.get() || self.spotify.searching.get() {
            return;
        }
        let Some(local) = catalog::local_playlist(&self.state.borrow(), id) else {
            return;
        };
        if !reload && (!local.valid || local.uris.is_empty()) {
            self.notice(
                &self.tr("Only playlists containing Spotify tracks can be published."),
                true,
                false,
            );
            return;
        }
        if !self.spotify_connected() {
            return;
        }
        let dialog = gtk::MessageDialog::builder().transient_for(&self.window).modal(true)
            .text(self.tr(if reload { "Replace local copy from Spotify?" } else if local.link.is_some() { "Sync with Spotify?" } else { "Publish privately to Spotify?" }))
            .secondary_text(self.tr(if reload { "Local edits will be replaced by the online playlist." } else { "Only Spotify tracks can be uploaded. Changes to linked playlists are synchronized with your Spotify account." })).build();
        dialog.add_button(&self.tr("Cancel"), gtk::ResponseType::Cancel);
        dialog.add_button(
            &self.tr(if reload {
                "Replace"
            } else if local.link.is_some() {
                "Sync playlists"
            } else {
                "Publish"
            }),
            gtk::ResponseType::Accept,
        );
        let weak = Rc::downgrade(self);
        let id = id.to_string();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(app) = weak.upgrade() {
                    if !app.spotify.busy.replace(true) {
                        if let Some(local) = catalog::local_playlist(&app.state.borrow(), &id) {
                            app.spotify.service.send(if reload {
                                Command::Reload(local)
                            } else {
                                Command::Publish(local)
                            });
                            app.notice(&app.tr("Synchronizing Spotify playlists…"), false, true);
                            app.refresh();
                        } else {
                            app.spotify.busy.set(false);
                        }
                    }
                }
            }
            dialog.close();
        });
        dialog.present();
    }
    pub(super) fn play_spotify(self: &Rc<Self>, track: &Track, position: f64, generation: u64) {
        if self.audio.borrow().spotify_ready() {
            let result = self.audio.borrow_mut().load_spotify(
                &track.uri,
                position,
                track.duration,
                self.resolve_autoplay.get(),
            );
            if let Err(e) = result {
                self.notice(&e, true, false);
            }
        } else {
            self.audio.borrow_mut().stop();
            let account = self.state.borrow().spotify_account.clone();
            if let Some(account) = account {
                self.resolving.set(true);
                self.spotify.pending_play.set(Some(generation));
                if !self.spotify.busy.get() {
                    self.restore_spotify(&account.id);
                }
                self.notice(&self.tr("Connecting to Spotify…"), false, true);
            } else {
                self.spotify_primary();
            }
        }
    }
    pub(super) fn handle_spotify(self: &Rc<Self>, reply: Reply) {
        match reply {
            Reply::Authorize(url) => {
                if let Err(e) =
                    gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
                {
                    self.spotify.service.cancel();
                    self.spotify.busy.set(false);
                    self.spotify.connecting.set(false);
                    if self.spotify.pending_play.take() == Some(self.play_generation.get()) {
                        self.resolving.set(false);
                    }
                    let message = format!("{} {e}", self.tr("Could not open the browser."));
                    self.spotify_dialog_status(&message);
                    self.notice(&message, true, false);
                }
            }
            Reply::Connected(result) => {
                self.spotify.busy.set(false);
                self.spotify.connecting.set(false);
                match result {
                    Ok(connected) => {
                        let (account, playback) = *connected;
                        if self
                            .state
                            .borrow()
                            .spotify_account
                            .as_ref()
                            .is_some_and(|a| a.id != account.id)
                        {
                            if self
                                .state
                                .borrow()
                                .current_track()
                                .is_some_and(|t| t.kind == Kind::Spotify)
                            {
                                self.stop_playback();
                            }
                            catalog::clear_account(&mut self.state.borrow_mut());
                            self.spotify.results.borrow_mut().clear();
                        }
                        let message = format!("{}: {}", self.tr("Connected account"), account.name);
                        self.state.borrow_mut().spotify_account = Some(account);
                        self.audio.borrow_mut().attach_spotify(playback);
                        self.audio.borrow().volume(self.state.borrow().volume);
                        self.spotify_dialog_status(&message);
                        self.notice(&message, false, false);
                        if self.spotify.pending_play.take() == Some(self.play_generation.get())
                            && self.resolving.replace(false)
                        {
                            let track = self.state.borrow().current_track().cloned();
                            if let Some(track) = track.filter(|t| t.kind == Kind::Spotify) {
                                let position = self.state.borrow().position;
                                self.play_spotify(&track, position, self.play_generation.get());
                            }
                        }
                    }
                    Err(error) => {
                        if self.spotify.pending_play.take() == Some(self.play_generation.get()) {
                            self.resolving.set(false);
                        }
                        self.spotify_dialog_status(&error);
                        self.notice(&error, true, false);
                    }
                }
            }
            Reply::Disconnected(result) => {
                self.cancel_discography();
                self.spotify.busy.set(false);
                self.spotify.searching.set(false);
                self.spotify.connecting.set(false);
                match result {
                    Ok(()) => {
                        self.spotify.results.borrow_mut().clear();
                        catalog::clear_account(&mut self.state.borrow_mut());
                        self.playlist.borrow_mut().take();
                        self.spotify_dialog_status(&self.tr("Not connected"));
                        self.notice(&self.tr("Spotify disconnected"), false, false);
                    }
                    Err(error) => {
                        self.spotify_dialog_status(&error);
                        self.notice(&error, true, false);
                    }
                }
            }
            Reply::GlobalSearch(generation, result) => {
                self.global_spotify_reply(generation, result);
                return;
            }
            Reply::Search(generation, result) => {
                if generation != self.spotify.generation.get() {
                    return;
                }
                self.spotify.searching.set(false);
                match result {
                    Ok(tracks) => {
                        *self.spotify.results.borrow_mut() =
                            tracks.iter().map(|t| t.id.clone()).collect();
                        self.state.borrow_mut().merge(tracks);
                        object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                    }
                    Err(error) => {
                        self.spotify.failed.set(true);
                        self.notice(&error, true, false);
                    }
                }
            }
            Reply::Album(id, result) => {
                self.handle_discography(id, super::discography::Provider::Spotify, result);
                return;
            }
            Reply::Synced(account, before, result) => {
                self.spotify.busy.set(false);
                match result {
                    Ok(result) => {
                        let failed = result.failed.len();
                        let uploaded = result.uploaded;
                        let details = result
                            .failed
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        let (synced, conflicts, skipped) = catalog::apply_sync(
                            &mut self.state.borrow_mut(),
                            &account,
                            &before,
                            result,
                        );
                        let mut message =
                            format!("{}: {synced}", self.tr("Playlists synchronized"));
                        for (label, count) in [
                            ("Uploaded", uploaded),
                            ("Conflicts kept unchanged", conflicts),
                            ("Failed", failed),
                            ("Unavailable tracks skipped", skipped),
                        ] {
                            if count > 0 {
                                message.push_str(&format!(" · {}: {count}", self.tr(label)));
                            }
                        }
                        if !details.is_empty() {
                            message.push_str(&format!("\n{details}"));
                        }
                        self.notice(&message, failed + conflicts > 0, false);
                        self.spotify_dialog_status(&message);
                    }
                    Err(error) => {
                        self.notice(&error, true, false);
                        self.spotify_dialog_status(&error);
                    }
                }
            }
        }
        self.refresh();
        self.ensure_artwork();
        self.update_player();
        self.persist();
    }
}
