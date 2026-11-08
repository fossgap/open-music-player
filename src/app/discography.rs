use super::*;
use crate::albums::Query;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Provider {
    Spotify,
    Youtube,
}

impl Provider {
    fn name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::Youtube => "YouTube Music",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Play,
    Queue,
}

pub(super) struct Pending {
    id: u64,
    query: Query,
    action: Action,
    cancel: Arc<AtomicU64>,
    providers: VecDeque<Provider>,
    active: Option<Provider>,
    accounts: (Option<String>, Option<String>),
    track: Option<String>,
    play_generation: u64,
    errors: Vec<String>,
}

pub(super) struct Row {
    pub label: gtk::Label,
    pub row: gtk::ListBoxRow,
    pub play: gtk::Button,
    pub queue: gtk::Button,
    pub play_icon: gtk::Image,
    pub queue_icon: gtk::Image,
    pub kind: String,
    pub title: String,
}

#[derive(Default)]
pub(super) struct Controls {
    pub rows: RefCell<HashMap<String, Row>>,
    generation: Cell<u64>,
    pending: RefCell<BTreeMap<u64, Pending>>,
    pub selected: RefCell<Option<String>>,
    pub rendering: Cell<bool>,
}

fn accounts(state: &State) -> (Option<String>, Option<String>) {
    (
        state.spotify_account.as_ref().map(|a| a.id.clone()),
        state.youtube_account.as_ref().map(|a| a.id.clone()),
    )
}

impl App {
    pub(super) fn activate_discography(self: &Rc<Self>, id: &str, action: Action) {
        let existing = self
            .discography
            .pending
            .borrow()
            .values()
            .find(|p| p.query.id == id)
            .map(|p| (p.id, p.action));
        if let Some((job, previous)) = existing {
            if previous == action {
                self.cancel_discography_job(job);
            } else {
                if action == Action::Play {
                    self.cancel_discography_playback();
                }
                if let Some(pending) = self.discography.pending.borrow_mut().get_mut(&job) {
                    pending.action = action;
                    pending.track = self.state.borrow().current.clone();
                    pending.play_generation = self.play_generation.get();
                }
                self.update_discography_buttons();
            }
            return;
        }
        let state = self.state.borrow();
        let Some(track) = state.current_track() else {
            return;
        };
        let Some(info) = &track.music_info else {
            return;
        };
        let Some(release) = info.discography.entries.iter().find(|r| r.id == id) else {
            return;
        };
        let query = Query {
            id: release.id.clone(),
            artist_id: info.discography.artist_id.clone(),
            artist: info
                .artist
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| track.artist.clone()),
            title: release.title.clone(),
            date: release.date.clone(),
        };
        let tracks = query.local_tracks(&state);
        drop(state);
        if action == Action::Play {
            self.cancel_discography_playback();
        }
        if !tracks.is_empty() {
            self.apply_release(action, tracks);
            return;
        }
        let job = self.begin_discography(query, action);
        self.request_discography(job);
    }

    // Separate intent from dispatch so stale replies can be tested without real accounts.
    pub(super) fn begin_discography(&self, query: Query, action: Action) -> u64 {
        if action == Action::Play {
            self.cancel_discography_playback();
        }
        let id = self.discography.generation.get() + 1;
        self.discography.generation.set(id);
        let state = self.state.borrow();
        let accounts = accounts(&state);
        let prefer_spotify = match state.current_track().map(|t| &t.kind) {
            Some(Kind::Spotify) => true,
            Some(Kind::YoutubeMusic) => false,
            _ => *self.streaming_service.borrow() == "spotify",
        };
        let mut providers = VecDeque::from([Provider::Youtube]);
        if accounts.0.is_some() {
            if prefer_spotify {
                providers.push_front(Provider::Spotify);
            } else {
                providers.push_back(Provider::Spotify);
            }
        }
        self.discography.pending.borrow_mut().insert(
            id,
            Pending {
                id,
                query,
                action,
                cancel: Arc::new(AtomicU64::new(id)),
                providers,
                active: None,
                accounts,
                track: self.state.borrow().current.clone(),
                play_generation: self.play_generation.get(),
                errors: vec![],
            },
        );
        id
    }

    pub(super) fn cancel_discography(&self) {
        self.cancel_discography_jobs(|_| true);
    }

    pub(super) fn cancel_discography_playback(&self) {
        self.cancel_discography_jobs(|p| p.action == Action::Play);
    }

    fn cancel_discography_job(&self, id: u64) {
        self.cancel_discography_jobs(|p| p.id == id);
    }

    fn cancel_discography_jobs(&self, cancel: impl Fn(&Pending) -> bool) {
        self.discography.pending.borrow_mut().retain(|_, job| {
            if cancel(job) {
                job.cancel.store(0, Ordering::SeqCst);
                false
            } else {
                true
            }
        });
        self.update_discography_buttons();
    }

    fn discography_job_valid(&self, job: &Pending) -> bool {
        job.accounts == accounts(&self.state.borrow())
            && (job.action == Action::Queue
                || (job.track == self.state.borrow().current
                    && job.play_generation == self.play_generation.get()))
    }

    fn request_discography(self: &Rc<Self>, id: u64) {
        let mut pending = self.discography.pending.borrow_mut();
        let Some(job) = pending.get_mut(&id) else {
            return;
        };
        if !self.discography_job_valid(job) {
            drop(pending);
            self.cancel_discography_job(id);
            return;
        }
        let Some(provider) = job.providers.pop_front() else {
            let text = if job.errors.is_empty() {
                self.tr("No matching playable album was found in your streaming services.")
            } else {
                job.errors.join("\n")
            };
            drop(pending);
            self.cancel_discography_job(id);
            self.notice(&text, true, false);
            return;
        };
        job.active = Some(provider);
        let (query, generation) = (job.query.clone(), job.cancel.clone());
        drop(pending);
        self.update_discography_buttons();
        match provider {
            Provider::Spotify => self
                .spotify
                .service
                .send(crate::spotify::Command::Album(id, query, generation)),
            Provider::Youtube => {
                let account = self.state.borrow().youtube_account.clone();
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = youtube_music::album(account.as_ref(), &query, || {
                        generation.load(Ordering::SeqCst) != id
                    });
                    let _ = tx.send(Message::Discography(id, Provider::Youtube, result));
                });
            }
        }
    }

    pub(super) fn handle_discography(
        self: &Rc<Self>,
        id: u64,
        provider: Provider,
        result: Result<Vec<Track>, String>,
    ) {
        let mut pending = self.discography.pending.borrow_mut();
        let Some(job) = pending.get_mut(&id).filter(|p| p.active == Some(provider)) else {
            return;
        };
        if !self.discography_job_valid(job) {
            drop(pending);
            self.cancel_discography_job(id);
            return;
        }
        match result {
            Ok(tracks) if !tracks.is_empty() => {
                // Library changes during the lookup still get local-playback priority.
                let action = job.action;
                let local = job.query.local_tracks(&self.state.borrow());
                drop(pending);
                self.cancel_discography_job(id);
                if !local.is_empty() {
                    self.apply_release(action, local);
                    return;
                }
                let ids = tracks.iter().map(|t| t.id.clone()).collect();
                self.state.borrow_mut().merge(tracks);
                self.apply_release(action, ids);
                self.ensure_artwork();
            }
            result => {
                if let Err(error) = result {
                    job.errors
                        .push(format!("{}: {}", provider.name(), self.tr(&error)));
                }
                drop(pending);
                self.request_discography(id);
            }
        }
    }

    fn apply_release(self: &Rc<Self>, action: Action, mut ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        if action == Action::Queue {
            // The queue identifies entries by track ID, so keep its existing
            // uniqueness rule while appending new album tracks in order.
            let mut state = self.state.borrow_mut();
            for id in ids {
                if !state.queue.contains(&id) {
                    state.queue.push(id);
                }
            }
            drop(state);
            self.refresh();
            self.prepare_transition();
            self.persist();
            self.notice(&self.tr("Added to queue"), false, false);
        } else {
            if self.state.borrow().shuffle {
                model::shuffle(&mut ids);
            }
            let first = ids[0].clone();
            self.state.borrow_mut().queue = ids;
            self.play_track(&first, Some(0.0));
        }
    }

    pub(super) fn update_discography_buttons(&self) {
        let pending = self.discography.pending.borrow();
        for (id, row) in self.discography.rows.borrow().iter() {
            let job = pending.values().find(|p| p.query.id == *id);
            let provider = job.and_then(|p| p.active);
            row.label.set_text(
                &provider
                    .map(|p| format!("{} {}…", self.tr("Searching"), p.name()))
                    .unwrap_or_else(|| row.kind.clone()),
            );
            for (action, button, icon, symbol, label, hint) in [
                (
                    Action::Play,
                    &row.play,
                    &row.play_icon,
                    "media-playback-start-symbolic",
                    "Play album",
                    "Replace queue and play album",
                ),
                (
                    Action::Queue,
                    &row.queue,
                    &row.queue_icon,
                    "list-add-symbolic",
                    "Add album to queue",
                    "Append album without interrupting playback",
                ),
            ] {
                let active = job.is_some_and(|p| p.action == action);
                icon.set_icon_name(Some(if active {
                    "process-stop-symbolic"
                } else {
                    symbol
                }));
                let label = self.tr(if active { "Cancel album lookup" } else { label });
                button.update_property(&[gtk::accessible::Property::Label(&format!(
                    "{label}: {}",
                    row.title
                ))]);
                button.set_tooltip_text(Some(&format!(
                    "{}\n{}",
                    row.title,
                    self.tr(if active { "Cancel album lookup" } else { hint })
                )));
            }
        }
    }
}

#[cfg(test)]
pub(super) fn exercise(app: &Rc<App>, tone: &std::path::Path) {
    use super::tests::settle;
    use enrichment::{Discography, Entity, Information, Release};
    let before = app.state.borrow().clone();
    app.stop_playback();
    let group = "72345678-1234-1234-1234-123456789abc";
    let artist_id = "82345678-1234-1234-1234-123456789abc";
    let song = |id: &str, album: &str, number| {
        let mut track = Track::new(&gio::File::for_path(tone).uri(), id, Kind::Music);
        track.id = id.into();
        track.artist = "Discography ensemble".into();
        track.album = album.into();
        track.number = number;
        track
    };
    let query = Query {
        id: group.into(),
        artist_id: artist_id.into(),
        artist: "Discography ensemble".into(),
        title: "Local album".into(),
        date: "2021".into(),
    };
    let mut origin = song("discographyorigin", "Context album", 1);
    origin.music_info = Some(Information {
        artist: Some(Entity {
            id: artist_id.into(),
            name: query.artist.clone(),
            ..Default::default()
        }),
        discography: Discography {
            artist_id: artist_id.into(),
            entries: vec![Release {
                id: group.into(),
                title: query.title.clone(),
                date: query.date.clone(),
                kind: "Album".into(),
            }],
            total: 1,
            ..Default::default()
        },
        ..Default::default()
    });
    {
        let mut state = app.state.borrow_mut();
        state.fetch_information = false;
        state.fetch_covers = false;
        state.fetch_lyrics = false;
        state.shuffle = false;
        state.merge(vec![
            origin,
            song("discographysecond", "Local album", 2),
            song("discographyfirst", "Local album", 1),
        ]);
    }
    app.play_track("discographyorigin", Some(0.0));
    settle();
    let list: gtk::ListBox = object(&app.ui, "discography_list");
    let button = |action: Action| {
        let rows = app.discography.rows.borrow();
        let row = rows.get(group).unwrap();
        match action {
            Action::Play => row.play.clone(),
            Action::Queue => row.queue.clone(),
        }
    };
    assert_eq!(
        button(Action::Play).action_name().as_deref(),
        Some("app.play-release")
    );
    assert_eq!(
        button(Action::Queue).action_name().as_deref(),
        Some("app.queue-release")
    );
    let row = app
        .discography
        .rows
        .borrow()
        .get(group)
        .unwrap()
        .row
        .clone();
    assert!(!row.is_activatable());
    object::<gtk::Revealer>(&app.ui, "detail_revealer").set_reveal_child(true);
    object::<gtk::Stack>(&app.ui, "detail_stack").set_visible_child_name("information");
    app.window.set_default_size(1240, 800);
    settle();
    let queue = app.state.borrow().queue.clone();
    // Actual body clicks, including a double click, must only select the row.
    click_release(app, &row, 1);
    assert_eq!(list.selected_row().unwrap().widget_name(), group);
    click_release(app, &row, 2);
    assert_eq!(app.state.borrow().queue, queue);
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographyorigin")
    );
    assert!(app.discography.pending.borrow().is_empty());
    // Paging or fresh metadata rebuilds rows without losing the selected release.
    app.state
        .borrow_mut()
        .tracks
        .iter_mut()
        .find(|t| t.id == "discographyorigin")
        .unwrap()
        .music_info
        .as_mut()
        .unwrap()
        .discography
        .total = 2;
    app.update_metadata();
    assert_eq!(list.selected_row().unwrap().widget_name(), group);
    super::tests::screenshot(app.window.upcast_ref(), "discography-actions-wide.png");
    app.window.set_default_size(820, 650);
    settle();
    app.window.set_default_size(800, 650);
    settle();
    assert!(app.window.width() <= 820);
    for action in [Action::Play, Action::Queue] {
        let bounds = button(action).compute_bounds(&app.window).unwrap();
        assert!(bounds.x() >= 0.0 && bounds.x() + bounds.width() <= app.window.width() as f32);
    }
    super::tests::screenshot(app.window.upcast_ref(), "discography-actions-compact.png");
    app.window.set_default_size(1240, 800);

    // Queue preserves the entire prefix, current track, and playing/paused state.
    app.state.borrow_mut().queue = vec!["discographyorigin".into()];
    button(Action::Queue).emit_clicked();
    assert_eq!(
        app.state.borrow().queue,
        ["discographyorigin", "discographyfirst", "discographysecond"]
    );
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographyorigin")
    );
    assert!(app.audio.borrow().playing);
    app.toggle_play();
    settle();
    let position = app.audio.borrow().position();
    app.state.borrow_mut().queue = vec!["discographyorigin".into()];
    button(Action::Queue).emit_clicked();
    button(Action::Queue).emit_clicked();
    assert_eq!(
        app.state.borrow().queue,
        ["discographyorigin", "discographyfirst", "discographysecond"]
    );
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographyorigin")
    );
    assert!(!app.audio.borrow().playing);
    assert!((app.audio.borrow().position() - position).abs() < 0.1);
    assert_eq!(list.selected_row().unwrap().widget_name(), group);
    assert!(
        app.discography.pending.borrow().is_empty(),
        "Local albums must not start online requests"
    );

    button(Action::Play).emit_clicked();
    settle();
    assert_eq!(
        app.state.borrow().queue,
        ["discographyfirst", "discographysecond"]
    );
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographyfirst")
    );
    assert!(app.audio.borrow().playing);

    app.play_track("discographyorigin", Some(0.0));
    let mut remote = query.clone();
    remote.title = "Online album".into();
    let begin = |query: Query, action| {
        let id = app.begin_discography(query, action);
        app.discography
            .pending
            .borrow_mut()
            .get_mut(&id)
            .unwrap()
            .active = Some(Provider::Youtube);
        app.update_discography_buttons();
        id
    };
    for action in [Action::Play, Action::Queue] {
        let id = begin(remote.clone(), action);
        let token = app
            .discography
            .pending
            .borrow()
            .get(&id)
            .unwrap()
            .cancel
            .clone();
        let rows = app.discography.rows.borrow();
        let row = rows.get(group).unwrap();
        let icon = match action {
            Action::Play => &row.play_icon,
            Action::Queue => &row.queue_icon,
        };
        assert_eq!(icon.icon_name().as_deref(), Some("process-stop-symbolic"));
        drop(rows);
        // A second click cancels this intent; late results cannot alter the queue.
        let queue = app.state.borrow().queue.clone();
        button(action).emit_clicked();
        assert_eq!(token.load(Ordering::SeqCst), 0);
        app.handle_discography(
            id,
            Provider::Youtube,
            Ok(vec![song("discographycancelled", "Online album", 1)]),
        );
        assert!(app.state.borrow().track("discographycancelled").is_none());
        assert_eq!(app.state.borrow().queue, queue);
        assert_eq!(
            app.state.borrow().current.as_deref(),
            Some("discographyorigin")
        );
    }

    let stale = begin(remote.clone(), Action::Play);
    app.play_track("discographysecond", Some(0.0));
    app.handle_discography(
        stale,
        Provider::Youtube,
        Ok(vec![song("discographystale", "Online album", 1)]),
    );
    assert!(app.state.borrow().track("discographystale").is_none());
    let old = begin(remote.clone(), Action::Play);
    let new = begin(remote.clone(), Action::Play);
    app.handle_discography(
        old,
        Provider::Youtube,
        Ok(vec![song("discographystale", "Online album", 1)]),
    );
    assert!(app.state.borrow().track("discographystale").is_none());
    // Catalogue and bridge tests cover native streaming records. PCM fixtures
    // here exercise the hand-off through the real player without an account.
    app.handle_discography(
        new,
        Provider::Youtube,
        Ok(vec![
            song("discographyonlinefirst", "Online album", 1),
            song("discographyonlinesecond", "Online album", 2),
        ]),
    );
    settle();
    assert_eq!(
        app.state.borrow().queue,
        ["discographyonlinefirst", "discographyonlinesecond"]
    );
    assert!(app.audio.borrow().playing);

    // Independent queue requests survive manual playback changes and append to
    // the latest queue, retaining edits made while the catalogues were searching.
    remote.title = "Queued album".into();
    let first = begin(remote.clone(), Action::Queue);
    let mut other = remote.clone();
    other.id = "92345678-1234-1234-1234-123456789abc".into();
    other.title = "Another album".into();
    let second = begin(other, Action::Queue);
    app.play_track("discographyorigin", Some(0.0));
    app.toggle_play();
    app.state.borrow_mut().queue = vec!["discographyorigin".into(), "discographysecond".into()];
    assert_eq!(app.discography.pending.borrow().len(), 2);
    app.handle_discography(
        first,
        Provider::Youtube,
        Ok(vec![
            song("discographyqueuedfirst", "Queued album", 1),
            song("discographyqueuedsecond", "Queued album", 2),
        ]),
    );
    app.handle_discography(
        second,
        Provider::Youtube,
        Ok(vec![song("discographyother", "Another album", 1)]),
    );
    assert_eq!(
        app.state.borrow().queue,
        [
            "discographyorigin",
            "discographysecond",
            "discographyqueuedfirst",
            "discographyqueuedsecond",
            "discographyother",
        ]
    );
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographyorigin")
    );
    assert!(!app.audio.borrow().playing);
    assert!(app.discography.pending.borrow().is_empty());

    // Change a pending Play into Queue using its separate button: no replay.
    remote.title = "Changed intent".into();
    let id = begin(remote.clone(), Action::Play);
    button(Action::Queue).emit_clicked();
    assert_eq!(
        app.discography.pending.borrow().get(&id).unwrap().action,
        Action::Queue
    );
    let mut expected = app.state.borrow().queue.clone();
    expected.push("discographychanged".into());
    app.handle_discography(
        id,
        Provider::Youtube,
        Ok(vec![song("discographychanged", "Changed intent", 1)]),
    );
    assert_eq!(app.state.borrow().queue, expected);
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographyorigin")
    );
    assert!(!app.audio.borrow().playing);

    // A queued lookup can also become an explicit Play without another request.
    remote.title = "Play instead".into();
    let id = begin(remote.clone(), Action::Queue);
    button(Action::Play).emit_clicked();
    assert_eq!(
        app.discography.pending.borrow().get(&id).unwrap().action,
        Action::Play
    );
    app.handle_discography(
        id,
        Provider::Youtube,
        Ok(vec![song("discographypromoted", "Play instead", 1)]),
    );
    assert_eq!(app.state.borrow().queue, ["discographypromoted"]);
    assert_eq!(
        app.state.borrow().current.as_deref(),
        Some("discographypromoted")
    );
    assert!(app.audio.borrow().playing);

    // Explicitly clearing the queue cancels outstanding additions as well.
    remote.title = "Cleared album".into();
    let id = begin(remote.clone(), Action::Queue);
    let page = app.page.borrow().clone();
    app.navigate("queue");
    app.primary_action();
    app.handle_discography(
        id,
        Provider::Youtube,
        Ok(vec![song("discographycleared", "Cleared album", 1)]),
    );
    assert!(app.state.borrow().queue.is_empty());
    assert!(app.state.borrow().track("discographycleared").is_none());
    app.navigate(&page);
    app.stop_playback();
    app.state.borrow_mut().current = None;
    app.apply_release(
        Action::Queue,
        vec!["discographyfirst".into(), "discographysecond".into()],
    );
    assert_eq!(
        app.state.borrow().queue,
        ["discographyfirst", "discographysecond"]
    );
    assert!(app.state.borrow().current.is_none());
    assert!(!app.audio.borrow().playing && !app.audio.borrow().loaded);

    remote.title = "Missing album".into();
    let id = begin(remote.clone(), Action::Queue);
    app.discography
        .pending
        .borrow_mut()
        .get_mut(&id)
        .unwrap()
        .providers
        .clear();
    let queue = app.state.borrow().queue.clone();
    app.handle_discography(id, Provider::Youtube, Ok(vec![]));
    assert_eq!(app.state.borrow().queue, queue);
    assert!(app.discography.pending.borrow().is_empty());
    assert!(object::<gtk::Box>(&app.ui, "notice").is_visible());

    let id = begin(remote, Action::Queue);
    app.state.borrow_mut().spotify_account = Some(model::SpotifyAccount {
        id: "changed-account".into(),
        name: "Changed".into(),
    });
    app.handle_discography(
        id,
        Provider::Youtube,
        Ok(vec![song("discographywrongaccount", "Missing album", 1)]),
    );
    assert!(
        app.state
            .borrow()
            .track("discographywrongaccount")
            .is_none()
    );
    assert_eq!(app.state.borrow().queue, queue);
    app.cancel_discography();
    app.stop_playback();
    *app.state.borrow_mut() = before;
    object::<gtk::Stack>(&app.ui, "detail_stack").set_visible_child_name("now-playing");
    app.refresh();
    app.update_player();
}

#[cfg(test)]
fn click_release(app: &Rc<App>, row: &gtk::ListBoxRow, clicks: u8) {
    use super::tests::settle;
    let xdo = |args: &[&str]| {
        let result = std::process::Command::new("xdotool")
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "xdotool: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    };
    let scroller: gtk::ScrolledWindow = object(&app.ui, "information_scroller");
    scroller
        .vadjustment()
        .set_value(scroller.vadjustment().upper());
    settle();
    let bounds = row.compute_bounds(&app.window).unwrap();
    let windows = xdo(&["search", "--onlyvisible", "--name", "^Open Music Player$"]);
    let window = windows.lines().last().unwrap();
    xdo(&["windowfocus", "--sync", window]);
    let (dx, dy) = app.window.surface_transform();
    xdo(&[
        "mousemove",
        "--window",
        window,
        &((f64::from(bounds.x()) + dx + 12.0).round() as i32).to_string(),
        &((f64::from(bounds.y() + bounds.height() / 2.0) + dy).round() as i32).to_string(),
    ]);
    xdo(&[
        "click",
        "--repeat",
        &clicks.to_string(),
        "--delay",
        "80",
        "1",
    ]);
    settle();
}
