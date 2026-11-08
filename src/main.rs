mod albums;
mod app;
mod audio;
mod audio_info;
mod collections;
mod desktop;
mod downloads;
mod enrichment;
mod i18n;
mod library;
mod lyrics;
mod model;
mod playlists;
mod process;
mod services;
mod spotify;
mod storage;
mod tags;
mod watch;
mod youtube_music;

use gtk::{gio, glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("Open Audio Player {}", env!("CARGO_PKG_VERSION"));
        return glib::ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Open Audio Player\n\nUsage: open-audio-player [--import FOLDER] [--smoke-test]\n\n  --import FOLDER  Scan a music folder on startup\n  --smoke-test     Open the UI, then exit after 3 seconds\n  --version       Print version\n\nEnvironment:\n  OPEN_AUDIO_DATA_DIR  Override the library directory\n  OPEN_AUDIO_FAKE_SINK Use a silent sink for automated tests"
        );
        return glib::ExitCode::SUCCESS;
    }
    let import = args
        .iter()
        .position(|a| a == "--import")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let smoke = args.iter().any(|a| a == "--smoke-test");
    let application = gtk::Application::builder()
        .application_id("io.github.openaudio.Player")
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();
    let controller: Rc<RefCell<Option<Rc<app::App>>>> = Rc::new(RefCell::new(None));
    let active = controller.clone();
    application.connect_startup(|_| {
        let bytes =
            glib::Bytes::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/icons.gresource")));
        let resource = gio::Resource::from_data(&bytes).expect("Invalid embedded icons");
        gio::resources_register(&resource);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_resource_path("/io/github/openaudio/Player/icons");
        }
    });
    application.connect_activate(move |application| {
        if let Some(app) = active.borrow().as_ref() {
            app.show();
            return;
        }
        let result = storage::load(&storage::data_dir().join("library.json"))
            .and_then(|state| app::App::new(application, state));
        match result {
            Ok(app) => {
                if let Some(folder) = &import {
                    let mut folders = app.state.borrow().folders.clone();
                    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.clone());
                    if !folders.contains(&folder) {
                        folders.push(folder);
                    }
                    app.scan(folders);
                }
                *active.borrow_mut() = Some(app);
                if smoke {
                    let application = application.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                        application.quit()
                    });
                }
            }
            Err(error) => {
                eprintln!("{error}");
                let dialog = gtk::MessageDialog::builder()
                    .application(application)
                    .text("Open Audio Player could not start")
                    .secondary_text(error)
                    .buttons(gtk::ButtonsType::Close)
                    .build();
                let application = application.clone();
                dialog.connect_response(move |d, _| {
                    d.close();
                    application.quit();
                });
                dialog.present();
            }
        }
    });
    // Custom options are parsed above, not forwarded to GApplication.
    let result = application.run_with_args(&["open-audio-player"]);
    if let Some(app) = controller.borrow_mut().take() {
        app.shutdown();
    }
    result
}
