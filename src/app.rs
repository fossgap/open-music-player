use crate::{
    audio::{Audio, Event},
    enrichment,
    i18n::I18n,
    library,
    model::{self, Kind, Repeat, State, Subscription, Track},
    playlists, services, storage, youtube_music,
};

mod artists;
mod artwork;
mod browse;
mod discography;
mod downloads;
mod files;
mod information;
mod interactions;
mod lyrics;
mod metadata;
mod playback;
mod podcasts;
mod preferences;
mod render_area;
mod search;
mod selection;
mod spotify;
mod tags;
#[cfg(test)]
mod tests;
mod theme;
mod updates;
mod visualization;
mod watermark;
mod window_state;
mod youtube;
type BoundItems = RefCell<HashMap<usize, (glib::WeakRef<gtk::ListItem>, gtk::Builder)>>;
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

enum Message {
    Scan(Result<library::Scan, String>, Vec<PathBuf>),
    LocalImport(Result<library::Import, String>),
    AudioDetails(String, String, Result<crate::audio_info::Details, String>),
    Radio(u64, Result<Vec<Track>, String>),
    PodcastSearch(u64, Result<Vec<services::PodcastShow>, String>),
    PlaylistImport(Result<playlists::Import, String>),
    Feed(podcasts::FeedReply),
    Youtube(u64, Result<youtube_music::Resolved, String>),
    Prepared((u64, String), Result<crate::audio::NextTrack, String>),
    Music(youtube::Reply),
    Discography(u64, discography::Provider, Result<Vec<Track>, String>),
    Artwork(String, Result<String, String>),
    SaveError(String),
}

pub struct App {
    pub application: gtk::Application,
    pub window: gtk::ApplicationWindow,
    pub state: RefCell<State>,
    pub audio: RefCell<Audio>,
    pub tray_available: Cell<bool>,
    ui: gtk::Builder,
    mini_ui: gtk::Builder,
    i18n: I18n,
    page: RefCell<String>,
    visible: RefCell<Vec<String>>,
    albums: RefCell<Vec<Vec<String>>>,
    tracks_model: gtk::StringList,
    selection_syncing: Cell<bool>,
    selection_context: RefCell<String>,
    albums_model: gtk::StringList,
    playlists_model: gtk::StringList,
    podcasts_model: gtk::StringList,
    track_rows: BoundItems,
    album_cards: BoundItems,
    artists: artists::Controls,
    playlist_cards: BoundItems,
    podcast_cards: BoundItems,
    youtube: youtube::Controls,
    spotify: spotify::Controls,
    search: search::Controls,
    metadata: RefCell<enrichment::Service>,
    metadata_last: Cell<Instant>,
    lyrics_key: RefCell<Option<String>>,
    lyrics: lyrics::Controls,
    information: information::Controls,
    discography: discography::Controls,
    details_requested: RefCell<HashSet<String>>,
    streaming_service: RefCell<String>,
    radio_results: RefCell<Vec<String>>,
    playlist: RefCell<Option<String>>,
    album: RefCell<Option<(String, String)>>,
    parent_query: RefCell<String>,
    podcast_discovery: Cell<bool>,
    podcast_results: RefCell<Vec<services::PodcastShow>>,
    podcast_generation: Cell<u64>,
    podcast_failed: Cell<bool>,
    podcasts: podcasts::Controls,
    refresh_timer: Cell<Instant>,
    refresh_search: RefCell<HashSet<String>>,
    radio_refreshing: Cell<bool>,
    scan_background: Cell<bool>,
    compact_browse: Cell<bool>,
    importing_playlist: Cell<bool>,
    source_generation: Cell<u64>,
    play_generation: Cell<u64>,
    resolving: Cell<bool>,
    resolve_autoplay: Cell<bool>,
    preparation: RefCell<playback::Preparation>,
    scanning: Cell<bool>,
    files: files::Controls,
    tags: tags::Controls,
    downloads: downloads::Controls,
    tx: mpsc::Sender<Message>,
    rx: RefCell<mpsc::Receiver<Message>>,
    save_tx: RefCell<Option<mpsc::Sender<State>>>,
    save_worker: RefCell<Option<std::thread::JoinHandle<()>>>,
    last_saved: Cell<Instant>,
    updating: Cell<bool>,
    muted_volume: Cell<f64>,
    buffering_since: Cell<Option<Instant>>,
    buffering_percent: Cell<i32>,
    buffering_notice: Cell<bool>,
    visualization: RefCell<visualization::Controls>,
    theme: RefCell<theme::Theme>,
    background_hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    watermark: watermark::Controls,
    backdrop: Cell<(f64, f64, f64)>,
    backdrop_art: RefCell<Option<gtk::gdk_pixbuf::Pixbuf>>,
    artwork_pending: RefCell<HashSet<String>>,
    images: artwork::Images,
    cache: PathBuf,
    pub dbus: RefCell<Option<gio::DBusConnection>>,
}

pub fn object<T: IsA<glib::Object>>(builder: &gtk::Builder, id: &str) -> T {
    builder
        .object(id)
        .unwrap_or_else(|| panic!("Missing UI object: {id}"))
}

fn close_on_escape(window: &gtk::Window) {
    let key = gtk::EventControllerKey::new();
    let weak = window.downgrade();
    key.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key);
}

impl App {
    pub fn new(application: &gtk::Application, state: State) -> Result<Rc<Self>, String> {
        let mut audio = Audio::new()?;
        audio.configure(state.playback);
        let i18n = I18n::new(&state.language);
        render_area::RenderArea::static_type();
        let ui = gtk::Builder::from_string(include_str!("../resources/window.ui"));
        let mini_ui = gtk::Builder::from_string(include_str!("../resources/mini.ui"));
        i18n.builder(&ui);
        i18n.builder(&mini_ui);
        let window: gtk::ApplicationWindow = object(&ui, "window");
        window.set_application(Some(application));
        let mini: gtk::Window = object(&mini_ui, "mini_window");
        mini.set_application(Some(application));
        let theme = theme::Theme::new(state.theme);
        let (tx, rx) = mpsc::channel();
        let (save_tx, save_rx) = mpsc::channel::<State>();
        let path = storage::data_dir().join("library.json");
        let error_tx = tx.clone();
        let worker = std::thread::spawn(move || {
            while let Ok(mut state) = save_rx.recv() {
                while let Ok(newer) = save_rx.try_recv() {
                    state = newer;
                }
                if let Err(e) = storage::save(&path, &state) {
                    eprintln!("{e}");
                    let _ = error_tx.send(Message::SaveError(e));
                }
            }
        });
        let this = Rc::new(Self {
            application: application.clone(),
            window,
            state: RefCell::new(state),
            audio: RefCell::new(audio),
            tray_available: Cell::new(false),
            ui,
            mini_ui,
            i18n,
            page: RefCell::new("library".into()),
            visible: RefCell::new(vec![]),
            albums: RefCell::new(vec![]),
            tracks_model: gtk::StringList::new(&[]),
            selection_syncing: Cell::new(false),
            selection_context: RefCell::new(String::new()),
            albums_model: gtk::StringList::new(&[]),
            playlists_model: gtk::StringList::new(&[]),
            podcasts_model: gtk::StringList::new(&[]),
            track_rows: Default::default(),
            album_cards: Default::default(),
            artists: Default::default(),
            playlist_cards: Default::default(),
            podcast_cards: Default::default(),
            youtube: youtube::Controls::default(),
            spotify: spotify::Controls::default(),
            search: search::Controls::default(),
            metadata: RefCell::new(enrichment::Service::new(enrichment::Config::default())),
            metadata_last: Cell::new(Instant::now()),
            lyrics_key: RefCell::new(None),
            lyrics: Default::default(),
            information: Default::default(),
            discography: Default::default(),
            details_requested: Default::default(),
            streaming_service: RefCell::new("youtube".into()),
            radio_results: RefCell::new(vec![]),
            playlist: RefCell::new(None),
            album: RefCell::new(None),
            parent_query: RefCell::new(String::new()),
            podcast_discovery: Cell::new(false),
            podcast_results: RefCell::new(vec![]),
            podcast_generation: Cell::new(0),
            podcast_failed: Cell::new(false),
            podcasts: podcasts::Controls::default(),
            refresh_timer: Cell::new(Instant::now()),
            refresh_search: RefCell::new(HashSet::new()),
            radio_refreshing: Cell::new(false),
            scan_background: Cell::new(false),
            compact_browse: Cell::new(false),
            importing_playlist: Cell::new(false),
            source_generation: Cell::new(0),
            play_generation: Cell::new(0),
            resolving: Cell::new(false),
            resolve_autoplay: Cell::new(true),
            preparation: Default::default(),
            scanning: Cell::new(false),
            files: files::Controls::default(),
            tags: tags::Controls::default(),
            downloads: downloads::Controls::default(),
            tx,
            rx: RefCell::new(rx),
            save_tx: RefCell::new(Some(save_tx)),
            save_worker: RefCell::new(Some(worker)),
            last_saved: Cell::new(Instant::now()),
            updating: Cell::new(false),
            muted_volume: Cell::new(0.75),
            buffering_since: Cell::new(None),
            buffering_percent: Cell::new(100),
            buffering_notice: Cell::new(false),
            visualization: RefCell::new(Default::default()),
            theme: RefCell::new(theme),
            background_hold: RefCell::new(None),
            watermark: Default::default(),
            backdrop: Cell::new((0.17, 0.23, 0.17)),
            backdrop_art: RefCell::new(None),
            artwork_pending: RefCell::new(HashSet::new()),
            images: artwork::Images::default(),
            cache: storage::data_dir().join("artwork"),
            dbus: RefCell::new(None),
        });
        this.setup_actions();
        this.setup_lists();
        this.setup_selection();
        this.setup_files();
        this.setup_tags();
        this.setup_global_search();
        this.setup_watermark();
        this.setup_controls();
        this.setup_window_state();
        this.setup_timing();
        this.setup_downloads();
        this.setup_metadata();
        this.setup_lyrics();
        this.setup_wheel_controls();
        this.setup_drawing();
        this.setup_system_appearance();
        this.audio.borrow().volume(this.state.borrow().volume);
        let (volume, shuffle) = {
            let state = this.state.borrow();
            (state.volume, state.shuffle)
        };
        object::<gtk::Scale>(&this.ui, "volume").set_value(volume);
        object::<gtk::ToggleButton>(&this.ui, "shuffle").set_active(shuffle);
        this.refresh();
        this.update_player();
        let weak = Rc::downgrade(&this);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if let Some(app) = weak.upgrade() {
                app.tick();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        crate::desktop::setup(&this);
        if this.state.borrow().start_in_background {
            this.enter_background();
        } else {
            this.window.present();
        }
        Ok(this)
    }

    pub fn tr(&self, key: &str) -> String {
        self.i18n.text(key)
    }
    fn label(&self, id: &str, text: &str) {
        object::<gtk::Label>(&self.ui, id).set_label(text);
    }
    fn button(&self, id: &str) -> gtk::Button {
        object(&self.ui, id)
    }
    fn on(self: &Rc<Self>, id: &str, handler: impl Fn(&Rc<Self>) + 'static) {
        let weak = Rc::downgrade(self);
        self.button(id).connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                handler(&app);
            }
        });
    }
    fn action(self: &Rc<Self>, name: &str, handler: impl Fn(&Rc<Self>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            if let Some(app) = weak.upgrade() {
                handler(&app);
            }
        });
        self.application.add_action(&action);
    }
    fn string_action(self: &Rc<Self>, name: &str, handler: impl Fn(&Rc<Self>, &str) + 'static) {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let weak = Rc::downgrade(self);
        action.connect_activate(move |_, param| {
            if let (Some(app), Some(value)) = (weak.upgrade(), param.and_then(|p| p.str())) {
                handler(&app, value);
            }
        });
        self.application.add_action(&action);
    }

    fn setup_actions(self: &Rc<Self>) {
        self.action("play", |app| app.toggle_play());
        self.action("next", |app| app.next(false));
        self.action("previous", |app| app.previous());
        self.action("quit", |app| app.quit());
        self.action("import", |app| app.choose_folder());
        self.action("settings", |app| app.settings());
        self.action("mini", |app| app.mini());
        self.action("show", |app| app.show());
        self.action("shortcuts", |app| app.shortcuts());
        self.action("primary", |app| app.primary_action());
        self.action("refresh", |app| app.refresh_sources());
        self.action("back", |app| app.browse_back());
        self.action("playlist-import", |app| app.choose_playlist());
        self.action("new-playlist", |app| app.playlist_dialog(None, None));
        self.action("podcast-search", |app| app.discover_podcasts());
        self.string_action("open-album", |app, id| app.open_album(id));
        self.string_action("open-playlist", |app, id| app.open_playlist(id));
        self.string_action("rename-playlist", |app, id| {
            app.playlist_dialog(None, Some(id.into()))
        });
        self.string_action("delete-playlist", |app, id| app.delete_playlist(id));
        self.string_action("import-playlist", |app, path| {
            app.import_playlist(PathBuf::from(path))
        });
        self.string_action("subscribe-show", |app, id| app.subscribe_show(id));
        self.action("details", |app| app.toggle_details());
        self.action("albums", |app| {
            app.navigate("library");
            object::<gtk::ToggleButton>(&app.ui, "albums_view").set_active(true);
        });
        self.action("tracks", |app| {
            app.navigate("library");
            object::<gtk::ToggleButton>(&app.ui, "tracks_view").set_active(true);
        });
        self.action("artists", |app| {
            app.navigate("library");
            object::<gtk::ToggleButton>(&app.ui, "artists_view").set_active(true);
        });
        self.string_action("subscribe", |app, uri| app.subscribe(uri));
        self.string_action("page", |app, page| app.navigate(page));
        self.string_action("play-track", |app, id| app.play_track(id, None));
        self.string_action("enqueue", |app, id| {
            if !app.state.borrow().queue.contains(&id.to_string()) {
                app.state.borrow_mut().queue.push(id.into());
            }
            app.persist();
            app.notice(&app.tr("Added to queue"), false, false);
            if *app.page.borrow() == "queue" {
                app.refresh();
            }
        });
        self.string_action("favorite", |app, id| app.favorite(id));
        self.string_action("playlist-add", |app, id| {
            app.playlist_dialog(Some(id.into()), None)
        });
        self.string_action("remove", |app, id| app.remove(id));
        self.application
            .set_accels_for_action("app.play", &["<Primary>space"]);
        self.application
            .set_accels_for_action("app.next", &["<Alt>Right"]);
        self.application
            .set_accels_for_action("app.previous", &["<Alt>Left"]);
        self.application
            .set_accels_for_action("app.import", &["<Primary>o"]);
        self.application
            .set_accels_for_action("app.quit", &["<Primary>q"]);
        self.application
            .set_accels_for_action("app.mini", &["<Primary>m"]);
        self.application
            .set_accels_for_action("app.settings", &["<Primary>comma"]);
        self.application
            .set_accels_for_action("app.details", &["<Primary>l"]);
        self.application
            .set_accels_for_action("app.refresh", &["<Primary>r"]);
        self.application
            .set_accels_for_action("app.back", &["<Alt>Up"]);
        let menu = gio::Menu::new();
        for (label, action) in [
            ("Add music", "app.import"),
            ("Mini player", "app.mini"),
            ("Settings", "app.settings"),
            ("Keyboard shortcuts", "app.shortcuts"),
            ("Quit", "app.quit"),
        ] {
            menu.append(Some(&self.tr(label)), Some(action));
        }
        object::<gtk::MenuButton>(&self.ui, "main_menu").set_menu_model(Some(&menu));
    }

    fn setup_controls(self: &Rc<Self>) {
        self.setup_browse_controls();
        self.setup_youtube_controls();
        self.setup_spotify_controls();
        self.on("play", |a| a.toggle_play());
        self.on("next", |a| a.next(false));
        self.on("previous", |a| a.previous());
        self.on("mini", |a| a.mini());
        self.on("page_action", |a| a.primary_action());
        self.on("empty_action", |a| {
            if *a.page.borrow() == "youtube" {
                if a.youtube.playlists.get() {
                    a.sync_youtube();
                } else {
                    a.search_youtube();
                }
            } else if *a.page.borrow() == "spotify" {
                a.refresh_spotify();
            } else if *a.page.borrow() == "podcasts" {
                a.discover_podcasts();
            } else if *a.page.borrow() == "playlists" && a.playlist.borrow().is_some() {
                a.navigate("library");
            } else {
                a.primary_action();
            }
        });
        self.on("refresh", |a| a.refresh_sources());
        self.on("play_all", |a| {
            let first = a.visible.borrow().first().cloned();
            if let Some(id) = first {
                a.start_visible(&id);
            }
        });
        self.on("url_submit", |a| a.submit_url());
        self.on("url_cancel", |a| {
            object::<gtk::Box>(&a.ui, "entry_bar").set_visible(false)
        });
        self.on("dismiss_notice", |a| {
            object::<gtk::Box>(&a.ui, "notice").set_visible(false)
        });
        self.on("sidebar_toggle", |a| {
            let sidebar: gtk::ScrolledWindow = object(&a.ui, "sidebar_scroller");
            sidebar.set_visible(!sidebar.is_visible());
        });
        self.on("art_button", |a| a.toggle_details());
        self.on("detail_close", |a| {
            object::<gtk::Revealer>(&a.ui, "detail_revealer").set_visible(false)
        });
        self.on("player_favorite", |a| {
            let id = a.state.borrow().current.clone();
            if let Some(id) = id {
                a.favorite(&id);
            }
        });
        self.on("repeat", |a| {
            let repeat = match a.state.borrow().repeat {
                Repeat::Off => Repeat::All,
                Repeat::All => Repeat::One,
                Repeat::One => Repeat::Off,
            };
            a.state.borrow_mut().repeat = repeat;
            a.update_player();
            a.persist();
        });
        self.on("mute", |a| {
            let volume = a.state.borrow().volume;
            if volume > 0.0 {
                a.muted_volume.set(volume);
            }
            a.set_volume(if volume > 0.0 {
                0.0
            } else {
                a.muted_volume.get()
            });
        });
        let weak = Rc::downgrade(self);
        object::<gtk::Entry>(&self.ui, "url_entry").connect_activate(move |_| {
            if let Some(a) = weak.upgrade() {
                a.submit_url();
            }
        });
        let weak = Rc::downgrade(self);
        object::<gtk::SearchEntry>(&self.ui, "search").connect_search_changed(move |_| {
            if let Some(a) = weak.upgrade() {
                a.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        object::<gtk::SearchEntry>(&self.ui, "search").connect_activate(move |_| {
            if let Some(a) = weak.upgrade() {
                if *a.page.borrow() == "youtube" && !a.youtube.playlists.get() {
                    a.search_youtube();
                } else if *a.page.borrow() == "spotify" && !a.spotify.playlists.get() {
                    a.search_spotify();
                } else if *a.page.borrow() == "radio" {
                    a.discover_radio();
                } else if *a.page.borrow() == "podcasts" && a.podcast_discovery.get() {
                    a.discover_podcasts();
                }
            }
        });
        for (id, label) in [
            ("tracks_view", "Tracks"),
            ("albums_view", "Albums"),
            ("artists_view", "Artists"),
        ] {
            let button: gtk::ToggleButton = object(&self.ui, id);
            button.update_property(&[gtk::accessible::Property::Label(&self.tr(label))]);
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |_| {
                if let Some(a) = weak.upgrade() {
                    a.refresh();
                }
            });
        }
        let weak = Rc::downgrade(self);
        object::<gtk::ToggleButton>(&self.ui, "shuffle").connect_toggled(move |button| {
            if let Some(a) = weak.upgrade()
                && !a.updating.get()
            {
                a.set_shuffle(button.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        object::<gtk::Scale>(&self.ui, "volume").connect_value_changed(move |scale| {
            if let Some(a) = weak.upgrade()
                && !a.updating.get()
            {
                a.set_volume(scale.value());
            }
        });
        let weak = Rc::downgrade(self);
        object::<gtk::Scale>(&self.ui, "seek").connect_change_value(move |_, _, value| {
            if let Some(a) = weak.upgrade() {
                a.seek(value);
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(a) = weak.upgrade() {
                a.persist();
                if a.state.borrow().background {
                    a.enter_background();
                } else {
                    a.quit();
                }
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        object::<gtk::Button>(&self.mini_ui, "mini_expand").connect_clicked(move |_| {
            if let Some(a) = weak.upgrade() {
                a.show();
            }
        });
        let weak = Rc::downgrade(self);
        object::<gtk::Window>(&self.mini_ui, "mini_window").connect_close_request(move |w| {
            w.hide();
            if let Some(a) = weak.upgrade() {
                if a.state.borrow().background {
                    a.enter_background();
                } else {
                    a.quit();
                }
            }
            glib::Propagation::Stop
        });
        let key = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        key.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::f {
                if let Some(a) = weak.upgrade() {
                    if *a.page.borrow() == "search" {
                        a.focus_global_search();
                    } else {
                        object::<gtk::SearchEntry>(&a.ui, "search").grab_focus();
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(key);
    }

    fn setup_drawing(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        object::<gtk::DrawingArea>(&self.ui, "empty_art").set_draw_func(move |_, cr, w, h| {
            let Some(app) = weak.upgrade() else { return };
            let theme = app.theme.borrow();
            let (x, y) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
            let [r, g, b] = theme.color("omp_raised");
            cr.set_source_rgb(r, g, b);
            cr.arc(x, y, 74.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            let [r, g, b] = theme.color("omp_outline");
            cr.set_source_rgb(r, g, b);
            cr.set_line_width(1.0);
            for r in [36.0, 43.0, 50.0, 57.0, 64.0] {
                cr.arc(x, y, r, 0.0, std::f64::consts::TAU);
                let _ = cr.stroke();
            }
            let [r, g, b] = theme.color("omp_accent");
            cr.set_source_rgb(r, g, b);
            cr.arc(x, y, 27.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            let [r, g, b] = theme.color("omp_accent_ink");
            cr.set_source_rgb(r, g, b);
            cr.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        self.setup_visualization();
        let weak = Rc::downgrade(self);
        object::<gtk::DrawingArea>(&self.ui, "detail_backdrop").set_draw_func(
            move |_, cr, w, h| {
                let Some(a) = weak.upgrade() else {
                    return;
                };
                let (r, g, b) = a.backdrop.get();
                let gradient =
                    gtk::cairo::LinearGradient::new(0.0, 0.0, f64::from(w), f64::from(h));
                gradient.add_color_stop_rgb(0.0, r * 0.8, g * 0.8, b * 0.8);
                gradient.add_color_stop_rgb(1.0, r * 0.3, g * 0.3, b * 0.3);
                let _ = cr.set_source(&gradient);
                let _ = cr.paint();
                if let Some(art) = a.backdrop_art.borrow().as_ref() {
                    use gdk::prelude::GdkCairoContextExt;
                    let _ = cr.save();
                    cr.scale(
                        f64::from(w) / f64::from(art.width()),
                        f64::from(h) / f64::from(art.height()),
                    );
                    cr.set_source_pixbuf(art, 0.0, 0.0);
                    cr.source().set_filter(gtk::cairo::Filter::Bilinear);
                    let _ = cr.paint_with_alpha(0.45);
                    let _ = cr.restore();
                }
            },
        );
    }

    fn artist(&self, track: &Track) -> String {
        if track.artist.is_empty() {
            self.tr("Unknown artist")
        } else {
            track.artist.clone()
        }
    }

    fn set_art(&self, picture: &gtk::Picture, path: Option<&str>) {
        self.images.set(picture, path);
    }

    pub fn navigate(self: &Rc<Self>, page: &str) {
        if ![
            "search",
            "library",
            "favorites",
            "playlists",
            "podcasts",
            "radio",
            "streams",
            "youtube",
            "spotify",
            "queue",
            "downloads",
        ]
        .contains(&page)
        {
            return;
        }
        *self.page.borrow_mut() = page.into();
        self.album.borrow_mut().take();
        self.artists.selected.borrow_mut().take();
        self.playlist.borrow_mut().take();
        object::<gtk::SearchEntry>(&self.ui, "search").set_text("");
        object::<gtk::Box>(&self.ui, "entry_bar").set_visible(false);
        let nav = if ["youtube", "spotify"].contains(&page) {
            "streams"
        } else {
            page
        };
        for name in [
            "search",
            "library",
            "favorites",
            "playlists",
            "podcasts",
            "radio",
            "streams",
            "queue",
            "downloads",
        ] {
            let b = self.button(&format!("nav_{name}"));
            if name == nav {
                b.add_css_class("selected");
            } else {
                b.remove_css_class("selected");
            }
        }
        if ["streams", "youtube", "spotify"].contains(&page) {
            *self.streaming_service.borrow_mut() = page.into();
            self.updating.set(true);
            object::<gtk::ToggleButton>(&self.ui, &format!("service_{page}")).set_active(true);
            self.updating.set(false);
        }
        self.refresh();
        if page == "search" {
            self.focus_global_search();
        }
    }

    fn primary_action(self: &Rc<Self>) {
        let page = self.page.borrow().clone();
        match page.as_str() {
            "youtube" => self.youtube_primary(),
            "spotify" => self.spotify_primary(),
            "radio" => self.discover_radio(),
            "podcasts" | "streams" => {
                object::<gtk::Box>(&self.ui, "entry_bar").set_visible(true);
                let entry: gtk::Entry = object(&self.ui, "url_entry");
                entry.set_placeholder_text(Some(&self.tr(if page == "podcasts" {
                    "Paste a podcast RSS feed URL"
                } else {
                    "Paste an audio stream URL"
                })));
                entry.grab_focus();
            }
            "playlists" => self.playlist_dialog(None, self.playlist.borrow().clone()),
            "queue" => {
                if self.visible.borrow().is_empty() {
                    self.navigate("library");
                } else {
                    self.cancel_discography();
                    self.state.borrow_mut().queue.clear();
                    self.persist();
                    self.refresh();
                }
            }
            _ => self.choose_folder(),
        }
    }

    pub fn notice(&self, text: &str, error: bool, busy: bool) {
        self.buffering_notice.set(false);
        if *self.page.borrow() == "search" {
            self.global_notice(text, error);
            return;
        }
        let notice: gtk::Box = object(&self.ui, "notice");
        notice.set_visible(true);
        if error {
            notice.add_css_class("error");
        } else {
            notice.remove_css_class("error");
        }
        self.label("notice_text", &self.tr(text));
        let spinner: gtk::Spinner = object(&self.ui, "busy_spinner");
        spinner.set_visible(busy);
        spinner.set_spinning(busy);
    }

    fn choose_folder(self: &Rc<Self>) {
        let dialog = gtk::FileChooserNative::new(
            Some(&self.tr("Choose a music folder")),
            Some(&self.window),
            gtk::FileChooserAction::SelectFolder,
            Some(&self.tr("Add music")),
            Some(&self.tr("Cancel")),
        );
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept
                && let (Some(app), Some(path)) =
                    (weak.upgrade(), dialog.file().and_then(|f| f.path()))
            {
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                let mut folders = app.state.borrow().folders.clone();
                if !folders.contains(&path) {
                    folders.push(path);
                }
                app.scan(folders);
            }
            dialog.destroy();
        });
        dialog.show();
    }

    pub fn scan(self: &Rc<Self>, folders: Vec<PathBuf>) {
        self.state.borrow_mut().folders = folders.clone();
        self.sync_folder_watch();
        self.persist();
        self.scan_folders(folders, false);
    }

    fn scan_folders(self: &Rc<Self>, folders: Vec<PathBuf>, background: bool) {
        if self.scanning.get() || self.files.importing.get() || self.tags.busy.get() {
            self.files.scan_pending.set(true);
            if !background {
                self.files.scan_manual.set(true);
            }
            return;
        }
        self.scanning.set(true);
        self.scan_background.set(background);
        if !background {
            self.notice(&self.tr("Finding your music…"), false, true);
        }
        self.button("refresh").set_sensitive(false);
        let existing = self.state.borrow().tracks.clone();
        let ignored = self.state.borrow().ignored_local.clone();
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = library::scan_filtered(&folders, &existing, &cache, &ignored);
            let _ = tx.send(Message::Scan(result, folders));
        });
    }

    fn refresh_sources(self: &Rc<Self>) {
        let page = self.page.borrow().clone();
        match page.as_str() {
            "youtube" => self.refresh_youtube(),
            "spotify" => self.refresh_spotify(),
            "radio" => self.discover_radio(),
            "podcasts" if self.podcast_discovery.get() => self.discover_podcasts(),
            "playlists" => self.refresh(),
            "podcasts" => {
                let subscriptions = self.state.borrow().subscriptions.clone();
                if subscriptions.is_empty() {
                    self.primary_action();
                    return;
                }
                self.notice(&self.tr("Refreshing podcasts…"), false, true);
                self.refresh_podcasts(false);
            }
            _ => {
                let folders = self.state.borrow().folders.clone();
                if folders.is_empty() {
                    self.choose_folder();
                } else {
                    self.scan(folders);
                }
            }
        }
    }

    fn discover_radio(self: &Rc<Self>) {
        self.radio_refreshing.set(true);
        let query = object::<gtk::SearchEntry>(&self.ui, "search")
            .text()
            .to_string();
        let generation = self.source_generation.get() + 1;
        self.source_generation.set(generation);
        let tx = self.tx.clone();
        let cache = self.cache.clone();
        self.notice(&self.tr("Finding stations…"), false, true);
        std::thread::spawn(move || {
            let _ = tx.send(Message::Radio(generation, services::radio(&query, &cache)));
        });
    }

    fn submit_url(self: &Rc<Self>) {
        let entry: gtk::Entry = object(&self.ui, "url_entry");
        let input = entry.text().trim().to_string();
        let url = match services::http_url(&input) {
            Ok(u) => u.to_string(),
            Err(e) => {
                self.notice(&e, true, false);
                return;
            }
        };
        if *self.page.borrow() == "podcasts" {
            self.subscribe(&url);
        } else {
            let title = url::Url::parse(&url)
                .ok()
                .and_then(|u| u.host_str().map(String::from))
                .unwrap_or_else(|| "Stream".into());
            let track = Track::new(&url, &title, Kind::Stream);
            let id = track.id.clone();
            self.state.borrow_mut().merge(vec![track]);
            self.state.borrow_mut().queue = vec![id.clone()];
            self.play_track(&id, None);
        }
        entry.set_text("");
        object::<gtk::Box>(&self.ui, "entry_bar").set_visible(false);
    }

    fn toggle_details(&self) {
        let panel: gtk::Revealer = object(&self.ui, "detail_revealer");
        let visible = !panel.is_visible();
        panel.set_visible(visible);
        panel.set_reveal_child(visible);
        self.schedule_metadata();
        self.update_metadata();
        if self.window.width() < 1100 {
            object::<gtk::ScrolledWindow>(&self.ui, "sidebar_scroller").set_visible(false);
        }
    }

    fn start_visible(self: &Rc<Self>, id: &str) {
        if *self.page.borrow() == "queue" {
            self.play_track(id, None);
            return;
        }
        let mut queue = self.visible.borrow().clone();
        if self.state.borrow().shuffle {
            queue.retain(|i| i != id);
            model::shuffle(&mut queue);
            queue.insert(0, id.into());
        }
        self.state.borrow_mut().queue = queue;
        self.play_track(id, None);
    }

    pub fn play_track(self: &Rc<Self>, id: &str, resume: Option<f64>) {
        self.cancel_discography_playback();
        self.clear_visualization();
        let track = self.state.borrow().track(id).cloned();
        let Some(track) = track else {
            return;
        };
        self.set_buffering(100);
        self.remember_position();
        let generation = self.play_generation.get() + 1;
        self.play_generation.set(generation);
        self.resolving.set(false);
        self.resolve_autoplay.set(true);
        *self.preparation.borrow_mut() = playback::Preparation::default();
        let position = resume.unwrap_or_else(|| {
            if track.kind == Kind::Podcast {
                self.state
                    .borrow()
                    .podcast_positions
                    .get(id)
                    .copied()
                    .unwrap_or(0.0)
            } else {
                0.0
            }
        });
        {
            let mut state = self.state.borrow_mut();
            state.current = Some(id.into());
            state.position = position;
            if !state.queue.contains(&id.to_string()) {
                state.queue.push(id.into());
            }
        }
        if track.kind == Kind::Spotify {
            self.play_spotify(&track, position, generation);
        } else if services::youtube_url(&track.uri) || track.download_path.is_some() {
            self.audio.borrow_mut().stop();
            self.resolving.set(true);
            self.notice(
                &self.tr(if track.download_path.is_some() {
                    "Opening saved audio…"
                } else {
                    "Opening YouTube Music…"
                }),
                false,
                true,
            );
            let account = self.state.borrow().youtube_account.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Message::Youtube(
                    generation,
                    if let Some(path) = track.download_path.filter(|p| p.is_file()) {
                        Ok(youtube_music::Resolved {
                            uri: gio::File::for_path(path).uri().to_string(),
                            headers: HashMap::new(),
                        })
                    } else if services::youtube_url(&track.uri) {
                        youtube_music::resolve(account.as_ref(), &track.uri)
                    } else {
                        Ok(youtube_music::Resolved {
                            uri: track.uri,
                            headers: HashMap::new(),
                        })
                    },
                ));
            });
        } else {
            let (next, fade) = self.next_playback();
            let result = self.audio.borrow_mut().load_queued(
                &track.uri,
                position,
                true,
                HashMap::new(),
                next,
                fade,
            );
            if let Err(e) = result {
                self.notice(&e, true, false);
            }
        }
        self.ensure_artwork();
        self.update_player();
        self.refresh();
        self.persist();
    }

    pub fn toggle_play(self: &Rc<Self>) {
        self.cancel_discography_playback();
        if self.resolving.get() {
            self.resolve_autoplay.set(!self.resolve_autoplay.get());
        } else if self.audio.borrow().loaded {
            let playing = !self.audio.borrow().playing;
            let result = self.audio.borrow_mut().set_playing(playing);
            if let Err(e) = result {
                self.notice(&e, true, false);
            }
        } else {
            let state = self.state.borrow();
            let id = state
                .current
                .clone()
                .or_else(|| state.queue.first().cloned())
                .or_else(|| self.visible.borrow().first().cloned());
            let position = state.position;
            drop(state);
            if let Some(id) = id {
                self.play_track(&id, Some(position));
            } else {
                self.notice(&self.tr("Add music to start listening."), false, false);
            }
        }
        self.update_player();
        self.persist();
    }

    pub fn playback_active(&self) -> bool {
        self.audio.borrow().playing || (self.resolving.get() && self.resolve_autoplay.get())
    }

    pub fn playback_pending(&self) -> bool {
        self.resolving.get()
    }

    pub fn pause_playback(&self) {
        self.resolve_autoplay.set(false);
        let _ = self.audio.borrow_mut().set_playing(false);
        self.update_player();
    }

    pub fn stop_playback(&self) {
        self.cancel_discography_playback();
        self.clear_visualization();
        self.set_buffering(100);
        self.play_generation.set(self.play_generation.get() + 1);
        self.resolving.set(false);
        self.audio.borrow_mut().stop();
    }

    pub fn next(self: &Rc<Self>, automatic: bool) {
        let next = self.state.borrow().adjacent(true, automatic);
        if let Some(id) = next {
            self.play_track(&id, Some(0.0));
        } else if automatic {
            self.remember_position();
            self.audio.borrow_mut().stop();
            self.state.borrow_mut().position = 0.0;
            self.update_player();
            self.persist();
        }
    }
    pub fn previous(self: &Rc<Self>) {
        if self.audio.borrow().position() > 3.0 {
            self.seek(0.0);
            return;
        }
        let previous = self.state.borrow().adjacent(false, false);
        if let Some(id) = previous {
            self.play_track(&id, Some(0.0));
        }
    }
    pub fn seek(&self, seconds: f64) {
        let duration = self.audio.borrow().duration();
        let seconds = if duration > 0.0 {
            seconds.clamp(0.0, duration)
        } else {
            seconds.max(0.0)
        };
        let result = self.audio.borrow_mut().seek(seconds);
        match result {
            Ok(()) => {
                self.state.borrow_mut().position = seconds;
                crate::desktop::seeked(self, seconds);
            }
            Err(e) => self.notice(&e, true, false),
        }
    }
    pub fn set_volume(&self, value: f64) {
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.75
        };
        self.state.borrow_mut().volume = value;
        self.audio.borrow().volume(value);
        self.updating.set(true);
        object::<gtk::Scale>(&self.ui, "volume").set_value(value);
        self.updating.set(false);
        self.button("mute").set_icon_name(if value == 0.0 {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        });
        self.persist();
        crate::desktop::changed(self);
    }

    pub fn set_shuffle(&self, enabled: bool) {
        let mut state = self.state.borrow_mut();
        state.shuffle = enabled;
        if enabled {
            let current = state.current.clone();
            if let Some(id) = &current {
                state.queue.retain(|i| i != id);
            }
            model::shuffle(&mut state.queue);
            if let Some(id) = current {
                state.queue.insert(0, id);
            }
        }
        drop(state);
        self.updating.set(true);
        object::<gtk::ToggleButton>(&self.ui, "shuffle").set_active(enabled);
        self.updating.set(false);
        self.persist();
        self.refresh();
        crate::desktop::changed(self);
    }

    fn favorite(&self, id: &str) {
        if let Some(t) = self
            .state
            .borrow_mut()
            .tracks
            .iter_mut()
            .find(|t| t.id == id)
        {
            t.favorite = !t.favorite;
        }
        self.refresh();
        self.update_player();
        self.persist();
    }

    fn remove(&self, id: &str) {
        let page = self.page.borrow().clone();
        if page == "queue" {
            self.state.borrow_mut().queue.retain(|i| i != id);
        } else if page == "playlists"
            || (["youtube", "spotify"].contains(&page.as_str()) && self.playlist.borrow().is_some())
        {
            if let Some(p) = self
                .state
                .borrow_mut()
                .playlists
                .iter_mut()
                .find(|p| Some(&p.id) == self.playlist.borrow().as_ref())
            {
                p.tracks.retain(|i| i != id);
            }
        } else {
            if self.state.borrow().current.as_deref() == Some(id) {
                self.stop_playback();
            }
            self.state
                .borrow_mut()
                .remove_from_library(&[id.to_string()]);
        }
        self.refresh();
        self.update_player();
        self.persist();
        self.notice(
            &self.tr("Removed from this list. Your files are unchanged."),
            false,
            false,
        );
    }

    fn remember_position(&self) {
        if !self.audio.borrow().loaded {
            return;
        }
        let position = self.audio.borrow().position();
        let mut state = self.state.borrow_mut();
        state.position = position;
        if let Some(track) = state
            .current_track()
            .filter(|t| t.kind == Kind::Podcast)
            .cloned()
        {
            state.podcast_positions.insert(track.id, position);
        }
    }

    pub fn persist(&self) {
        self.remember_position();
        if let Some(tx) = self.save_tx.borrow().as_ref() {
            let _ = tx.send(self.state.borrow().clone());
        }
        self.last_saved.set(Instant::now());
    }

    pub fn shutdown(&self) {
        self.cancel_global_search();
        self.shutdown_tags();
        self.background_hold.borrow_mut().take();
        self.theme.borrow_mut().detach();
        self.cancel_discography();
        self.files.watcher.stop();
        self.finish_trash();
        self.metadata.borrow().stop();
        self.audio.borrow_mut().stop();
        self.downloads.manager.borrow_mut().shutdown();
        self.collect_downloads();
        self.persist();
        self.audio.borrow_mut().detach_spotify();
        self.spotify.service.send(crate::spotify::Command::Shutdown);
        self.save_tx.borrow_mut().take();
        if let Some(worker) = self.save_worker.borrow_mut().take() {
            let _ = worker.join();
        }
    }
    pub fn quit(&self) {
        self.persist();
        self.background_hold.borrow_mut().take();
        self.application.quit();
    }
    pub fn show(&self) {
        self.background_hold.borrow_mut().take();
        object::<gtk::Window>(&self.mini_ui, "mini_window").hide();
        self.window.present();
    }
    fn mini(&self) {
        self.background_hold.borrow_mut().take();
        object::<gtk::Window>(&self.mini_ui, "mini_window").present();
        self.window.hide();
    }

    pub fn update_player(&self) {
        let state = self.state.borrow();
        let track = state.current_track();
        let playing = self.playback_active();
        let title = track
            .map(|t| t.title.clone())
            .unwrap_or_else(|| self.tr("Nothing playing"));
        let artist = track
            .map(|t| self.artist(t))
            .unwrap_or_else(|| self.tr("Pick something you love."));
        self.label("player_title", &title);
        self.label("player_artist", &artist);
        self.label("detail_title", &title);
        self.label("detail_artist", &artist);
        object::<gtk::Label>(&self.mini_ui, "mini_title").set_label(&title);
        object::<gtk::Label>(&self.mini_ui, "mini_artist").set_label(&artist);
        let icon = if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        self.button("play").set_icon_name(icon);
        object::<gtk::Button>(&self.mini_ui, "mini_play").set_icon_name(icon);
        self.button("player_favorite")
            .set_sensitive(track.is_some());
        self.set_favorite_button(
            &object::<gtk::ToggleButton>(&self.ui, "player_favorite"),
            track.is_some_and(|t| t.favorite),
        );
        self.button("repeat")
            .set_icon_name(if state.repeat == Repeat::One {
                "media-playlist-repeat-song-symbolic"
            } else {
                "media-playlist-repeat-symbolic"
            });
        self.button("repeat")
            .set_tooltip_text(Some(&self.tr(match state.repeat {
                Repeat::Off => "Repeat off",
                Repeat::All => "Repeat all",
                Repeat::One => "Repeat one",
            })));
        if state.repeat != Repeat::Off {
            self.button("repeat").add_css_class("active");
        } else {
            self.button("repeat").remove_css_class("active");
        }
        let art = track.and_then(|t| t.artwork.as_deref());
        for id in ["player_art", "detail_art", "visualization_art"] {
            self.set_art(&object::<gtk::Picture>(&self.ui, id), art);
        }
        self.set_art(&object::<gtk::Picture>(&self.mini_ui, "mini_art"), art);
        self.update_backdrop();
        drop(state);
        self.schedule_metadata();
        self.update_metadata();
        self.update_save_control();
        crate::desktop::changed(self);
    }

    fn set_favorite_button(&self, button: &gtk::ToggleButton, active: bool) {
        button.set_active(active);
        button.set_icon_name(if active {
            "starred-symbolic"
        } else {
            "open-music-star-outline-symbolic"
        });
        let label = self.tr(if active {
            "Remove from favorites"
        } else {
            "Add to favorites"
        });
        button.set_tooltip_text(Some(&label));
        button.update_property(&[gtk::accessible::Property::Label(&label)]);
    }

    fn ensure_artwork(&self) {
        let urls: Vec<String> = self
            .global_artwork_urls()
            .into_iter()
            .chain(
                self.podcast_results
                    .borrow()
                    .iter()
                    .filter_map(|s| s.artwork.as_ref())
                    .cloned()
                    .chain(
                        self.state
                            .borrow()
                            .tracks
                            .iter()
                            .filter_map(|t| t.artwork.as_ref())
                            .cloned(),
                    ),
            )
            .filter(|u| u.starts_with("http"))
            .take(60)
            .collect();
        let mut pending = self.artwork_pending.borrow_mut();
        let urls: Vec<String> = urls
            .into_iter()
            .filter(|u| pending.insert(u.clone()))
            .collect();
        if urls.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let cache = self.cache.clone();
        std::thread::spawn(move || {
            for uri in urls {
                let result = services::artwork(&uri, &cache);
                let _ = tx.send(Message::Artwork(uri, result));
            }
        });
    }

    fn tick(self: &Rc<Self>) {
        self.poll_global_search();
        self.poll_tags();
        if self.images.take_changed() {
            self.update_backdrop();
        }
        let compact = self.window.width() < 1000 || self.window.height() < 700;
        if self.compact_browse.replace(compact) != compact {
            self.layout_player(compact);
            self.refresh();
            self.update_metadata();
        }
        let messages: Vec<Message> = self.rx.borrow().try_iter().take(30).collect();
        for message in messages {
            match message {
                Message::PodcastSearch(generation, result)
                    if generation == self.podcast_generation.get() =>
                {
                    self.button("podcast_search").set_sensitive(true);
                    match result {
                        Ok(shows) => {
                            *self.podcast_results.borrow_mut() = shows;
                            if *self.page.borrow() == "podcasts" && self.podcast_discovery.get() {
                                object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                            }
                            self.refresh();
                            self.ensure_artwork();
                        }
                        Err(_) => {
                            self.podcast_failed.set(true);
                            object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                            self.refresh();
                        }
                    }
                }
                Message::PlaylistImport(result) => {
                    self.importing_playlist.set(false);
                    self.button("playlist_import").set_sensitive(true);
                    match result {
                        Ok(imported) => {
                            let mut state = self.state.borrow_mut();
                            let name = state.unique_playlist_name(&imported.name);
                            let ids = imported.tracks.iter().map(|t| t.id.clone()).collect();
                            state.merge(imported.tracks);
                            let id = state
                                .create_playlist(&name, ids)
                                .expect("Unique playlist name");
                            drop(state);
                            self.open_playlist(&id);
                            self.notice(
                                &format!(
                                    "{} · {} {}",
                                    self.tr("Playlist imported"),
                                    imported.skipped,
                                    self.tr("entries skipped")
                                ),
                                false,
                                false,
                            );
                            self.persist();
                        }
                        Err(error) => self.notice(&error, true, false),
                    }
                }
                Message::LocalImport(result) => self.imported_local(result),
                Message::AudioDetails(id, uri, Ok(details)) => {
                    if let Some(track) = self
                        .state
                        .borrow_mut()
                        .tracks
                        .iter_mut()
                        .find(|t| t.id == id && t.uri == uri && t.audio_details.codec.is_none())
                    {
                        track.audio_details = details;
                    }
                    self.persist();
                    self.update_metadata();
                }
                Message::Scan(result, folders) => {
                    let background = self.scan_background.replace(false);
                    self.scanning.set(false);
                    self.button("refresh").set_sensitive(true);
                    if folders != self.state.borrow().folders {
                        self.files.scan_pending.set(true);
                        continue;
                    }
                    match result {
                        Ok(scan) => {
                            let count = scan.tracks.len();
                            let removed = scan.removed;
                            let newly_unavailable = !scan.unavailable.is_empty()
                                && *self.files.unavailable.borrow() != scan.unavailable;
                            *self.files.unavailable.borrow_mut() = scan.unavailable;
                            {
                                let mut state = self.state.borrow_mut();
                                if state
                                    .current
                                    .as_ref()
                                    .is_some_and(|id| removed.contains(id))
                                {
                                    self.audio.borrow_mut().stop();
                                }
                                state.forget_many(removed.iter().map(String::as_str));
                                state.merge(scan.tracks);
                            }
                            if !background {
                                self.notice(
                                    &format!(
                                        "{} {} · {} {}",
                                        count,
                                        self.tr("tracks found"),
                                        scan.skipped,
                                        self.tr("files skipped")
                                    ),
                                    false,
                                    false,
                                );
                            }
                            if newly_unavailable {
                                self.notice(&self.tr("Some music folders are unavailable. Their library entries were kept."), true, false);
                            } else if self.files.unavailable.borrow().is_empty()
                                && object::<gtk::Label>(&self.ui, "notice_text").text()
                                    == self.tr("Some music folders are unavailable. Their library entries were kept.")
                            {
                                object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                            }
                            self.refresh();
                            self.update_player();
                            self.persist();
                        }
                        Err(e) => self.notice(&e, true, false),
                    }
                }
                Message::Radio(generation, result)
                    if generation == self.source_generation.get() =>
                {
                    self.radio_refreshing.set(false);
                    match result {
                        Ok(tracks) => {
                            *self.radio_results.borrow_mut() =
                                tracks.iter().map(|t| t.id.clone()).collect();
                            self.state.borrow_mut().merge(tracks);
                            object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                            self.refresh();
                            self.ensure_artwork();
                            self.persist();
                        }
                        Err(e) => self.notice(&e, true, false),
                    }
                }
                Message::Feed(reply) => self.handle_feed(reply),
                Message::Youtube(generation, result)
                    if generation == self.play_generation.get() =>
                {
                    self.resolving.set(false);
                    match result {
                        Ok(resolved) => {
                            let position = self.state.borrow().position;
                            let (next, fade) = self.next_playback();
                            let result = self.audio.borrow_mut().load_queued(
                                &resolved.uri,
                                position,
                                self.resolve_autoplay.get(),
                                resolved.headers,
                                next,
                                fade,
                            );
                            if let Err(e) = result {
                                self.notice(&e, true, false);
                            } else {
                                object::<gtk::Box>(&self.ui, "notice").set_visible(false);
                            }
                            self.refresh();
                            self.ensure_artwork();
                            self.update_player();
                            self.persist();
                        }
                        Err(e) => {
                            self.notice(&e, true, false);
                            self.update_player();
                        }
                    }
                }
                Message::Prepared(key, result) => self.prepared(key, result),
                Message::Music(reply) => self.handle_youtube(reply),
                Message::Discography(id, provider, result) => {
                    self.handle_discography(id, provider, result)
                }
                Message::Artwork(uri, result) => {
                    self.artwork_pending.borrow_mut().remove(&uri);
                    let Ok(path) = result else {
                        self.global_artwork_failed(&uri);
                        continue;
                    };
                    self.global_artwork(&uri, &path);
                    for show in self.podcast_results.borrow_mut().iter_mut() {
                        if show.artwork.as_deref() == Some(&uri) {
                            show.artwork = Some(path.clone());
                        }
                    }
                    for track in &mut self.state.borrow_mut().tracks {
                        if track.artwork.as_deref() == Some(&uri) {
                            track.artwork = Some(path.clone());
                        }
                    }
                    if *self.page.borrow() != "search" {
                        self.refresh();
                    }
                    self.update_player();
                }
                Message::SaveError(e) => self.notice(&e, true, false),
                _ => {}
            }
        }
        for reply in self.spotify.service.poll() {
            self.handle_spotify(reply);
        }
        self.poll_metadata();
        self.poll_source_refresh();
        self.poll_files();
        self.poll_downloads();
        let events = self.audio.borrow_mut().poll();
        for event in events {
            match event {
                Event::Advanced(id) => {
                    if !self.advanced(id) {
                        break;
                    }
                }
                Event::End => {
                    self.remember_position();
                    self.audio.borrow_mut().stop();
                    let id = self.state.borrow().current.clone();
                    if let Some(id) = id {
                        self.state.borrow_mut().podcast_positions.remove(&id);
                    }
                    self.next(true);
                    break;
                }
                Event::Error(e) => {
                    self.notice(
                        &format!(
                            "{} {e}",
                            self.tr("Playback failed. Try another track or check your connection.")
                        ),
                        true,
                        false,
                    );
                    self.update_player();
                }
                Event::Ready => {
                    let duration = self.audio.borrow().duration();
                    let mut state = self.state.borrow_mut();
                    let current = state.current.clone();
                    let changed = if let Some(track) = state
                        .tracks
                        .iter_mut()
                        .find(|t| Some(&t.id) == current.as_ref())
                    {
                        if duration > 0.0 && track.duration != duration {
                            track.duration = duration;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    drop(state);
                    if changed {
                        self.update_player();
                    }
                }
                Event::Buffering(percent) => {
                    self.set_buffering(percent);
                }
            }
        }
        self.prepare_transition();
        self.sync_visualization();
        self.update_music_theme();
        self.update_buffering_notice();
        let audio = self.audio.borrow();
        let duration = if audio.loaded {
            audio.duration()
        } else {
            self.state
                .borrow()
                .current_track()
                .map(|t| t.duration)
                .unwrap_or(0.0)
        };
        let position = if audio.loaded {
            audio.position()
        } else {
            self.state.borrow().position
        };
        let seek: gtk::Scale = object(&self.ui, "seek");
        seek.set_range(0.0, duration.max(1.0));
        seek.set_value(position);
        seek.set_sensitive(audio.loaded && duration > 0.0);
        self.update_timing();
        self.label("duration", &model::duration(duration));
        if audio.playing && self.last_saved.get().elapsed() > Duration::from_secs(10) {
            drop(audio);
            self.persist();
        }
    }

    fn settings(self: &Rc<Self>) {
        let ui = gtk::Builder::from_string(include_str!("../resources/settings.ui"));
        self.i18n.builder(&ui);
        let window: gtk::Window = object(&ui, "settings_window");
        close_on_escape(&window);
        window.set_transient_for(Some(&self.window));
        window.set_application(Some(&self.application));
        let language: gtk::DropDown = object(&ui, "language");
        let language_names: Vec<_> = crate::i18n::LANGUAGES
            .iter()
            .map(|(_, name)| self.tr(name))
            .collect();
        language.set_model(Some(&gtk::StringList::new(
            &language_names
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )));
        language.set_selected(
            crate::i18n::LANGUAGES
                .iter()
                .position(|(code, _)| *code == self.state.borrow().language)
                .unwrap_or(0) as u32,
        );
        let weak = Rc::downgrade(self);
        let note: gtk::Label = object(&ui, "restart_note");
        language.connect_selected_notify(move |picker| {
            if let Some(a) = weak.upgrade()
                && let Some((code, _)) = crate::i18n::LANGUAGES.get(picker.selected() as usize)
            {
                a.state.borrow_mut().language = (*code).into();
                a.persist();
                note.set_visible(true);
            }
        });
        let background: gtk::Switch = object(&ui, "background");
        background.set_active(self.state.borrow().background);
        let weak = Rc::downgrade(self);
        background.connect_active_notify(move |switch| {
            if let Some(a) = weak.upgrade() {
                a.state.borrow_mut().background = switch.is_active();
                a.persist();
            }
        });
        self.preference_settings(&ui);
        self.metadata_settings(&ui);
        self.refresh_settings(&ui);
        self.playback_settings(&ui);
        self.file_settings(&ui);
        self.download_settings(&ui);
        for folder in self.state.borrow().folders.clone() {
            let builder = gtk::Builder::from_string(include_str!("../resources/setting-row.ui"));
            self.i18n.builder(&builder);
            let row: gtk::Box = object(&builder, "row");
            object::<gtk::Label>(&builder, "name").set_label(&folder.to_string_lossy());
            row.set_tooltip_text(Some(&folder.to_string_lossy()));
            object::<gtk::ListBox>(&ui, "folders").append(&row);
            let weak = Rc::downgrade(self);
            let weak_row = row.downgrade();
            object::<gtk::Button>(&builder, "remove").connect_clicked(move |_| {
                if let Some(a) = weak.upgrade() {
                    a.state.borrow_mut().folders.retain(|f| f != &folder);
                    a.sync_folder_watch();
                    a.persist();
                    if let Some(row) = weak_row.upgrade() {
                        row.set_sensitive(false);
                    }
                    a.notice(
                        &a.tr("Folder removed from scanning. Your music is unchanged."),
                        false,
                        false,
                    );
                }
            });
        }
        for sub in self.state.borrow().subscriptions.clone() {
            let builder = gtk::Builder::from_string(include_str!("../resources/setting-row.ui"));
            self.i18n.builder(&builder);
            let row: gtk::Box = object(&builder, "row");
            object::<gtk::Label>(&builder, "name").set_label(&sub.title);
            row.set_tooltip_text(Some(&sub.url));
            object::<gtk::ListBox>(&ui, "subscriptions").append(&row);
            let weak = Rc::downgrade(self);
            let weak_row = row.downgrade();
            object::<gtk::Button>(&builder, "remove").connect_clicked(move |_| {
                if let Some(a) = weak.upgrade() {
                    a.unsubscribe(&sub.url);
                    if let Some(row) = weak_row.upgrade() {
                        row.set_sensitive(false);
                    }
                }
            });
        }
        let weak = Rc::downgrade(self);
        let weak_window = window.downgrade();
        object::<gtk::Button>(&ui, "add_folder").connect_clicked(move |_| {
            if let Some(w) = weak_window.upgrade() {
                w.close();
            }
            if let Some(a) = weak.upgrade() {
                a.choose_folder();
            }
        });
        let weak_window = window.downgrade();
        object::<gtk::Button>(&ui, "close").connect_clicked(move |_| {
            if let Some(w) = weak_window.upgrade() {
                w.close();
            }
        });
        window.present();
    }

    fn shortcuts(&self) {
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .text(self.tr("Keyboard shortcuts"))
            .secondary_text(format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                self.tr("Ctrl + Space — Play / pause"),
                self.tr("Alt + Left / Right — Previous / next"),
                self.tr("Ctrl + O — Add music"),
                self.tr("Ctrl + F — Search"),
                self.tr("Ctrl + K — Search all audio"),
                self.tr("Ctrl + M — Mini player"),
                self.tr("Ctrl + , — Settings"),
                self.tr("Ctrl + Q — Quit"),
                self.tr("Ctrl / Shift + click — Select multiple tracks"),
                self.tr("Ctrl + A / Escape — Select all / clear selection"),
                self.tr("Ctrl + Shift + Up / Down — Reorder selected tracks")
            ))
            .buttons(gtk::ButtonsType::Close)
            .build();
        dialog.connect_response(|dialog, _| dialog.close());
        dialog.present();
    }
}
