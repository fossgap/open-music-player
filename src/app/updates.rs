use super::*;

impl App {
    pub(super) fn refresh_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        let interval: gtk::SpinButton = object(ui, "refresh_minutes");
        interval.set_value(f64::from(self.state.borrow().refresh_minutes));
        let weak = Rc::downgrade(self);
        interval.connect_value_changed(move |spin| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().refresh_minutes = spin.value_as_int().clamp(0, 1440) as u32;
                app.refresh_timer.set(Instant::now());
                if spin.value_as_int() == 0 {
                    app.refresh_search.borrow_mut().clear();
                }
                app.persist();
            }
        });
    }

    pub(super) fn poll_source_refresh(self: &Rc<Self>) {
        let interval = self.state.borrow().refresh_minutes;
        if interval == 0 {
            return;
        }
        let page = self.page.borrow().clone();
        // A streaming sync and search share a backend; run the visible search after sync.
        for provider in ["youtube", "spotify"] {
            let idle = if provider == "youtube" {
                !self.youtube.busy.get() && !self.youtube.searching.get()
            } else {
                !self.spotify.busy.get() && !self.spotify.searching.get()
            };
            if idle
                && self.refresh_search.borrow_mut().remove(provider)
                && page == provider
                && !object::<gtk::SearchEntry>(&self.ui, "search")
                    .text()
                    .trim()
                    .is_empty()
            {
                if provider == "youtube" && !self.youtube.playlists.get() {
                    self.search_youtube();
                }
                if provider == "spotify"
                    && !self.spotify.playlists.get()
                    && self.audio.borrow().spotify_ready()
                {
                    self.search_spotify();
                }
            }
        }
        if self.refresh_timer.get().elapsed() < Duration::from_secs(u64::from(interval) * 60) {
            return;
        }
        self.refresh_timer.set(Instant::now());
        let (folders, youtube, spotify) = {
            let state = self.state.borrow();
            (
                state.folders.clone(),
                state.youtube_account.is_some(),
                state.spotify_account.is_some(),
            )
        };
        if !folders.is_empty() && !self.scanning.get() {
            self.scan_folders(folders, true);
        }
        self.refresh_podcasts(true);
        if youtube && !self.youtube.busy.get() && !self.youtube.searching.get() {
            self.sync_youtube();
            if page == "youtube" {
                self.refresh_search.borrow_mut().insert("youtube".into());
            }
        } else if !youtube
            && page == "youtube"
            && !self.youtube.busy.get()
            && !self.youtube.searching.get()
            && !self.youtube.playlists.get()
            && !object::<gtk::SearchEntry>(&self.ui, "search")
                .text()
                .trim()
                .is_empty()
        {
            self.search_youtube();
        }
        // Never open a sign-in dialog as part of an unattended refresh.
        if spotify
            && self.audio.borrow().spotify_ready()
            && !self.spotify.busy.get()
            && !self.spotify.searching.get()
        {
            self.sync_spotify();
            if page == "spotify" {
                self.refresh_search.borrow_mut().insert("spotify".into());
            }
        }
        if page == "podcasts"
            && self.podcast_discovery.get()
            && self.button("podcast_search").is_sensitive()
        {
            self.discover_podcasts();
        }
        if page == "radio" && !self.radio_refreshing.get() {
            self.discover_radio();
        }
    }
}
