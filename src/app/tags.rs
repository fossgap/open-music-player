use super::*;
use crate::tags::{Destination, Field, Patch, Saved};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(super) struct Controls {
    pub busy: Cell<bool>,
    editor: RefCell<Option<Rc<Editor>>>,
    worker: RefCell<Option<std::thread::JoinHandle<Vec<Saved>>>>,
    progress: RefCell<Option<mpsc::Receiver<(usize, usize)>>>,
    cancel: Arc<AtomicBool>,
}

struct Editor {
    ui: gtk::Builder,
    tracks: RefCell<Vec<Track>>,
    fields: BTreeMap<Field, (gtk::CheckButton, gtk::Entry)>,
    cover: RefCell<String>,
    reviewing: Cell<bool>,
}

impl Editor {
    fn window(&self) -> gtk::Window {
        object(&self.ui, "tag_window")
    }
    fn patch(&self) -> Patch {
        let mut patch = Patch(
            self.fields
                .iter()
                .filter(|(_, (apply, _))| apply.is_active())
                .map(|(&field, (_, entry))| (field, entry.text().to_string()))
                .collect(),
        );
        if object::<gtk::CheckButton>(&self.ui, "apply_cover").is_active() {
            patch.0.insert(Field::Cover, self.cover.borrow().clone());
        }
        if object::<gtk::CheckButton>(&self.ui, "apply_lyrics").is_active() {
            let buffer = object::<gtk::TextView>(&self.ui, "tag_lyrics").buffer();
            patch.0.insert(
                Field::Lyrics,
                buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string(),
            );
        }
        patch
    }
    fn destination(&self) -> Destination {
        if object::<gtk::DropDown>(&self.ui, "tag_destination").selected() == 1 {
            Destination::Files
        } else {
            Destination::Library
        }
    }
    fn changed(&self) {
        let changed = self.fields.values().any(|(apply, _)| apply.is_active())
            || object::<gtk::CheckButton>(&self.ui, "apply_cover").is_active()
            || object::<gtk::CheckButton>(&self.ui, "apply_lyrics").is_active();
        object::<gtk::Button>(&self.ui, "tag_commit").set_sensitive(
            changed
                && (self.destination() == Destination::Library
                    || self.tracks.borrow().iter().all(crate::tags::writable)),
        );
    }
    fn status(&self, text: &str, error: bool) {
        let label: gtk::Label = object(&self.ui, "tag_status");
        label.set_label(text);
        label.set_tooltip_text(Some(text));
        label.set_visible(!text.is_empty());
        if error {
            label.add_css_class("error");
        } else {
            label.remove_css_class("error");
        }
    }
}

fn common(tracks: &[Track], field: Field) -> Option<String> {
    let first = field.value(tracks.first()?);
    tracks
        .iter()
        .all(|t| field.value(t) == first)
        .then_some(first)
}

impl App {
    pub(super) fn setup_tags(self: &Rc<Self>) {
        self.string_action("edit-tags", |app, id| app.edit_tags(vec![id.to_string()]));
        self.action("selection-tags", |app| app.edit_tags(app.selected_ids()));
    }

    pub(super) fn edit_tags(self: &Rc<Self>, ids: Vec<String>) {
        if let Some(editor) = self.tags.editor.borrow().as_ref() {
            editor.window().present();
            return;
        }
        let mut seen = HashSet::new();
        let tracks: Vec<_> = ids
            .iter()
            .filter(|id| seen.insert(*id))
            .filter_map(|id| self.state.borrow().track(id).cloned())
            .filter(|t| t.kind == Kind::Music && library::local_path(&t.uri).is_some())
            .collect();
        if tracks.is_empty() || self.tags.busy.get() {
            return;
        }
        let ui = gtk::Builder::from_string(include_str!("../../resources/tag-editor.ui"));
        self.i18n.builder(&ui);
        let window: gtk::Window = object(&ui, "tag_window");
        window.set_application(Some(&self.application));
        window.set_transient_for(Some(&self.window));
        let bulk = tracks.len() > 1;
        object::<gtk::Label>(&ui, "tag_summary").set_label(&if bulk {
            format!("{}: {}", self.tr("Selected tracks"), tracks.len())
        } else {
            format!("{} — {}", tracks[0].title, self.artist(&tracks[0]))
        });
        object::<gtk::Label>(&ui, "tag_hint").set_visible(bulk);
        let container: gtk::Box = object(&ui, "tag_fields");
        let numbers = gtk::Grid::builder()
            .column_spacing(16)
            .row_spacing(12)
            .column_homogeneous(true)
            .build();
        let mut fields = BTreeMap::new();
        for (index, field) in Field::TEXT.into_iter().enumerate() {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
            let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let apply = gtk::CheckButton::new();
            apply.set_visible(bulk);
            let label = gtk::Label::new(Some(&self.tr(field.label())));
            label.set_xalign(0.0);
            let entry = gtk::Entry::new();
            entry.set_hexpand(true);
            entry.set_width_chars(1);
            entry.set_widget_name(&format!("tag_{field:?}").to_lowercase());
            entry.set_max_length(if field.numeric() { 5 } else { 4096 });
            entry.update_property(&[gtk::accessible::Property::Label(&self.tr(field.label()))]);
            apply.set_tooltip_text(Some(&self.tr("Apply to selected tracks")));
            apply.update_property(&[gtk::accessible::Property::Label(&format!(
                "{}: {}",
                self.tr("Apply to selected tracks"),
                self.tr(field.label())
            ))]);
            if field.numeric() {
                entry.set_input_purpose(gtk::InputPurpose::Digits);
            }
            if let Some(value) = common(&tracks, field) {
                entry.set_text(&value);
            } else {
                entry.set_placeholder_text(Some(&self.tr("Multiple values")));
            }
            heading.append(&apply);
            heading.append(&label);
            row.append(&heading);
            row.append(&entry);
            if field.numeric() {
                let position = index as i32 - 4;
                numbers.attach(&row, position % 2, position / 2, 1, 1);
                if field == Field::Number {
                    container.append(&numbers);
                }
            } else {
                container.append(&row);
            }
            if field == Field::Artists {
                let hint = gtk::Label::new(Some(
                    &self.tr("Separate multiple artists, genres or composers with ;"),
                ));
                hint.set_wrap(true);
                hint.set_xalign(0.0);
                hint.add_css_class("muted");
                hint.add_css_class("small");
                row.append(&hint);
            }
            if field == Field::Date && common(&tracks, field).is_some() {
                entry.set_placeholder_text(Some("YYYY-MM-DD"));
            }
            fields.insert(field, (apply, entry));
        }
        let destination: gtk::DropDown = object(&ui, "tag_destination");
        destination.set_model(Some(&gtk::StringList::new(&[
            &self.tr("Library only"),
            &self.tr("Audio files"),
        ])));
        destination.update_property(&[gtk::accessible::Property::Label(&self.tr("Save to"))]);
        let lyrics: gtk::TextView = object(&ui, "tag_lyrics");
        lyrics.update_property(&[gtk::accessible::Property::Label(&self.tr("Lyrics"))]);
        lyrics
            .buffer()
            .set_text(&common(&tracks, Field::Lyrics).unwrap_or_default());
        object::<gtk::Button>(&ui, "tag_reset")
            .set_visible(tracks.iter().any(|t| !t.tag_overrides.is_empty()));
        object::<gtk::Button>(&ui, "tag_reset").update_property(&[
            gtk::accessible::Property::Label(&self.tr("Reload file tags")),
        ]);
        let editor = Rc::new(Editor {
            cover: RefCell::new(common(&tracks, Field::Cover).unwrap_or_default()),
            tracks: RefCell::new(tracks),
            ui,
            fields,
            reviewing: Cell::new(false),
        });
        *self.tags.editor.borrow_mut() = Some(editor.clone());
        for (apply, entry) in editor.fields.values() {
            let weak = Rc::downgrade(&editor);
            let apply = apply.clone();
            entry.connect_changed(move |_| {
                apply.set_active(true);
                if let Some(e) = weak.upgrade() {
                    e.changed();
                }
            });
        }
        for apply in editor
            .fields
            .values()
            .map(|(apply, _)| apply.clone())
            .chain([
                object(&editor.ui, "apply_cover"),
                object(&editor.ui, "apply_lyrics"),
            ])
        {
            let weak = Rc::downgrade(&editor);
            apply.connect_toggled(move |_| {
                if let Some(e) = weak.upgrade() {
                    e.changed();
                }
            });
        }
        let weak = Rc::downgrade(&editor);
        lyrics.buffer().connect_changed(move |_| {
            if let Some(e) = weak.upgrade() {
                object::<gtk::CheckButton>(&e.ui, "apply_lyrics").set_active(true);
                e.changed();
            }
        });
        let weak = Rc::downgrade(self);
        destination.connect_selected_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                app.tag_destination_hint();
            }
        });
        for (id, action) in [
            ("tag_commit", 0),
            ("tag_cancel", 1),
            ("tag_back", 2),
            ("choose_tag_cover", 3),
            ("remove_tag_cover", 4),
            ("tag_reset", 5),
        ] {
            let weak = Rc::downgrade(self);
            object::<gtk::Button>(&editor.ui, id).connect_clicked(move |_| {
                let Some(app) = weak.upgrade() else { return };
                let Some(e) = app.tags.editor.borrow().clone() else {
                    return;
                };
                match action {
                    0 if e.reviewing.get() => app.save_tags(e.destination()),
                    0 => app.review_tags(),
                    1 if app.tags.busy.get() => {
                        app.tags.cancel.store(true, Ordering::Relaxed);
                        e.status(&app.tr("Cancelling…"), false);
                    }
                    1 => e.window().close(),
                    2 => {
                        e.reviewing.set(false);
                        object::<gtk::Stack>(&e.ui, "tag_flow").set_visible_child_name("edit");
                        object::<gtk::Button>(&e.ui, "tag_back").set_visible(false);
                        object::<gtk::Button>(&e.ui, "tag_commit")
                            .set_label(&app.tr("Review changes"));
                        object::<gtk::Box>(&e.ui, "tag_destination_row").set_sensitive(true);
                    }
                    3 => app.choose_tag_cover(),
                    4 => {
                        e.cover.borrow_mut().clear();
                        object::<gtk::CheckButton>(&e.ui, "apply_cover").set_active(true);
                        app.tag_cover_preview();
                    }
                    _ => app.confirm_reload_tags(),
                }
            });
        }
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| {
            if let Some(app) = weak.upgrade() {
                if app.tags.busy.get() {
                    app.tags.cancel.store(true, Ordering::Relaxed);
                    return glib::Propagation::Stop;
                }
                app.tags.editor.borrow_mut().take();
            }
            glib::Propagation::Proceed
        });
        let keys = gtk::EventControllerKey::new();
        let weak_window = window.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(w) = weak_window.upgrade() {
                    w.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        self.tag_destination_hint();
        self.tag_cover_preview();
        window.present();
    }

    fn tag_destination_hint(&self) {
        let Some(e) = self.tags.editor.borrow().clone() else {
            return;
        };
        object::<gtk::Label>(&e.ui, "tag_destination_hint").set_label(&self.tr(if e.destination() == Destination::Library {
            "Changes stay in this library and survive rescans. Audio files are unchanged."
        } else if e.tracks.borrow().iter().all(crate::tags::writable) {
            "Writes checked tags into your files without re-encoding audio. Each file is verified before replacement."
        } else { "Some selected formats support library-only edits." }));
        e.changed();
    }

    fn tag_cover_preview(&self) {
        let Some(e) = self.tags.editor.borrow().clone() else {
            return;
        };
        let path = e.cover.borrow();
        self.set_art(
            &object::<gtk::Picture>(&e.ui, "tag_cover"),
            (!path.is_empty()).then_some(path.as_str()),
        );
        object::<gtk::Label>(&e.ui, "tag_cover_name").set_label(&if path.is_empty() {
            self.tr("No cover selected")
        } else {
            PathBuf::from(path.as_str())
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    }

    fn choose_tag_cover(self: &Rc<Self>) {
        let Some(e) = self.tags.editor.borrow().clone() else {
            return;
        };
        let chooser = gtk::FileChooserNative::new(
            Some(&self.tr("Choose cover")),
            Some(&e.window()),
            gtk::FileChooserAction::Open,
            Some(&self.tr("Open")),
            Some(&self.tr("Cancel")),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&self.tr("PNG and JPEG images")));
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        chooser.add_filter(&filter);
        let weak = Rc::downgrade(self);
        chooser.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept
                && let (Some(app), Some(path)) =
                    (weak.upgrade(), dialog.file().and_then(|f| f.path()))
                && let Some(e) = app.tags.editor.borrow().clone()
            {
                *e.cover.borrow_mut() = path.to_string_lossy().into_owned();
                object::<gtk::CheckButton>(&e.ui, "apply_cover").set_active(true);
                app.tag_cover_preview();
            }
            dialog.destroy();
        });
        chooser.show();
    }

    fn review_tags(&self) {
        let Some(e) = self.tags.editor.borrow().clone() else {
            return;
        };
        let patch = e.patch();
        let tracks = e.tracks.borrow();
        if let Err(error) = patch.validate(&tracks) {
            e.status(&self.tr(error), true);
            return;
        }
        if patch.is_empty() {
            return;
        }
        e.status("", false);
        let review: gtk::Box = object(&e.ui, "tag_review");
        while let Some(child) = review.first_child() {
            review.remove(&child);
        }
        for (&field, value) in &patch.0 {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
            let title = gtk::Label::new(Some(&self.tr(field.label())));
            title.set_xalign(0.0);
            title.add_css_class("section-title");
            row.append(&title);
            let shorten = |s: &str| -> String {
                if s.is_empty() {
                    return self.tr("Empty");
                }
                if field == Field::Cover {
                    return PathBuf::from(s)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                }
                let value: String = s.chars().take(180).collect();
                if s.chars().count() > 180 {
                    format!("{value}…")
                } else {
                    value
                }
            };
            let old = common(&tracks, field)
                .map(|s| shorten(&s))
                .unwrap_or_else(|| self.tr("Multiple values"));
            for (prefix, value) in [("Before", old), ("After", shorten(value))] {
                let label = gtk::Label::new(Some(&format!("{}: {value}", self.tr(prefix))));
                label.set_xalign(0.0);
                label.set_wrap(true);
                label.set_selectable(true);
                if prefix == "Before" {
                    label.add_css_class("muted");
                }
                row.append(&label);
            }
            review.append(&row);
            review.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        let heading = gtk::Label::new(Some(&format!(
            "{}: {}",
            self.tr("Selected tracks"),
            tracks.len()
        )));
        heading.set_xalign(0.0);
        review.append(&heading);
        let paths: Vec<_> = tracks
            .iter()
            .map(|t| {
                library::local_path(&t.uri)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        let strings = gtk::StringList::new(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            label.add_css_class("small");
            item.set_child(Some(&label));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let label = item.child().unwrap().downcast::<gtk::Label>().unwrap();
            let value = item
                .item()
                .unwrap()
                .downcast::<gtk::StringObject>()
                .unwrap()
                .string();
            label.set_label(&value);
            label.set_tooltip_text(Some(&value));
        });
        let list = gtk::ListView::new(Some(gtk::NoSelection::new(Some(strings))), Some(factory));
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .height_request(100)
            .child(&list)
            .build();
        review.append(&scroll);
        e.reviewing.set(true);
        object::<gtk::Stack>(&e.ui, "tag_flow").set_visible_child_name("review");
        object::<gtk::Button>(&e.ui, "tag_back").set_visible(true);
        object::<gtk::Button>(&e.ui, "tag_commit").set_label(&self.tr("Save changes"));
        object::<gtk::Box>(&e.ui, "tag_destination_row").set_sensitive(false);
    }

    fn confirm_reload_tags(self: &Rc<Self>) {
        let Some(e) = self.tags.editor.borrow().clone() else {
            return;
        };
        let dialog = gtk::MessageDialog::builder().transient_for(&e.window()).modal(true)
            .text(self.tr("Reload file tags?"))
            .secondary_text(self.tr("Remove library-only edits and read tags from the files. Audio files are unchanged."))
            .buttons(gtk::ButtonsType::None).build();
        dialog.add_button(&self.tr("Cancel"), gtk::ResponseType::Cancel);
        dialog.add_button(&self.tr("Reload"), gtk::ResponseType::Accept);
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |d, response| {
            d.close();
            if response == gtk::ResponseType::Accept
                && let Some(app) = weak.upgrade()
            {
                app.save_tags(Destination::Reload);
            }
        });
        dialog.present();
    }

    fn save_tags(&self, destination: Destination) {
        let Some(e) = self.tags.editor.borrow().clone() else {
            return;
        };
        if self.tags.busy.get() {
            return;
        }
        if self.scanning.get() || self.files.importing.get() || self.files.deleting.get() {
            e.status(
                &self.tr("The library is updating. Try saving again shortly."),
                true,
            );
            return;
        }
        let patch = if destination == Destination::Reload {
            Patch::default()
        } else {
            e.patch()
        };
        let snapshots = e.tracks.borrow();
        let state = self.state.borrow();
        let tracks: Vec<_> = snapshots
            .iter()
            .filter_map(|old| {
                state
                    .track(&old.id)
                    .filter(|t| {
                        t.uri == old.uri
                            && t.tag_revision == old.tag_revision
                            && t.modified == old.modified
                            && t.size == old.size
                    })
                    .cloned()
            })
            .collect();
        if tracks.len() != snapshots.len() {
            e.status(
                &self.tr("These tracks changed. Close the editor and open it again."),
                true,
            );
            return;
        }
        if let Err(error) = patch.validate(&tracks) {
            e.status(&self.tr(error), true);
            return;
        }
        drop(state);
        drop(snapshots);
        self.tags.busy.set(true);
        self.tags.cancel.store(false, Ordering::Relaxed);
        object::<gtk::Stack>(&e.ui, "tag_flow").set_sensitive(false);
        for id in ["tag_commit", "tag_back", "tag_reset"] {
            object::<gtk::Button>(&e.ui, id).set_sensitive(false);
        }
        for id in ["tag_back", "tag_reset"] {
            object::<gtk::Button>(&e.ui, id).set_visible(false);
        }
        object::<gtk::Box>(&e.ui, "tag_destination_row").set_sensitive(false);
        object::<gtk::Button>(&e.ui, "tag_cancel").set_label(&self.tr("Cancel remaining"));
        object::<gtk::ProgressBar>(&e.ui, "tag_progress").set_visible(true);
        e.status(&self.tr("Saving tags…"), false);
        let (tx, rx) = mpsc::channel();
        *self.tags.progress.borrow_mut() = Some(rx);
        let cache = self.cache.clone();
        let cancel = self.tags.cancel.clone();
        *self.tags.worker.borrow_mut() = Some(std::thread::spawn(move || {
            crate::tags::save(
                tracks,
                patch,
                destination,
                &cache,
                &cancel,
                |done, total| {
                    let _ = tx.send((done, total));
                },
            )
        }));
        self.update_selection();
    }

    pub(super) fn poll_tags(&self) {
        if let Some(rx) = self.tags.progress.borrow().as_ref()
            && let Some((done, total)) = rx.try_iter().last()
            && let Some(e) = self.tags.editor.borrow().as_ref()
        {
            object::<gtk::ProgressBar>(&e.ui, "tag_progress")
                .set_fraction(done as f64 / total.max(1) as f64);
            e.status(
                &format!("{} {done}/{total}", self.tr("Saving tags…")),
                false,
            );
        }
        if self
            .tags
            .worker
            .borrow()
            .as_ref()
            .is_some_and(|w| w.is_finished())
        {
            self.finish_tags(true);
        }
    }

    pub(super) fn finish_tags(&self, notify: bool) {
        let Some(worker) = self.tags.worker.borrow_mut().take() else {
            return;
        };
        let results = worker.join().unwrap_or_else(|_| {
            self.tags.editor.borrow().as_ref().map(|e| e.tracks.borrow().iter().cloned().map(|original| Saved {
                original, updated: Err("Tag editing stopped unexpectedly. Refresh the library before retrying.".into()),
            }).collect()).unwrap_or_default()
        });
        self.tags.busy.set(false);
        self.tags.progress.borrow_mut().take();
        let mut count = 0;
        let mut errors = Vec::new();
        let mut failed = Vec::new();
        let mut state = self.state.borrow_mut();
        for result in results {
            let Some(current) = state
                .tracks
                .iter_mut()
                .find(|t| t.id == result.original.id && t.uri == result.original.uri)
            else {
                continue;
            };
            match result.updated {
                Ok(mut updated) if current.tag_revision == result.original.tag_revision => {
                    updated.favorite = current.favorite;
                    updated.download_path = current.download_path.clone();
                    *current = updated;
                    count += 1;
                }
                outcome => {
                    let error = outcome.err().unwrap_or_else(|| {
                        "These tracks changed. Close the editor and open it again.".into()
                    });
                    let translated = error.split_once(": ").map_or_else(
                        || self.tr(&error),
                        |(field, message)| format!("{}: {}", self.tr(field), self.tr(message)),
                    );
                    errors.push(format!("{}: {translated}", current.title));
                    failed.push(current.clone());
                }
            }
        }
        let album = self.album.borrow().clone();
        if let Some(key) = album
            && !state.tracks.iter().any(|t| t.album_key() == key)
        {
            // The album was renamed; return to the grid instead of leaving a misleading empty album.
            drop(state);
            self.album.borrow_mut().take();
            state = self.state.borrow_mut();
        }
        drop(state);
        self.persist();
        if !notify {
            return;
        }
        self.refresh();
        self.update_player();
        let summary = format!("{}: {count}", self.tr("Tracks updated"));
        let editor = self.tags.editor.borrow().clone();
        if let Some(e) = editor {
            if errors.is_empty() {
                e.window().close();
                self.notice(&summary, false, false);
            } else {
                *e.tracks.borrow_mut() = failed;
                object::<gtk::Label>(&e.ui, "tag_summary").set_label(&format!(
                    "{}: {}",
                    self.tr("Selected tracks"),
                    e.tracks.borrow().len()
                ));
                if e.reviewing.get() {
                    self.review_tags();
                }
                object::<gtk::Stack>(&e.ui, "tag_flow").set_sensitive(true);
                for id in ["tag_commit", "tag_back", "tag_reset"] {
                    object::<gtk::Button>(&e.ui, id).set_sensitive(true);
                }
                object::<gtk::Button>(&e.ui, "tag_back").set_visible(e.reviewing.get());
                object::<gtk::Button>(&e.ui, "tag_reset").set_visible(
                    e.tracks
                        .borrow()
                        .iter()
                        .any(|t| !t.tag_overrides.is_empty()),
                );
                object::<gtk::Box>(&e.ui, "tag_destination_row").set_sensitive(!e.reviewing.get());
                object::<gtk::Button>(&e.ui, "tag_cancel").set_label(&self.tr("Close"));
                object::<gtk::ProgressBar>(&e.ui, "tag_progress").set_visible(false);
                e.status(
                    &format!(
                        "{summary} · {}: {}\n{}",
                        self.tr("Tracks not updated"),
                        errors.len(),
                        errors
                            .iter()
                            .take(5)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    true,
                );
                e.changed();
            }
        }
    }

    pub(super) fn shutdown_tags(&self) {
        self.tags.cancel.store(true, Ordering::Relaxed);
        self.finish_tags(false);
    }
}

#[cfg(test)]
pub(super) fn exercise(app: &Rc<App>, tone: &std::path::Path) {
    use super::tests::{screenshot, settle};
    use std::os::unix::fs::PermissionsExt;
    let until = |condition: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !condition() && Instant::now() < deadline {
            settle();
        }
        assert!(condition(), "Tag editor operation timed out");
    };
    let before = app.state.borrow().clone();
    let page = app.page.borrow().clone();
    let albums_view = object::<gtk::ToggleButton>(&app.ui, "albums_view").is_active();
    let directory = tempfile::tempdir().unwrap();
    let mut tracks = Vec::new();
    for (index, (disc, number)) in [(2, 1), (1, 2), (1, 1)].into_iter().enumerate() {
        let path = directory.path().join(format!("track-{index}.flac"));
        let output = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(tone)
            .args([
                "-metadata",
                &format!("title=Tag fixture {index}"),
                "-metadata",
                &format!("artist=Guest {index}"),
                "-metadata",
                "album=Tag compilation",
                "-metadata",
                "album_artist=Various Artists",
                "-metadata",
                &format!("disc={disc}/2"),
                "-metadata",
                &format!("track={number}/8"),
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        tracks.push(library::probe(&path, &app.cache).unwrap());
    }
    let ids: Vec<_> = tracks.iter().map(|t| t.id.clone()).collect();
    let path = library::local_path(&tracks[0].uri).unwrap();
    let original_bytes = std::fs::read(&path).unwrap();
    app.state.borrow_mut().merge(tracks.clone());
    app.navigate("library");
    object::<gtk::ToggleButton>(&app.ui, "albums_view").set_active(true);
    object::<gtk::SearchEntry>(&app.ui, "search").set_text("Tag compilation");
    app.refresh();
    settle();
    assert_eq!(
        app.albums_model.n_items(),
        1,
        "Guest artists must share one compilation card"
    );
    app.open_album(&ids[0]);
    settle();
    assert_eq!(
        *app.visible.borrow(),
        vec![ids[2].clone(), ids[1].clone(), ids[0].clone()]
    );
    app.play_track(&ids[0], None);
    app.pause_playback();
    let queue = app.state.borrow().queue.clone();
    app.edit_tags(ids.clone());
    settle();
    let e = app.tags.editor.borrow().clone().unwrap();
    assert!(
        e.patch().is_empty(),
        "Opening the editor must not prepare changes"
    );
    assert_eq!(
        e.fields[&Field::Title].1.placeholder_text().as_deref(),
        Some("Multiple values")
    );
    e.fields[&Field::Album].1.set_text("Corrected compilation");
    e.fields[&Field::Genres].1.set_text("Ambient; Jazz");
    e.fields[&Field::Date].1.set_text("2024-06-15");
    e.fields[&Field::Number].1.set_text("-1");
    app.review_tags();
    assert!(!e.reviewing.get());
    assert!(object::<gtk::Label>(&e.ui, "tag_status").is_visible());
    e.fields[&Field::Number].1.set_text("");
    e.fields[&Field::Number].0.set_active(false);
    e.status("", false);
    screenshot(&e.window(), "tags-bulk-sage.png");
    e.window().set_default_size(430, 620);
    settle();
    screenshot(&e.window(), "tags-bulk-compact.png");
    app.state.borrow_mut().theme = model::Theme::System;
    app.apply_theme();
    app.theme.borrow_mut().system_scheme(2);
    settle();
    screenshot(&e.window(), "tags-system-light.png");
    app.state.borrow_mut().theme = model::Theme::Sage;
    app.apply_theme();
    settle();
    app.review_tags();
    settle();
    screenshot(&e.window(), "tags-review-compact.png");
    assert_eq!(
        app.state.borrow().track(&ids[0]).unwrap().album,
        "Tag compilation"
    );
    app.save_tags(Destination::Library);
    until(&|| !app.tags.busy.get());
    assert!(app.tags.editor.borrow().is_none());
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
    for id in &ids {
        let state = app.state.borrow();
        let t = state.track(id).unwrap();
        assert_eq!(t.album, "Corrected compilation");
        assert_eq!(t.genres, ["Ambient", "Jazz"]);
        assert_eq!(t.date, "2024-06-15");
        assert!(t.title.starts_with("Tag fixture"));
    }
    assert_eq!(app.state.borrow().queue, queue);
    assert_eq!(app.state.borrow().current.as_ref(), Some(&ids[0]));
    assert!(
        !app.audio.borrow().playing,
        "Tag changes must not restart paused playback"
    );
    let rescanned = library::scan(
        &[directory.path().into()],
        &app.state.borrow().tracks,
        &app.cache,
    )
    .unwrap();
    app.state.borrow_mut().merge(rescanned.tracks);
    assert_eq!(
        app.state.borrow().track(&ids[0]).unwrap().album,
        "Corrected compilation"
    );
    app.edit_tags(vec![ids[0].clone()]);
    settle();
    let e = app.tags.editor.borrow().clone().unwrap();
    e.fields[&Field::Title].1.set_text("Saved to the file");
    let cover =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 64, 64).unwrap();
    cover.fill(0x446688ff);
    let cover_path = directory.path().join("chosen.png");
    std::fs::write(&cover_path, cover.save_to_bufferv("png", &[]).unwrap()).unwrap();
    *e.cover.borrow_mut() = cover_path.to_string_lossy().into_owned();
    object::<gtk::CheckButton>(&e.ui, "apply_cover").set_active(true);
    app.tag_cover_preview();
    object::<gtk::Stack>(&e.ui, "tag_pages").set_visible_child_name("artwork");
    settle();
    screenshot(&e.window(), "tags-artwork.png");
    object::<gtk::TextView>(&e.ui, "tag_lyrics")
        .buffer()
        .set_text("[00:01.00]First line\n[00:03.00]Second line");
    object::<gtk::Stack>(&e.ui, "tag_pages").set_visible_child_name("lyrics");
    settle();
    screenshot(&e.window(), "tags-lyrics.png");
    object::<gtk::DropDown>(&e.ui, "tag_destination").set_selected(1);
    app.review_tags();
    app.save_tags(Destination::Files);
    until(&|| !app.tags.busy.get());
    assert!(
        app.tags.editor.borrow().is_none(),
        "{}",
        object::<gtk::Label>(&e.ui, "tag_status").text()
    );
    let on_disk = library::probe(&path, &app.cache).unwrap();
    assert_eq!(on_disk.title, "Saved to the file");
    assert!(on_disk.artwork.is_some());
    assert!(on_disk.lyrics.unwrap().contains("[00:03.00]Second line"));
    assert_eq!(
        app.state.borrow().track(&ids[0]).unwrap().album,
        "Corrected compilation"
    );
    app.edit_tags(vec![ids[0].clone()]);
    settle();
    let e = app.tags.editor.borrow().clone().unwrap();
    e.fields[&Field::Title].1.set_text("Must not be saved");
    object::<gtk::DropDown>(&e.ui, "tag_destination").set_selected(1);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    app.review_tags();
    app.save_tags(Destination::Files);
    until(&|| !app.tags.busy.get());
    settle();
    assert!(
        object::<gtk::Label>(&e.ui, "tag_status")
            .text()
            .contains("read-only")
    );
    screenshot(&e.window(), "tags-read-only.png");
    assert_eq!(
        app.state.borrow().track(&ids[0]).unwrap().title,
        "Saved to the file"
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    e.window().close();
    app.edit_tags(vec![ids[0].clone()]);
    settle();
    app.save_tags(Destination::Reload);
    until(&|| !app.tags.busy.get());
    assert!(
        app.state
            .borrow()
            .track(&ids[0])
            .unwrap()
            .tag_overrides
            .is_empty()
    );
    assert_eq!(
        app.state.borrow().track(&ids[0]).unwrap().album,
        "Tag compilation"
    );
    app.stop_playback();
    *app.state.borrow_mut() = before;
    app.theme.borrow_mut().system_scheme(0);
    app.apply_theme();
    app.navigate(&page);
    object::<gtk::ToggleButton>(
        &app.ui,
        if albums_view {
            "albums_view"
        } else {
            "tracks_view"
        },
    )
    .set_active(true);
    app.refresh();
    app.update_player();
}
