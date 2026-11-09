use super::*;

pub(super) fn exercise(uri: &str) {
    crate::app::render_area::RenderArea::static_type();
    // Menu buttons are not GtkButton subclasses. Exercise the actual selection
    // toolbar in a language where "Actions" differs from its English spelling.
    let ui = gtk::Builder::from_string(include_str!("../../../resources/window.ui"));
    crate::i18n::I18n::new("es").builder(&ui);
    assert_eq!(
        object::<gtk::MenuButton>(&ui, "selection_actions")
            .label()
            .as_deref(),
        Some("Acciones")
    );
    object::<gtk::Window>(&ui, "window").destroy();

    let track = Track::new(uri, "Playback speed", Kind::Music);
    let id = track.id.clone();
    let state = State {
        language: "fr".into(),
        tracks: vec![track],
        fetch_covers: false,
        fetch_lyrics: false,
        fetch_information: false,
        ..State::default()
    };
    let application = gtk::Application::builder()
        .application_id("io.github.openmusic.TranslationTest")
        .build();
    application.register(None::<&gio::Cancellable>).unwrap();
    let app = App::new(&application, state).unwrap();
    settle();
    let speed: gtk::MenuButton = object(&app.ui, "playback_speed");
    assert_eq!(speed.tooltip_text().as_deref(), Some("Vitesse de lecture"));
    assert_eq!(
        app.button("time_toggle").tooltip_text().as_deref(),
        Some("Afficher le temps restant")
    );
    app.play_track(&id, None);
    until(|| app.audio.borrow().position() > 0.2);
    assert_eq!(
        object::<gtk::Label>(&app.ui, "player_title").text(),
        "Playback speed",
        "User track titles must not be translated"
    );
    app.button("time_toggle").emit_clicked();
    assert_eq!(
        app.button("time_toggle").tooltip_text().as_deref(),
        Some("Afficher le temps écoulé")
    );
    app.stop_playback();
    // A stopped deck cannot seek. Mark it loaded to exercise the unavailable
    // source branch without a live radio server or a Spotify account.
    app.audio.borrow_mut().loaded = true;
    app.update_timing();
    assert!(!speed.is_sensitive());
    assert_eq!(
        speed.tooltip_text().as_deref(),
        Some("La vitesse de lecture n’est pas réglable pour cette source.")
    );
    app.audio.borrow_mut().loaded = false;
    app.update_timing();
    assert!(speed.is_sensitive());
    assert_eq!(speed.tooltip_text().as_deref(), Some("Vitesse de lecture"));

    app.navigate("downloads");
    settle();
    assert_eq!(
        object::<gtk::Label>(&app.ui, "page_title").text(),
        "Téléchargements"
    );
    app.notice("This playlist contains no tracks", true, false);
    assert_eq!(
        object::<gtk::Label>(&app.ui, "notice_text").text(),
        "Cette playlist ne contient aucun titre"
    );
    screenshot(app.window.upcast_ref(), "i18n-fr-downloads.png");
    app.navigate("search");
    app.notice("The request timed out", true, false);
    assert_eq!(
        descendant(app.window.upcast_ref(), "global_message")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap()
            .text(),
        "La requête a expiré"
    );

    app.settings();
    settle();
    let settings = window("Paramètres");
    let language = descendant(settings.upcast_ref(), "language")
        .unwrap()
        .downcast::<gtk::DropDown>()
        .unwrap();
    assert_eq!(
        language
            .model()
            .unwrap()
            .item(0)
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string(),
        "Système"
    );
    let crossfade = descendant(settings.upcast_ref(), "crossfade_seconds").unwrap();
    assert_eq!(
        crossfade.tooltip_text().as_deref(),
        Some("Durée du fondu enchaîné en secondes ; 0 pour désactiver")
    );
    settings.set_default_size(430, 560);
    settle();
    screenshot(&settings, "i18n-fr-settings-compact.png");
    let scroller = settings
        .child()
        .and_downcast::<gtk::ScrolledWindow>()
        .unwrap();
    scroller.vadjustment().set_value(600.0);
    settle();
    screenshot(&settings, "i18n-fr-settings-playback.png");
    settings.close();

    app.spotify_primary();
    app.handle_spotify(crate::spotify::Reply::Connected(Err(
        "Spotify sign-in expired. Reconnect your account.".into(),
    )));
    settle();
    let spotify = window("Compte Spotify");
    assert!(has_label(
        spotify.upcast_ref(),
        "La connexion Spotify a expiré. Reconnectez votre compte."
    ));
    screenshot(&spotify, "i18n-fr-spotify-error.png");
    spotify.close();

    app.youtube_primary();
    app.handle_youtube(crate::app::youtube::Reply::Connected(Err(
        "Your saved session is unavailable. Reconnect YouTube Music.".into(),
    )));
    settle();
    let youtube = window("Compte YouTube Music");
    assert!(has_label(
        youtube.upcast_ref(),
        "Votre session enregistrée est indisponible. Reconnectez YouTube Music."
    ));
    screenshot(&youtube, "i18n-fr-youtube-error.png");
    youtube.close();
    app.shutdown();
    app.window.destroy();
    object::<gtk::Window>(&app.mini_ui, "mini_window").destroy();
}

fn window(title: &str) -> gtk::Window {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.title().as_deref() == Some(title))
        .unwrap_or_else(|| panic!("Missing translated window: {title}"))
}

fn has_label(widget: &gtk::Widget, text: &str) -> bool {
    if widget
        .downcast_ref::<gtk::Label>()
        .is_some_and(|label| label.text() == text)
    {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if has_label(&current, text) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}
