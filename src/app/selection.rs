use super::*;
use crate::collections;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Collection {
    Queue,
    Playlist(String),
}

#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "OpenAudioTrackDrag")]
pub(super) struct TrackDrag {
    collection: Collection,
    order: Vec<String>,
    positions: Vec<usize>,
}

impl App {
    fn track_selection(&self) -> gtk::SelectionModel {
        object::<gtk::ListView>(&self.ui, "track_list")
            .model()
            .unwrap()
    }

    pub(super) fn selected_rows(&self) -> Vec<usize> {
        let selection = self.track_selection();
        (0..self.tracks_model.n_items())
            .filter(|&i| selection.is_selected(i))
            .map(|i| i as usize)
            .collect()
    }

    pub(super) fn selected_ids(&self) -> Vec<String> {
        self.selected_rows()
            .into_iter()
            .filter_map(|i| self.tracks_model.string(i as u32).map(|s| s.to_string()))
            .collect()
    }

    pub(super) fn collection(&self) -> Option<Collection> {
        if self.page.borrow().as_str() == "queue" {
            Some(Collection::Queue)
        } else if ["playlists", "youtube", "spotify"].contains(&self.page.borrow().as_str()) {
            self.playlist.borrow().clone().map(Collection::Playlist)
        } else {
            None
        }
    }

    fn collection_order(&self, collection: &Collection) -> Option<Vec<String>> {
        let state = self.state.borrow();
        match collection {
            Collection::Queue => Some(state.queue.clone()),
            Collection::Playlist(id) => state.playlist(id).map(|p| p.tracks.clone()),
        }
    }

    pub(super) fn can_reorder(&self) -> bool {
        self.collection().is_some()
            && object::<gtk::SearchEntry>(&self.ui, "search")
                .text()
                .trim()
                .is_empty()
    }

    pub(super) fn sync_track_selection(&self, ids: &[String]) {
        let context = format!(
            "{}:{:?}:{:?}",
            self.page.borrow(),
            self.playlist.borrow(),
            self.album.borrow()
        );
        let same_context = *self.selection_context.borrow() == context;
        let old: Vec<String> = (0..self.tracks_model.n_items())
            .filter_map(|i| self.tracks_model.string(i).map(|s| s.to_string()))
            .collect();
        if same_context && old == ids {
            return;
        }
        let old_keys = occurrence_keys(&old);
        let keep: HashSet<_> = if same_context {
            self.selected_rows()
                .into_iter()
                .filter_map(|i| old_keys.get(i).cloned())
                .collect()
        } else {
            HashSet::new()
        };
        self.selection_syncing.set(true);
        browse::sync_model(&self.tracks_model, ids);
        let selection = self.track_selection();
        selection.unselect_all();
        for (i, key) in occurrence_keys(ids).iter().enumerate() {
            if keep.contains(key) {
                selection.select_item(i as u32, false);
            }
        }
        *self.selection_context.borrow_mut() = context;
        self.selection_syncing.set(false);
    }

    fn select_rows(&self, rows: &[usize]) {
        self.selection_syncing.set(true);
        let selection = self.track_selection();
        selection.unselect_all();
        for &row in rows {
            selection.select_item(row as u32, false);
        }
        self.selection_syncing.set(false);
        self.update_selection();
    }

    pub(super) fn setup_selection(self: &Rc<Self>) {
        self.action("selection-play", |app| {
            let mut ids = app.selected_ids();
            if app.state.borrow().shuffle {
                model::shuffle(&mut ids);
            }
            if let Some(first) = ids.first().cloned() {
                app.state.borrow_mut().queue = ids;
                app.play_track(&first, None);
            }
        });
        self.action("selection-queue", |app| {
            let ids = app.selected_ids();
            let mut state = app.state.borrow_mut();
            for id in ids {
                if !state.queue.contains(&id) {
                    state.queue.push(id);
                }
            }
            drop(state);
            app.persist();
            app.refresh();
            app.notice(&app.tr("Added to queue"), false, false);
        });
        self.action("selection-playlist", |app| {
            app.playlist_tracks_dialog(app.selected_ids(), None)
        });
        self.action("selection-favorite", |app| app.favorite_selection(true));
        self.action("selection-unfavorite", |app| app.favorite_selection(false));
        self.action("selection-remove", |app| {
            app.remove_rows(&app.selected_rows())
        });
        self.action("selection-trash", |app| {
            app.trash_local_tracks(app.selected_ids())
        });
        self.action("selection-clear", |app| {
            app.track_selection().unselect_all();
        });
        self.action("selection-all", |app| {
            app.track_selection().select_all();
        });
        self.action("selection-up", |app| app.move_selection(false));
        self.action("selection-down", |app| app.move_selection(true));
        self.action("selection-first", |app| app.move_selection_to(0));
        self.action("selection-last", |app| {
            app.move_selection_to(app.tracks_model.n_items() as usize)
        });
        self.string_action("row-remove", |app, position| {
            if let Ok((page, playlist, id, position)) =
                serde_json::from_str::<(String, Option<String>, String, usize)>(position)
            {
                if page == *app.page.borrow()
                    && playlist == *app.playlist.borrow()
                    && app.tracks_model.string(position as u32).as_deref() == Some(&id)
                {
                    app.remove_rows(&[position]);
                }
            }
        });
        let menu = gio::Menu::new();
        for (label, action) in [
            ("Edit tags", "selection-tags"),
            ("Add to queue", "selection-queue"),
            ("Add to playlist", "selection-playlist"),
            ("Add to favorites", "selection-favorite"),
            ("Remove from favorites", "selection-unfavorite"),
            ("Move to top", "selection-first"),
            ("Move to bottom", "selection-last"),
            ("Remove from this list", "selection-remove"),
            ("Move files to Trash", "selection-trash"),
            ("Select all", "selection-all"),
        ] {
            menu.append(Some(&self.tr(label)), Some(&format!("app.{action}")));
        }
        object::<gtk::MenuButton>(&self.ui, "selection_actions").set_menu_model(Some(&menu));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(app) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK) {
                match key {
                    gdk::Key::Up => app.move_selection(false),
                    gdk::Key::Down => app.move_selection(true),
                    gdk::Key::Home => app.move_selection_to(0),
                    gdk::Key::End => app.move_selection_to(app.tracks_model.n_items() as usize),
                    _ => return glib::Propagation::Proceed,
                }
            } else if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::a {
                app.track_selection().select_all();
            } else if key == gdk::Key::Escape {
                app.track_selection().unselect_all();
            } else if key == gdk::Key::Delete {
                app.remove_rows(&app.selected_rows());
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        object::<gtk::ListView>(&self.ui, "track_list").add_controller(keys);
        let end = gtk::DropTarget::new(TrackDrag::static_type(), gdk::DragAction::MOVE);
        let weak = Rc::downgrade(self);
        end.connect_drop(move |_, value, _, _| {
            let (Some(app), Ok(drag)) = (weak.upgrade(), value.get::<TrackDrag>()) else {
                return false;
            };
            app.drop_tracks(&drag, app.tracks_model.n_items() as usize)
        });
        object::<gtk::ListView>(&self.ui, "track_list").add_controller(end);
        self.update_selection();
    }

    pub(super) fn update_selection(&self) {
        let selected = self.selected_rows();
        let tracks = object::<gtk::Stack>(&self.ui, "content_stack")
            .visible_child_name()
            .as_deref()
            == Some("tracks");
        object::<gtk::Box>(&self.ui, "selection_bar").set_visible(tracks && !selected.is_empty());
        let page = self.page.borrow();
        let compact = self.compact_browse.get()
            && (["youtube", "spotify", "streams"].contains(&page.as_str())
                || (*page == "podcasts" && self.podcast_discovery.get())
                || (tracks && !selected.is_empty()));
        let content: gtk::Box = object(&self.ui, "content");
        content.set_spacing(if compact { 10 } else { 20 });
        content.set_margin_top(if compact { 16 } else { 26 });
        content.set_margin_bottom(if compact { 12 } else { 20 });
        object::<gtk::Label>(&self.ui, "footer_status").set_visible(!compact);
        self.label(
            "selection_count",
            &format!("{} {}", selected.len(), self.tr("selected")),
        );
        let reorder = tracks && self.can_reorder() && !selected.is_empty();
        let can_trash = tracks
            && !selected.is_empty()
            && !self.files.deleting.get()
            && !self.tags.busy.get()
            && self.selected_ids().iter().all(|id| {
                self.state
                    .borrow()
                    .track(id)
                    .is_some_and(|t| t.kind == Kind::Music && library::local_path(&t.uri).is_some())
            });
        if let Some(action) = self
            .application
            .lookup_action("selection-trash")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(can_trash);
        }
        if let Some(action) = self
            .application
            .lookup_action("selection-tags")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(can_trash);
        }
        for name in [
            "selection-play",
            "selection-queue",
            "selection-playlist",
            "selection-favorite",
            "selection-unfavorite",
            "selection-remove",
            "selection-clear",
        ] {
            if let Some(action) = self
                .application
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(tracks && !selected.is_empty());
            }
        }
        object::<gtk::Box>(&self.ui, "selection_order").set_visible(self.collection().is_some());
        for (name, enabled) in [
            (
                "selection-up",
                reorder
                    && selected
                        .iter()
                        .any(|&i| i > 0 && selected.binary_search(&(i - 1)).is_err()),
            ),
            (
                "selection-down",
                reorder
                    && selected.iter().any(|&i| {
                        i + 1 < self.tracks_model.n_items() as usize
                            && selected.binary_search(&(i + 1)).is_err()
                    }),
            ),
            ("selection-first", reorder),
            ("selection-last", reorder),
        ] {
            if let Some(action) = self
                .application
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(enabled);
            }
        }
        object::<gtk::Box>(&self.ui, "selection_order").set_tooltip_text(Some(&self.tr(
            if self.can_reorder() {
                "Reorder selected tracks"
            } else {
                "Clear search to reorder tracks."
            },
        )));
    }

    fn favorite_selection(&self, favorite: bool) {
        let ids: HashSet<_> = self.selected_ids().into_iter().collect();
        for track in &mut self.state.borrow_mut().tracks {
            if ids.contains(&track.id) {
                track.favorite = favorite;
            }
        }
        self.refresh();
        self.update_player();
        self.persist();
    }

    pub(super) fn remove_rows(&self, rows: &[usize]) {
        let ids: Vec<String> = rows
            .iter()
            .filter_map(|&i| self.tracks_model.string(i as u32).map(|s| s.to_string()))
            .collect();
        if ids.is_empty() {
            return;
        }
        if let Some(collection) = self.collection() {
            let Some(mut order) = self.collection_order(&collection) else {
                return;
            };
            let map = collections::source_positions(&order, &self.visible.borrow());
            let positions: Vec<_> = rows.iter().filter_map(|&i| map.get(i).copied()).collect();
            collections::remove(&mut order, &positions);
            self.set_collection_order(&collection, order);
        } else if self.page.borrow().as_str() == "favorites" {
            for track in &mut self.state.borrow_mut().tracks {
                if ids.contains(&track.id) {
                    track.favorite = false;
                }
            }
        } else if self.page.borrow().as_str() == "library" {
            self.remove_local_tracks(&ids);
            return;
        } else {
            if self
                .state
                .borrow()
                .current
                .as_ref()
                .is_some_and(|id| ids.contains(id))
            {
                self.stop_playback();
            }
            let mut state = self.state.borrow_mut();
            state.remove_from_library(&ids);
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

    fn set_collection_order(&self, collection: &Collection, order: Vec<String>) {
        let mut state = self.state.borrow_mut();
        match collection {
            Collection::Queue => state.queue = order,
            Collection::Playlist(id) => {
                if let Some(list) = state.playlists.iter_mut().find(|p| &p.id == id) {
                    list.tracks = order;
                }
            }
        }
    }

    pub(super) fn move_selection(&self, down: bool) {
        if !self.can_reorder() {
            return;
        }
        let Some(collection) = self.collection() else {
            return;
        };
        let Some(mut order) = self.collection_order(&collection) else {
            return;
        };
        if let Some(selected) = collections::step(&mut order, &self.selected_rows(), down) {
            self.set_collection_order(&collection, order);
            self.refresh();
            self.select_rows(&selected);
            self.persist();
        }
    }

    fn move_selection_to(&self, at: usize) {
        if let Some(drag) = self.track_drag() {
            self.drop_tracks(&drag, at);
        }
    }

    fn track_drag(&self) -> Option<TrackDrag> {
        if !self.can_reorder() {
            return None;
        }
        let collection = self.collection()?;
        let order = self.collection_order(&collection)?;
        let positions = self.selected_rows();
        if positions.is_empty() {
            return None;
        }
        Some(TrackDrag {
            collection,
            order,
            positions,
        })
    }

    fn drop_tracks(&self, drag: &TrackDrag, at: usize) -> bool {
        if !self.can_reorder()
            || self.collection().as_ref() != Some(&drag.collection)
            || self.collection_order(&drag.collection).as_ref() != Some(&drag.order)
        {
            return false;
        }
        let mut order = drag.order.clone();
        let Some(selected) = collections::reorder(&mut order, &drag.positions, at) else {
            return false;
        };
        self.set_collection_order(&drag.collection, order);
        self.refresh();
        self.select_rows(&selected);
        self.persist();
        true
    }

    pub(super) fn setup_track_drag(self: &Rc<Self>, item: &gtk::ListItem, ui: &gtk::Builder) {
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        source.set_button(1);
        source.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        let weak_item = item.downgrade();
        source.connect_begin(move |source, _| {
            let (Some(app), Some(item)) = (weak.upgrade(), weak_item.upgrade()) else {
                source.set_state(gtk::EventSequenceState::Denied);
                return;
            };
            if !app.can_reorder() || item.position() >= app.tracks_model.n_items() {
                source.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            // Own the press before the row click or list rubberband can claim
            // its motion. Claiming in prepare is too late: it runs at threshold.
            source.set_state(gtk::EventSequenceState::Claimed);
            if !app.track_selection().is_selected(item.position()) {
                app.track_selection().select_item(item.position(), true);
            }
        });
        let weak = Rc::downgrade(self);
        let weak_item = item.downgrade();
        source.connect_prepare(move |_, _, _| {
            let app = weak.upgrade()?;
            let item = weak_item.upgrade()?;
            if !app.can_reorder() {
                return None;
            }
            if !app.track_selection().is_selected(item.position()) {
                app.track_selection().select_item(item.position(), true);
            }
            app.track_drag()
                .map(|drag| gdk::ContentProvider::for_value(&drag.to_value()))
        });
        let handle: gtk::Image = object(ui, "drag_handle");
        handle.set_cursor_from_name(Some("grab"));
        handle.add_controller(source);
        let target = gtk::DropTarget::new(TrackDrag::static_type(), gdk::DragAction::MOVE);
        let row: gtk::Box = object(ui, "row");
        let weak_row = row.downgrade();
        target.connect_motion(move |_, _, y| {
            if let Some(row) = weak_row.upgrade() {
                row.remove_css_class("drop-before");
                row.remove_css_class("drop-after");
                row.add_css_class(if y < f64::from(row.height()) / 2.0 {
                    "drop-before"
                } else {
                    "drop-after"
                });
            }
            gdk::DragAction::MOVE
        });
        let weak_row = row.downgrade();
        target.connect_leave(move |_| {
            if let Some(row) = weak_row.upgrade() {
                row.remove_css_class("drop-before");
                row.remove_css_class("drop-after");
            }
        });
        let weak = Rc::downgrade(self);
        let weak_item = item.downgrade();
        let weak_row = row.downgrade();
        target.connect_drop(move |_, value, _, y| {
            let (Some(app), Some(item), Some(row)) =
                (weak.upgrade(), weak_item.upgrade(), weak_row.upgrade())
            else {
                return false;
            };
            row.remove_css_class("drop-before");
            row.remove_css_class("drop-after");
            let Ok(drag) = value.get::<TrackDrag>() else {
                return false;
            };
            app.drop_tracks(
                &drag,
                item.position() as usize + usize::from(y >= f64::from(row.height()) / 2.0),
            )
        });
        row.add_controller(target);
    }
}

fn occurrence_keys(ids: &[String]) -> Vec<(String, usize)> {
    let mut counts = HashMap::new();
    ids.iter()
        .map(|id| {
            let count = counts.entry(id).or_insert(0);
            let key = (id.clone(), *count);
            *count += 1;
            key
        })
        .collect()
}
