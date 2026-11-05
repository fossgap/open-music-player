use crate::{
    app::App,
    model::{Kind, Repeat, Track},
    services,
};
use gtk::{gio, glib, prelude::*};
use std::{collections::HashMap, rc::Rc};

const ROOT: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
type Properties = HashMap<String, glib::Variant>;

fn path(id: &str) -> glib::variant::ObjectPath {
    glib::variant::ObjectPath::try_from(format!("/org/mpris/MediaPlayer2/Track/{id}"))
        .expect("Track IDs contain only alphanumeric characters")
}

fn metadata(app: &App) -> Properties {
    let state = app.state.borrow();
    let mut properties = HashMap::new();
    if let Some(t) = state.current_track() {
        properties.insert("mpris:trackid".into(), path(&t.id).to_variant());
        properties.insert(
            "mpris:length".into(),
            ((t.duration * 1e6) as i64).to_variant(),
        );
        properties.insert("xesam:title".into(), t.title.to_variant());
        properties.insert("xesam:artist".into(), vec![t.artist.clone()].to_variant());
        properties.insert("xesam:album".into(), t.album.to_variant());
        properties.insert("xesam:url".into(), t.uri.to_variant());
        if let Some(art) = &t.artwork {
            let uri = if art.starts_with("http") {
                Some(art.clone())
            } else {
                url::Url::from_file_path(art).ok().map(String::from)
            };
            if let Some(uri) = uri {
                properties.insert("mpris:artUrl".into(), uri.to_variant());
            }
        }
    }
    properties
}

fn property(app: &App, name: &str) -> glib::Variant {
    let state = app.state.borrow();
    let audio = app.audio.borrow();
    match name {
        "Identity" => "Open Audio Player".to_variant(),
        "DesktopEntry" => "io.github.openaudio.Player".to_variant(),
        "SupportedUriSchemes" => vec!["file", "http", "https"].to_variant(),
        "SupportedMimeTypes" => vec![
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/x-wav",
            "audio/mp4",
            "application/ogg",
        ]
        .to_variant(),
        "HasTrackList" => false.to_variant(),
        "PlaybackStatus" => if audio.playing {
            "Playing"
        } else if audio.loaded {
            "Paused"
        } else {
            "Stopped"
        }
        .to_variant(),
        "LoopStatus" => match state.repeat {
            Repeat::Off => "None",
            Repeat::All => "Playlist",
            Repeat::One => "Track",
        }
        .to_variant(),
        "Shuffle" => state.shuffle.to_variant(),
        "Volume" => state.volume.to_variant(),
        "Rate" | "MinimumRate" | "MaximumRate" => 1.0_f64.to_variant(),
        "Position" => ((audio.position() * 1e6) as i64).to_variant(),
        "Metadata" => metadata(app).to_variant(),
        "CanSeek" => (audio.loaded && audio.duration() > 0.0).to_variant(),
        "CanGoNext" => state.adjacent(true, false).is_some().to_variant(),
        "CanGoPrevious" => state.adjacent(false, false).is_some().to_variant(),
        "CanPlay" => (!state.tracks.is_empty()).to_variant(),
        "CanPause" => audio.loaded.to_variant(),
        _ => true.to_variant(),
    }
}

pub fn setup(app: &Rc<App>) {
    let weak = Rc::downgrade(app);
    gio::bus_own_name(
        gio::BusType::Session,
        "org.mpris.MediaPlayer2.OpenAudioPlayer",
        gio::BusNameOwnerFlags::NONE,
        move |connection, _| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            *app.dbus.borrow_mut() = Some(connection.clone());
            let info = gio::DBusNodeInfo::for_xml(include_str!("../resources/mpris.xml"))
                .expect("Invalid MPRIS definition");
            for interface in ["org.mpris.MediaPlayer2", PLAYER] {
                let weak = Rc::downgrade(&app);
                let getter = Rc::downgrade(&app);
                let setter = Rc::downgrade(&app);
                let registration = connection
                    .register_object(ROOT, &info.lookup_interface(interface).unwrap())
                    .method_call(move |_, _, _, _, method, params, invocation| {
                        let Some(app) = weak.upgrade() else {
                            invocation.return_dbus_error(
                                "org.freedesktop.DBus.Error.Failed",
                                "Player closed",
                            );
                            return;
                        };
                        match method {
                            "Raise" => app.show(),
                            "Quit" => app.quit(),
                            "PlayPause" => app.toggle_play(),
                            "Play" => {
                                if !app.audio.borrow().playing {
                                    app.toggle_play();
                                }
                            }
                            "Pause" => {
                                if app.audio.borrow().playing {
                                    app.toggle_play();
                                }
                            }
                            "Next" | "Previous" => {
                                let playing = app.audio.borrow().playing;
                                if method == "Next" {
                                    app.next(false);
                                } else {
                                    app.previous();
                                }
                                if !playing {
                                    let _ = app.audio.borrow_mut().set_playing(false);
                                    app.update_player();
                                }
                            }
                            "Stop" => {
                                app.audio.borrow_mut().stop();
                                app.state.borrow_mut().position = 0.0;
                                app.update_player();
                                app.persist();
                            }
                            "Seek" => {
                                if let Some((offset,)) = params.get::<(i64,)>() {
                                    let position =
                                        app.audio.borrow().position() + offset as f64 / 1e6;
                                    if position > app.audio.borrow().duration() {
                                        app.next(false);
                                    } else {
                                        app.seek(position.max(0.0));
                                    }
                                }
                            }
                            "SetPosition" => {
                                if let Some((id, position)) =
                                    params.get::<(glib::variant::ObjectPath, i64)>()
                                {
                                    let current =
                                        app.state.borrow().current.as_ref().map(|id| path(id));
                                    if current.as_ref() == Some(&id)
                                        && position >= 0
                                        && position as f64 / 1e6 <= app.audio.borrow().duration()
                                    {
                                        app.seek(position as f64 / 1e6);
                                    }
                                }
                            }
                            "OpenUri" => {
                                if let Some((uri,)) = params.get::<(String,)>() {
                                    let parsed = url::Url::parse(&uri).ok();
                                    if parsed.as_ref().is_some_and(|u| {
                                        u.scheme() == "file"
                                            && u.to_file_path().is_ok_and(|p| p.is_file())
                                    }) || services::http_url(&uri).is_ok()
                                    {
                                        let kind = if uri.starts_with("file:") {
                                            Kind::Music
                                        } else {
                                            Kind::Stream
                                        };
                                        let title = parsed
                                            .and_then(|u| {
                                                u.path_segments().and_then(|mut s| {
                                                    s.next_back().map(String::from)
                                                })
                                            })
                                            .unwrap_or_else(|| "Audio".into());
                                        let track = Track::new(&uri, &title, kind);
                                        let id = track.id.clone();
                                        app.state.borrow_mut().merge(vec![track]);
                                        app.play_track(&id, None);
                                    } else {
                                        invocation.return_dbus_error(
                                            "org.freedesktop.DBus.Error.InvalidArgs",
                                            "Unsupported or missing media URI",
                                        );
                                        return;
                                    }
                                }
                            }
                            _ => {
                                invocation.return_dbus_error(
                                    "org.freedesktop.DBus.Error.UnknownMethod",
                                    "Unknown method",
                                );
                                return;
                            }
                        }
                        invocation.return_value(Some(&().to_variant()));
                    })
                    .property(move |_, _, _, _, name| {
                        getter
                            .upgrade()
                            .map(|app| property(&app, name))
                            .unwrap_or_else(|| false.to_variant())
                    })
                    .set_property(move |_, _, _, _, name, value| {
                        let Some(app) = setter.upgrade() else {
                            return false;
                        };
                        match name {
                            "Volume" => {
                                if let Some(volume) = value.get::<f64>() {
                                    app.set_volume(volume);
                                } else {
                                    return false;
                                }
                            }
                            "Shuffle" => {
                                if let Some(shuffle) = value.get::<bool>() {
                                    app.set_shuffle(shuffle);
                                } else {
                                    return false;
                                }
                            }
                            "LoopStatus" => {
                                let repeat = match value.str() {
                                    Some("None") => Repeat::Off,
                                    Some("Track") => Repeat::One,
                                    Some("Playlist") => Repeat::All,
                                    _ => return false,
                                };
                                app.state.borrow_mut().repeat = repeat;
                                app.persist();
                                app.update_player();
                            }
                            "Rate" => return value.get::<f64>() == Some(1.0),
                            _ => return false,
                        }
                        true
                    })
                    .build();
                if let Err(e) = registration {
                    eprintln!("MPRIS registration: {e}");
                }
            }
            register_tray(&app, &connection);
        },
        |_, _| {},
        |_, _| {},
    );
}

pub fn changed(app: &App) {
    let Some(connection) = app.dbus.borrow().clone() else {
        return;
    };
    let mut properties: Properties = HashMap::new();
    for name in [
        "PlaybackStatus",
        "Metadata",
        "Volume",
        "Shuffle",
        "LoopStatus",
        "CanSeek",
        "CanGoNext",
        "CanGoPrevious",
        "CanPlay",
        "CanPause",
    ] {
        properties.insert(name.into(), property(app, name));
    }
    let _ = connection.emit_signal(
        None,
        ROOT,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        Some(&(PLAYER, properties, Vec::<String>::new()).to_variant()),
    );
}
pub fn seeked(app: &App, seconds: f64) {
    if let Some(connection) = app.dbus.borrow().as_ref() {
        let _ = connection.emit_signal(
            None,
            ROOT,
            PLAYER,
            "Seeked",
            Some(&((seconds * 1e6) as i64,).to_variant()),
        );
    }
}

fn menu_properties(app: &App, id: i32) -> Properties {
    let mut props = Properties::new();
    if id == 0 {
        props.insert("children-display".into(), "submenu".to_variant());
    } else {
        props.insert(
            "label".into(),
            app.tr(match id {
                1 => "Open Audio Player",
                2 => "Play / pause",
                3 => "Next",
                _ => "Quit",
            })
            .to_variant(),
        );
        props.insert("enabled".into(), true.to_variant());
        props.insert("visible".into(), true.to_variant());
    }
    props
}
fn menu_event(app: &Rc<App>, id: i32) {
    match id {
        1 => app.show(),
        2 => app.toggle_play(),
        3 => app.next(false),
        4 => app.quit(),
        _ => {}
    }
}

fn register_tray(app: &Rc<App>, connection: &gio::DBusConnection) {
    let info = gio::DBusNodeInfo::for_xml(include_str!("../resources/tray.xml"))
        .expect("Invalid tray definition");
    let weak = Rc::downgrade(app);
    let registration = connection
        .register_object(
            "/StatusNotifierItem",
            &info.lookup_interface("org.kde.StatusNotifierItem").unwrap(),
        )
        .method_call(move |_, _, _, _, method, params, invocation| {
            if let Some(app) = weak.upgrade() {
                match method {
                    "Activate" | "ContextMenu" => app.show(),
                    "SecondaryActivate" => app.toggle_play(),
                    "Scroll" => {
                        if let Some((delta, _)) = params.get::<(i32, String)>() {
                            let volume = app.state.borrow().volume;
                            app.set_volume(volume + f64::from(delta.signum()) * 0.05);
                        }
                    }
                    _ => {}
                }
            }
            invocation.return_value(Some(&().to_variant()));
        })
        .property(|_, _, _, _, name| match name {
            "Category" => "ApplicationStatus".to_variant(),
            "Id" => "open-audio-player".to_variant(),
            "Title" => "Open Audio Player".to_variant(),
            "Status" => "Active".to_variant(),
            "IconName" => "audio-x-generic".to_variant(),
            "WindowId" => 0_u32.to_variant(),
            "ItemIsMenu" => false.to_variant(),
            "Menu" => glib::variant::ObjectPath::try_from("/Menu")
                .unwrap()
                .to_variant(),
            "IconPixmap" | "OverlayIconPixmap" | "AttentionIconPixmap" => {
                Vec::<(i32, i32, Vec<u8>)>::new().to_variant()
            }
            "ToolTip" => (
                "audio-x-generic",
                Vec::<(i32, i32, Vec<u8>)>::new(),
                "Open Audio Player",
                "Music, podcasts, and radio",
            )
                .to_variant(),
            _ => "".to_variant(),
        })
        .build();
    if let Err(e) = registration {
        eprintln!("Tray registration: {e}");
        return;
    }
    let weak = Rc::downgrade(app);
    let registration = connection
        .register_object(
            "/Menu",
            &info.lookup_interface("com.canonical.dbusmenu").unwrap(),
        )
        .method_call(move |_, _, _, _, method, params, invocation| {
            let Some(app) = weak.upgrade() else {
                invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", "Player closed");
                return;
            };
            let result = match method {
                "GetLayout" => {
                    let parent = params.child_get::<i32>(0);
                    let children: Vec<glib::Variant> = if parent == 0 {
                        (1..=4)
                            .map(|id| {
                                (id, menu_properties(&app, id), Vec::<glib::Variant>::new())
                                    .to_variant()
                            })
                            .collect()
                    } else {
                        vec![]
                    };
                    (1_u32, (parent, menu_properties(&app, parent), children)).to_variant()
                }
                "GetGroupProperties" => {
                    let ids = params.child_get::<Vec<i32>>(0);
                    (ids.into_iter()
                        .map(|id| (id, menu_properties(&app, id)))
                        .collect::<Vec<_>>(),)
                        .to_variant()
                }
                "GetProperty" => {
                    let id = params.child_get::<i32>(0);
                    let name = params.child_get::<String>(1);
                    (menu_properties(&app, id)
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| "".to_variant()),)
                        .to_variant()
                }
                "Event" => {
                    if params.child_get::<String>(1) == "clicked" {
                        menu_event(&app, params.child_get::<i32>(0));
                    }
                    ().to_variant()
                }
                "EventGroup" => {
                    for (id, event, _, _) in
                        params.child_get::<Vec<(i32, String, glib::Variant, u32)>>(0)
                    {
                        if event == "clicked" {
                            menu_event(&app, id);
                        }
                    }
                    (Vec::<i32>::new(),).to_variant()
                }
                "AboutToShow" => (false,).to_variant(),
                "AboutToShowGroup" => (Vec::<i32>::new(), Vec::<i32>::new()).to_variant(),
                _ => ().to_variant(),
            };
            invocation.return_value(Some(&result));
        })
        .property(|_, _, _, _, name| match name {
            "Version" => 3_u32.to_variant(),
            "TextDirection" => "ltr".to_variant(),
            "IconThemePath" => Vec::<String>::new().to_variant(),
            _ => "normal".to_variant(),
        })
        .build();
    if let Err(e) = registration {
        eprintln!("Tray menu registration: {e}");
        return;
    }
    let weak = Rc::downgrade(app);
    let gone = Rc::downgrade(app);
    gio::bus_watch_name(
        gio::BusType::Session,
        "org.kde.StatusNotifierWatcher",
        gio::BusNameWatcherFlags::NONE,
        move |connection, _, _| {
            let weak = weak.clone();
            connection.call(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher",
                "RegisterStatusNotifierItem",
                Some(&("/StatusNotifierItem",).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                3000,
                None::<&gio::Cancellable>,
                move |result| {
                    if let Some(app) = weak.upgrade() {
                        app.tray_available.set(result.is_ok());
                    }
                },
            );
        },
        move |_, _| {
            if let Some(app) = gone.upgrade() {
                app.tray_available.set(false);
                if !app.window.is_visible() {
                    app.show();
                }
            }
        },
    );
}
