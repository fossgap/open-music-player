use super::*;
use crate::search::{
    self as index, Document, Group, Target,
    providers::{Payload, Reply},
};
use std::sync::Arc;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use tests::exercise;

#[derive(Clone)]
enum Status {
    Loading,
    Ready,
    Failed(String),
    Disconnected,
    Cancelled,
}
struct Section {
    status: Status,
    items: Vec<Document>,
}
#[derive(Clone)]
enum Row {
    Heading(Group, String, bool),
    Item(Box<Document>),
    More,
}

pub(super) struct Controls {
    ui: gtk::Builder,
    model: gtk::StringList,
    rows: RefCell<HashMap<String, Row>>,
    collapsed: RefCell<HashSet<Group>>,
    bound: BoundItems,
    hidden: RefCell<Vec<(gtk::Widget, bool)>>,
    documents: RefCell<Arc<Vec<Document>>>,
    local: index::LocalSearch,
    matches: RefCell<index::Results>,
    local_busy: Cell<bool>,
    limit: Cell<usize>,
    service: index::providers::Service,
    sections: RefCell<BTreeMap<Group, Section>>,
    tracks: RefCell<HashMap<String, Track>>,
    preview: RefCell<Option<services::PodcastShow>>,
    query: RefCell<String>,
    spotify_account: RefCell<Option<String>>,
    artwork: RefCell<HashMap<String, String>>,
    artwork_failed: RefCell<HashSet<String>>,
    artwork_check: Cell<Instant>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            ui: gtk::Builder::from_string(include_str!("../../resources/search.ui")),
            model: gtk::StringList::new(&[]),
            rows: Default::default(),
            collapsed: Default::default(),
            bound: Default::default(),
            hidden: Default::default(),
            documents: Default::default(),
            local: Default::default(),
            matches: Default::default(),
            local_busy: Cell::new(false),
            limit: Cell::new(50),
            service: Default::default(),
            sections: Default::default(),
            tracks: Default::default(),
            preview: Default::default(),
            query: Default::default(),
            spotify_account: Default::default(),
            artwork: Default::default(),
            artwork_failed: Default::default(),
            artwork_check: Cell::new(Instant::now()),
        }
    }
}

impl App {
    pub(super) fn setup_global_search(self: &Rc<Self>) {
        self.i18n.builder(&self.search.ui);
        let root: gtk::Box = object(&self.search.ui, "global_search_root");
        object::<gtk::Box>(&self.ui, "content").append(&root);
        self.action("global-search", |app| app.navigate("search"));
        self.application
            .set_accels_for_action("app.global-search", &["<Control>k"]);
        let query: gtk::SearchEntry = object(&self.search.ui, "global_query");
        query.update_property(&[gtk::accessible::Property::Label(
            &self.tr("Search all audio"),
        )]);
        if let Some(text) = query.delegate().and_downcast::<gtk::Text>() {
            text.set_max_length(256);
        }
        let weak = Rc::downgrade(self);
        query.connect_changed(move |_| {
            if let Some(app) = weak.upgrade() {
                app.invalidate_global_query();
            }
        });
        let weak = Rc::downgrade(self);
        query.connect_search_changed(move |_| {
            if let Some(app) = weak.upgrade() {
                app.search_collection();
            }
        });
        let weak = Rc::downgrade(self);
        query.connect_activate(move |_| {
            if let Some(app) = weak.upgrade() {
                app.search_online();
            }
        });
        let filter: gtk::DropDown = object(&self.search.ui, "global_filter");
        let labels: Vec<_> = [
            "All audio",
            "Music",
            "Albums",
            "Playlists",
            "Podcasts",
            "Stations",
            "Streams",
        ]
        .map(|s| self.tr(s))
        .into();
        filter.set_model(Some(&gtk::StringList::new(
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        filter.update_property(&[gtk::accessible::Property::Label(&self.tr("Audio type"))]);
        let weak = Rc::downgrade(self);
        filter.connect_selected_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                app.render_global_search();
            }
        });
        for (id, action) in [
            ("global_submit", 0),
            ("global_cancel", 1),
            ("global_back", 2),
            ("global_dismiss", 3),
        ] {
            let weak = Rc::downgrade(self);
            object::<gtk::Button>(&self.search.ui, id).connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    match action {
                        0 => app.search_online(),
                        1 => {
                            app.cancel_global_search();
                            app.render_global_search();
                        }
                        2 => {
                            app.search.preview.borrow_mut().take();
                            app.search.sections.borrow_mut().remove(&Group::Feed);
                            app.render_global_search();
                        }
                        _ => object::<gtk::Box>(&app.search.ui, "global_notice").set_visible(false),
                    }
                }
            });
        }
        let list: gtk::ListView = object(&self.search.ui, "global_results");
        let weak = Rc::downgrade(self);
        object::<gtk::Button>(&self.search.ui, "global_subscribe").connect_clicked(move |_| {
            let Some(app) = weak.upgrade() else { return };
            let show = app.search.preview.borrow().clone();
            if let Some(show) = show {
                app.toggle_search_subscription(&show.feed_url);
            }
        });
        let selection = gtk::SingleSelection::new(Some(self.search.model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        list.set_model(Some(&selection));
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(self);
        factory.connect_setup(move |_, item| {
            let Some(app) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let ui = gtk::Builder::from_string(include_str!("../../resources/search-row.ui"));
            for (id, action) in [("primary", 0), ("secondary", 1), ("heading_toggle", 3)] {
                let weak = Rc::downgrade(&app);
                let row = item.downgrade();
                object::<gtk::Button>(&ui, id).connect_clicked(move |_| {
                    if let (Some(app), Some(item)) = (weak.upgrade(), row.upgrade()) {
                        if let Some(key) = item.item().and_downcast::<gtk::StringObject>() {
                            app.act_global(&key.string(), action);
                        }
                    }
                });
            }
            item.set_child(Some(&object::<gtk::Box>(&ui, "row")));
            app.search
                .bound
                .borrow_mut()
                .insert(item.as_ptr() as usize, (item.downgrade(), ui));
        });
        let weak = Rc::downgrade(self);
        factory.connect_bind(move |_, item| {
            if let Some(app) = weak.upgrade() {
                app.bind_global_row(item.downcast_ref::<gtk::ListItem>().unwrap());
            }
        });
        let weak = Rc::downgrade(self);
        factory.connect_teardown(move |_, item| {
            if let Some(app) = weak.upgrade() {
                app.search
                    .bound
                    .borrow_mut()
                    .remove(&(item.as_ptr() as usize));
            }
        });
        list.set_factory(Some(&factory));
        let weak = Rc::downgrade(self);
        list.connect_activate(move |_, position| {
            if let Some(app) = weak.upgrade() {
                if let Some(key) = app.search.model.string(position) {
                    app.act_global(&key, 2);
                }
            }
        });
    }

    pub(super) fn focus_global_search(&self) {
        object::<gtk::SearchEntry>(&self.search.ui, "global_query").grab_focus();
    }

    pub(super) fn show_global_search(&self) {
        let root: gtk::Box = object(&self.search.ui, "global_search_root");
        if self.search.hidden.borrow().is_empty() {
            let content: gtk::Box = object(&self.ui, "content");
            let mut child = content.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if widget != root {
                    self.search
                        .hidden
                        .borrow_mut()
                        .push((widget.clone(), widget.is_visible()));
                }
            }
        }
        for (widget, _) in self.search.hidden.borrow().iter() {
            widget.set_visible(false);
        }
        root.set_visible(true);
        *self.search.documents.borrow_mut() = index::snapshot(&self.state.borrow());
        self.search_collection();
        self.render_global_search();
    }

    pub(super) fn hide_global_search(&self) {
        for (widget, visible) in self.search.hidden.borrow_mut().drain(..) {
            widget.set_visible(visible);
        }
        object::<gtk::Box>(&self.search.ui, "global_search_root").set_visible(false);
    }

    fn invalidate_global_query(&self) {
        let query = object::<gtk::SearchEntry>(&self.search.ui, "global_query")
            .text()
            .trim()
            .to_string();
        if *self.search.query.borrow() == query {
            return;
        }
        self.cancel_global_search();
        *self.search.query.borrow_mut() = query;
        self.search.matches.borrow_mut().clear();
        self.search.sections.borrow_mut().clear();
        self.search.tracks.borrow_mut().clear();
        self.search.preview.borrow_mut().take();
        self.search.artwork_failed.borrow_mut().clear();
        self.search.limit.set(50);
        object::<gtk::Box>(&self.search.ui, "global_notice").set_visible(false);
        object::<gtk::ScrolledWindow>(&self.search.ui, "global_scroller")
            .vadjustment()
            .set_value(0.0);
        self.render_global_search();
    }

    fn search_collection(&self) {
        let query = self.search.query.borrow().clone();
        if query.is_empty() {
            self.search.local_busy.set(false);
            return;
        }
        self.search.local_busy.set(true);
        self.search.local.search(
            self.search.documents.borrow().clone(),
            query,
            self.search.limit.get(),
        );
    }

    fn search_online(&self) {
        self.invalidate_global_query();
        if self.search.query.borrow().is_empty() {
            self.focus_global_search();
            return;
        }
        let category = object::<gtk::DropDown>(&self.search.ui, "global_filter").selected();
        if !matches!(category, 0 | 1 | 4 | 5) {
            self.search_collection();
            return;
        }
        self.search.preview.borrow_mut().take();
        self.search.service.cancel();
        self.search.sections.borrow_mut().clear();
        self.search.tracks.borrow_mut().clear();
        for group in Group::ONLINE {
            if category == 0 || category == group.category() {
                self.search_provider(group);
            }
        }
        self.search_collection();
        self.render_global_search();
    }

    fn search_provider(&self, group: Group) {
        let query = if group == Group::Feed {
            let Some(show) = self.search.preview.borrow().clone() else {
                return;
            };
            show.feed_url
        } else {
            self.search.query.borrow().clone()
        };
        if query.is_empty() {
            return;
        }
        self.search.sections.borrow_mut().insert(
            group,
            Section {
                status: Status::Loading,
                items: Vec::new(),
            },
        );
        if group == Group::Spotify {
            let account = self
                .state
                .borrow()
                .spotify_account
                .as_ref()
                .map(|a| a.id.clone());
            *self.search.spotify_account.borrow_mut() = account;
            if !self.audio.borrow().spotify_ready() {
                self.search
                    .sections
                    .borrow_mut()
                    .get_mut(&group)
                    .unwrap()
                    .status = Status::Disconnected;
            } else {
                self.spotify
                    .service
                    .send(crate::spotify::Command::GlobalSearch(
                        self.search.service.generation(),
                        query,
                        self.search.service.epoch.clone(),
                    ));
            }
        } else {
            self.search.service.search(
                group,
                query,
                self.state.borrow().youtube_account.clone(),
                self.cache.clone(),
            );
        }
    }

    pub(super) fn cancel_global_search(&self) {
        self.search.service.cancel();
        self.search.local.cancel();
        self.search.local_busy.set(false);
        for section in self.search.sections.borrow_mut().values_mut() {
            if matches!(section.status, Status::Loading) {
                section.status = Status::Cancelled;
            }
        }
    }

    pub(super) fn poll_global_search(&self) {
        // Drain artwork in bounded batches; failures stay placeholders until the next query.
        if *self.page.borrow() == "search"
            && self.search.artwork_check.get().elapsed() >= Duration::from_millis(500)
        {
            self.search.artwork_check.set(Instant::now());
            if self.artwork_pending.borrow().is_empty() && !self.global_artwork_urls().is_empty() {
                self.ensure_artwork();
            }
        }
        let mut changed = false;
        if let Some(matches) = self.search.local.poll() {
            *self.search.matches.borrow_mut() = matches;
            self.search.local_busy.set(false);
            changed = true;
        }
        for reply in self.search.service.poll() {
            self.global_reply(reply);
        }
        let account = self
            .state
            .borrow()
            .spotify_account
            .as_ref()
            .map(|a| a.id.clone());
        if let Some(section) = self.search.sections.borrow_mut().get_mut(&Group::Spotify) {
            if matches!(section.status, Status::Loading)
                && (account != *self.search.spotify_account.borrow()
                    || !self.audio.borrow().spotify_ready() && !self.spotify.busy.get())
            {
                section.status = Status::Disconnected;
                changed = true;
            }
        }
        if changed {
            self.render_global_search();
        }
    }

    pub(super) fn global_spotify_reply(&self, generation: u64, result: Result<Vec<Track>, String>) {
        self.global_reply(Reply {
            generation,
            group: Group::Spotify,
            query: self.search.query.borrow().clone(),
            result: result.map(Payload::Tracks),
        });
    }

    fn global_reply(&self, reply: Reply) {
        if reply.generation != self.search.service.generation() {
            return;
        }
        if reply.group == Group::Spotify
            && (self
                .state
                .borrow()
                .spotify_account
                .as_ref()
                .map(|a| a.id.clone())
                != *self.search.spotify_account.borrow()
                || !self.audio.borrow().spotify_ready())
        {
            return;
        }
        if reply.group == Group::Feed
            && self
                .search
                .preview
                .borrow()
                .as_ref()
                .is_none_or(|s| s.feed_url != reply.query)
        {
            return;
        }
        if !self.search.sections.borrow().contains_key(&reply.group) {
            return;
        }
        let section = match reply.result {
            Ok(payload) => {
                let items = match payload {
                    Payload::Tracks(tracks) => {
                        let items = tracks
                            .iter()
                            .map(|t| index::remote_track(t, reply.group))
                            .collect();
                        self.search
                            .tracks
                            .borrow_mut()
                            .extend(tracks.into_iter().map(|t| (t.id.clone(), t)));
                        items
                    }
                    Payload::Shows(shows) => shows
                        .into_iter()
                        .map(|s| index::show_document(s, reply.group))
                        .collect(),
                };
                Section {
                    status: Status::Ready,
                    items,
                }
            }
            Err(error) => Section {
                status: Status::Failed(error),
                items: Vec::new(),
            },
        };
        self.search
            .sections
            .borrow_mut()
            .insert(reply.group, section);
        self.render_global_search();
        self.ensure_artwork();
    }

    fn render_global_search(&self) {
        let category = object::<gtk::DropDown>(&self.search.ui, "global_filter").selected();
        let preview = self.search.preview.borrow().clone();
        let subscribe: gtk::Button = object(&self.search.ui, "global_subscribe");
        subscribe.set_visible(preview.is_some());
        object::<gtk::DropDown>(&self.search.ui, "global_filter").set_visible(preview.is_none());
        if let Some(show) = &preview {
            let subscribed = self
                .state
                .borrow()
                .subscriptions
                .iter()
                .any(|s| s.url == show.feed_url);
            subscribe.set_label(&self.tr(if subscribed {
                "Unsubscribe"
            } else {
                "Subscribe"
            }));
            subscribe.set_sensitive(!self.podcast_pending(&show.feed_url));
        }
        object::<gtk::Button>(&self.search.ui, "global_back").set_visible(preview.is_some());
        object::<gtk::Label>(&self.search.ui, "global_title").set_label(
            &preview
                .as_ref()
                .map(|s| s.title.clone())
                .unwrap_or_else(|| self.tr("Search")),
        );
        object::<gtk::Label>(&self.search.ui, "global_subtitle").set_label(&self.tr(
            if preview.is_some() {
                "Podcast episodes"
            } else {
                "Music, podcasts, radio and streams"
            },
        ));
        let mut rows = HashMap::new();
        let mut keys = Vec::new();
        let mut seen = HashSet::new();
        let mut count = 0;
        let mut more = false;
        let collapsed = self.search.collapsed.borrow().clone();
        let mut append = |group: Group, items: &[Document], status: String, retry: bool| {
            let items: Vec<_> = items
                .iter()
                .filter(|item| seen.insert(item.key.clone()))
                .collect();
            let key = format!("heading:{group:?}");
            let detail = if status.is_empty() {
                items.len().to_string()
            } else {
                status
            };
            rows.insert(key.clone(), Row::Heading(group, detail, retry));
            keys.push(key);
            count += items.len();
            if collapsed.contains(&group) {
                return;
            }
            for item in items {
                rows.insert(item.key.clone(), Row::Item(Box::new(item.clone())));
                keys.push(item.key.clone());
            }
        };
        if preview.is_none() {
            for (&group, results) in self.search.matches.borrow().iter() {
                if category != 0 && category != group.category() {
                    continue;
                }
                more |= !collapsed.contains(&group) && results.total > results.items.len();
                append(
                    group,
                    &results.items,
                    if results.items.len() < results.total {
                        format!("{} / {}", results.items.len(), results.total)
                    } else {
                        results.total.to_string()
                    },
                    false,
                );
            }
        }
        let mut pending = self.search.local_busy.get()
            || self
                .search
                .sections
                .borrow()
                .values()
                .any(|s| matches!(s.status, Status::Loading));
        for (&group, section) in self.search.sections.borrow().iter() {
            if preview.is_some() && group != Group::Feed
                || preview.is_none()
                    && (group == Group::Feed || category != 0 && category != group.category())
            {
                continue;
            }
            let (status, retry) = match &section.status {
                Status::Loading => {
                    pending = true;
                    (self.tr("Searching…"), false)
                }
                Status::Ready => (String::new(), false),
                Status::Failed(_) => (self.tr("Search failed — try again"), true),
                Status::Disconnected => (self.tr("Not connected"), true),
                Status::Cancelled => (self.tr("Cancelled"), true),
            };
            append(group, &section.items, status, retry);
        }
        if more && preview.is_none() {
            keys.push("more".into());
            rows.insert("more".into(), Row::More);
        }
        *self.search.rows.borrow_mut() = rows;
        let previous: Vec<_> = (0..self.search.model.n_items())
            .filter_map(|p| self.search.model.string(p).map(|s| s.to_string()))
            .collect();
        if previous != keys {
            let selection = object::<gtk::ListView>(&self.search.ui, "global_results")
                .model()
                .unwrap()
                .downcast::<gtk::SingleSelection>()
                .unwrap();
            let selected = selection
                .selected_item()
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().to_string());
            super::browse::sync_model(&self.search.model, &keys);
            if let Some(position) = selected.and_then(|key| keys.iter().position(|k| *k == key)) {
                selection.set_selected(position as u32);
            }
        }
        let bound: Vec<_> = self
            .search
            .bound
            .borrow()
            .values()
            .filter_map(|(item, _)| item.upgrade())
            .collect();
        for item in bound {
            self.bind_global_row(&item);
        }
        object::<gtk::Stack>(&self.search.ui, "global_body")
            .set_visible_child_name(if keys.is_empty() { "empty" } else { "results" });
        let query_empty = self.search.query.borrow().is_empty();
        object::<gtk::Label>(&self.search.ui, "global_empty_title").set_label(&self.tr(
            if query_empty {
                "Find something to listen to"
            } else if pending {
                "Searching…"
            } else {
                "No matching audio"
            },
        ));
        object::<gtk::Label>(&self.search.ui, "global_empty_detail").set_label(&self.tr(
            if query_empty {
                "Search by title, artist, album, playlist or topic."
            } else if !matches!(category, 0 | 1 | 4 | 5) {
                "Try a different search."
            } else {
                "Try another search or audio type. Press Enter to search online."
            },
        ));
        object::<gtk::Label>(&self.search.ui, "global_count").set_label(&if query_empty {
            String::new()
        } else {
            format!(
                "{count} {}",
                self.tr(if count == 1 { "result" } else { "results" })
            )
        });
        object::<gtk::Spinner>(&self.search.ui, "global_busy").set_spinning(pending);
        object::<gtk::Spinner>(&self.search.ui, "global_busy").set_visible(pending);
        object::<gtk::Button>(&self.search.ui, "global_cancel").set_visible(pending);
        object::<gtk::Button>(&self.search.ui, "global_submit")
            .set_sensitive(!query_empty && matches!(category, 0 | 1 | 4 | 5));
        object::<gtk::Label>(&self.search.ui, "global_hint").set_label(&self.tr(
            if preview.is_some() {
                "Play or queue an episode, or go back to search results."
            } else if matches!(category, 0 | 1 | 4 | 5) {
                "Type to search your collection. Press Enter to search online."
            } else {
                "Albums, playlists and stream URLs are searched in your collection."
            },
        ));
    }

    fn bind_global_row(&self, item: &gtk::ListItem) {
        let Some(key) = item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let row = self
            .search
            .rows
            .borrow()
            .get(key.string().as_str())
            .cloned();
        let Some((_, ui)) = self
            .search
            .bound
            .borrow()
            .get(&(item.as_ptr() as usize))
            .cloned()
        else {
            return;
        };
        let heading = matches!(row, Some(Row::Heading(..)));
        let title: gtk::Label = object(&ui, if heading { "heading_title" } else { "title" });
        let detail: gtk::Label = object(&ui, if heading { "heading_detail" } else { "detail" });
        let toggle: gtk::Button = object(&ui, "heading_toggle");
        toggle.set_visible(heading);
        object::<gtk::Box>(&ui, "labels").set_visible(!heading);
        object::<gtk::Box>(&ui, "row").set_tooltip_text(None);
        let primary: gtk::Button = object(&ui, "primary");
        let secondary: gtk::Button = object(&ui, "secondary");
        let art: gtk::Picture = object(&ui, "art");
        title.remove_css_class("section-title");
        title.add_css_class("track-title");
        primary.set_visible(true);
        primary.set_sensitive(true);
        secondary.set_visible(false);
        item.set_selectable(matches!(row, Some(Row::Item(_))));
        item.set_activatable(matches!(row, Some(Row::Item(_))));
        object::<gtk::Overlay>(&ui, "artwork").set_visible(matches!(row, Some(Row::Item(_))));
        match row {
            Some(Row::Heading(group, text, retry)) => {
                let expanded = !self.search.collapsed.borrow().contains(&group);
                object::<gtk::Image>(&ui, "disclosure").set_icon_name(Some(if expanded {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                }));
                toggle.set_tooltip_text(Some(&self.tr(if expanded {
                    "Collapse group"
                } else {
                    "Expand group"
                })));
                toggle.update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
                toggle.update_property(&[gtk::accessible::Property::Label(&format!(
                    "{} — {text}",
                    self.tr(group.label())
                ))]);
                title.remove_css_class("track-title");
                title.add_css_class("section-title");
                title.set_label(&self.tr(group.label()));
                detail.set_label(&text);
                primary.set_visible(retry);
                primary.set_icon_name("view-refresh-symbolic");
                primary.set_tooltip_text(Some(&self.tr(
                    if group == Group::Spotify && !self.audio.borrow().spotify_ready() {
                        "Connect account"
                    } else {
                        "Try again"
                    },
                )));
                let error =
                    self.search
                        .sections
                        .borrow()
                        .get(&group)
                        .and_then(|s| match &s.status {
                            Status::Failed(e) => Some(e.clone()),
                            Status::Disconnected => {
                                Some(self.tr("Connect Spotify in Streaming to search."))
                            }
                            _ => None,
                        });
                detail.set_tooltip_text(error.as_deref());
                self.set_art(&art, None);
            }
            Some(Row::More) => {
                title.set_label(&self.tr("Show more results"));
                detail.set_label("");
                primary.set_icon_name("list-add-symbolic");
                primary.set_tooltip_text(Some(&self.tr("Show more results")));
                self.set_art(&art, None);
            }
            Some(Row::Item(document)) => {
                title.set_label(&document.title);
                detail.set_label(&document.detail);
                detail.set_tooltip_text(Some(&document.detail));
                let art_path = document.artwork.as_ref().map(|p| {
                    self.search
                        .artwork
                        .borrow()
                        .get(p)
                        .cloned()
                        .unwrap_or_else(|| p.clone())
                });
                self.set_art(&art, art_path.as_deref());
                let (icon, play, add) = match &document.target {
                    Target::Show(show) => {
                        primary.set_icon_name("go-next-symbolic");
                        secondary.set_sensitive(!self.podcast_pending(&show.feed_url));
                        let subscribed = self
                            .state
                            .borrow()
                            .subscriptions
                            .iter()
                            .any(|s| s.url == show.feed_url);
                        secondary.set_icon_name(if subscribed {
                            "emblem-ok-symbolic"
                        } else {
                            "list-add-symbolic"
                        });
                        (
                            "audio-input-microphone-symbolic",
                            "View episodes",
                            if subscribed {
                                "Unsubscribe"
                            } else {
                                "Subscribe"
                            },
                        )
                    }
                    target => {
                        primary.set_icon_name("media-playback-start-symbolic");
                        secondary.set_icon_name("list-add-symbolic");
                        secondary.set_sensitive(true);
                        if let Target::Playlist(id) = target {
                            let playable = self
                                .state
                                .borrow()
                                .playlist(id)
                                .is_some_and(|p| !p.tracks.is_empty());
                            primary.set_sensitive(playable);
                            secondary.set_sensitive(playable);
                        }
                        (
                            match target {
                                Target::Album(_) => "media-optical-symbolic",
                                Target::Playlist(_) => "view-list-symbolic",
                                _ => "audio-x-generic-symbolic",
                            },
                            "Play",
                            "Add to queue",
                        )
                    }
                };
                object::<gtk::Image>(&ui, "icon").set_icon_name(Some(icon));
                primary.set_tooltip_text(Some(&self.tr(play)));
                secondary.set_tooltip_text(Some(&self.tr(add)));
                secondary.set_visible(true);
                object::<gtk::Box>(&ui, "row")
                    .set_tooltip_text(Some(&format!("{} — {}", document.title, document.detail)));
            }
            None => return,
        }
        detail.set_visible(!detail.label().is_empty());
        for button in [primary, secondary] {
            if let Some(label) = button.tooltip_text() {
                button.update_property(&[gtk::accessible::Property::Label(&label)]);
            }
        }
    }

    fn act_global(self: &Rc<Self>, key: &str, action: u8) {
        let row = self.search.rows.borrow().get(key).cloned();
        let Some(row) = row else { return };
        let document = match row {
            Row::Heading(group, _, _) if action == 3 => {
                let mut collapsed = self.search.collapsed.borrow_mut();
                if !collapsed.remove(&group) {
                    collapsed.insert(group);
                }
                drop(collapsed);
                self.render_global_search();
                return;
            }
            Row::Heading(group, _, true) => {
                if group == Group::Spotify && !self.audio.borrow().spotify_ready() {
                    self.navigate("spotify");
                    self.spotify_primary();
                } else {
                    self.search_provider(group);
                    self.render_global_search();
                }
                return;
            }
            Row::Heading(..) => return,
            Row::More => {
                self.search.limit.set(self.search.limit.get() + 50);
                self.search_collection();
                return;
            }
            Row::Item(document) => *document,
        };
        if let Target::Show(show) = document.target {
            if action == 1 {
                self.toggle_search_subscription(&show.feed_url);
            } else {
                *self.search.preview.borrow_mut() = Some(show);
                self.search_provider(Group::Feed);
                object::<gtk::ScrolledWindow>(&self.search.ui, "global_scroller")
                    .vadjustment()
                    .set_value(0.0);
                self.render_global_search();
            }
            return;
        }
        if action == 2 {
            match &document.target {
                Target::Album(id) => {
                    self.open_album(id);
                    return;
                }
                Target::Playlist(id) => {
                    self.open_playlist(id);
                    return;
                }
                _ => {}
            }
        }
        let ids = match &document.target {
            Target::Track(id) => {
                if self.state.borrow().track(id).is_none() {
                    if let Some(mut track) = self.search.tracks.borrow().get(id).cloned() {
                        if let Some(art) = track
                            .artwork
                            .as_ref()
                            .and_then(|p| self.search.artwork.borrow().get(p).cloned())
                        {
                            track.artwork = Some(art);
                        }
                        self.state.borrow_mut().merge(vec![track]);
                    }
                }
                if self.state.borrow().track(id).is_none() {
                    return;
                }
                vec![id.clone()]
            }
            Target::Album(id) => {
                let state = self.state.borrow();
                let Some(album) = state.track(id) else { return };
                let key = album.album_key();
                let mut tracks: Vec<_> = state
                    .tracks
                    .iter()
                    .filter(|t| t.kind == Kind::Music && t.album_key() == key)
                    .collect();
                tracks.sort_by_key(|t| t.album_order());
                tracks.into_iter().map(|t| t.id.clone()).collect()
            }
            Target::Playlist(id) => self
                .state
                .borrow()
                .playlist(id)
                .map(|p| p.tracks.clone())
                .unwrap_or_default(),
            Target::Show(_) => unreachable!(),
        };
        if action == 1 {
            let mut state = self.state.borrow_mut();
            for id in ids {
                if !state.queue.contains(&id) {
                    state.queue.push(id);
                }
            }
            drop(state);
            self.persist();
            self.notice(&self.tr("Added to queue"), false, false);
        } else if let Some(first) = ids.first() {
            if !matches!(document.target, Target::Track(_)) {
                self.state.borrow_mut().queue = ids.clone();
            }
            self.play_track(first, None);
        }
    }

    pub(super) fn global_artwork_urls(&self) -> Vec<String> {
        let rows = self.search.rows.borrow();
        (0..self.search.model.n_items())
            .filter_map(|position| self.search.model.string(position))
            .filter_map(|key| rows.get(key.as_str()))
            .filter_map(|row| match row {
                Row::Item(d) => d.artwork.clone(),
                _ => None,
            })
            .filter(|p| {
                p.starts_with("http")
                    && !self.search.artwork.borrow().contains_key(p)
                    && !self.search.artwork_failed.borrow().contains(p)
            })
            .take(30)
            .collect()
    }
    fn toggle_search_subscription(self: &Rc<Self>, url: &str) {
        let subscribed = self
            .state
            .borrow()
            .subscriptions
            .iter()
            .any(|s| s.url == url);
        if subscribed {
            self.unsubscribe(url);
        } else {
            self.subscribe(url);
        }
        self.render_global_search();
    }
    pub(super) fn global_artwork(&self, uri: &str, path: &str) {
        self.search
            .artwork
            .borrow_mut()
            .insert(uri.into(), path.into());
        if *self.page.borrow() == "search" {
            self.render_global_search();
        }
    }
    pub(super) fn global_artwork_failed(&self, uri: &str) {
        self.search.artwork_failed.borrow_mut().insert(uri.into());
    }
    pub(super) fn global_notice(&self, text: &str, error: bool) {
        let banner: gtk::Box = object(&self.search.ui, "global_notice");
        object::<gtk::Label>(&self.search.ui, "global_message").set_label(text);
        if error {
            banner.add_css_class("error");
        } else {
            banner.remove_css_class("error");
        }
        banner.set_visible(!text.is_empty());
    }
}
