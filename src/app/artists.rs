use super::*;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use tests::exercise;

pub(super) struct Controls {
    pub model: gtk::StringList,
    pub cards: BoundItems,
    pub groups: RefCell<BTreeMap<String, Artist>>,
    pub selected: RefCell<Option<String>>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            model: gtk::StringList::new(&[]),
            cards: Default::default(),
            groups: Default::default(),
            selected: Default::default(),
        }
    }
}

#[derive(Default)]
pub(super) struct Artist {
    pub name: String,
    pub tracks: Vec<String>,
    pub artwork: Option<String>,
    pub matches: bool,
}

fn group_tracks(tracks: &[Track], query: &str) -> BTreeMap<String, Artist> {
    let mut tracks: Vec<_> = tracks.iter().filter(|t| t.kind == Kind::Music).collect();
    tracks.sort_by_key(|t| {
        let (artist, album) = t.album_key();
        (artist.to_lowercase(), album.to_lowercase(), t.album_order())
    });
    let mut groups: BTreeMap<String, Artist> = BTreeMap::new();
    for track in tracks {
        let mut names: Vec<_> = track
            .artists
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect();
        if names.is_empty() {
            names.push(track.artist.trim());
        }
        let matches = track.matches(query);
        let mut seen = HashSet::new();
        for name in names {
            let key = name.to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let artist = groups.entry(key.clone()).or_insert_with(|| Artist {
                name: name.into(),
                ..Artist::default()
            });
            artist.tracks.push(track.id.clone());
            artist.matches |= matches
                || query
                    .split_whitespace()
                    .all(|word| key.contains(&word.to_lowercase()));
            if artist.artwork.is_none() {
                artist.artwork = track.artwork.clone();
            }
        }
    }
    groups
}

impl App {
    pub(super) fn refresh_artists(&self, tracks: &[Track], query: &mut String) {
        let mut groups = group_tracks(tracks, query);
        let removed = self
            .artists
            .selected
            .borrow()
            .as_ref()
            .is_some_and(|id| !groups.contains_key(id));
        if removed {
            self.artists.selected.borrow_mut().take();
            *query = self.parent_query.borrow().clone();
            object::<gtk::SearchEntry>(&self.ui, "search").set_text(query);
            groups = group_tracks(tracks, query);
        }
        let ids = groups
            .iter()
            .filter(|(_, artist)| artist.matches)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        *self.artists.groups.borrow_mut() = groups;
        browse::sync_model(&self.artists.model, &ids);
    }

    pub(super) fn open_artist(self: &Rc<Self>, id: &str) {
        if *self.page.borrow() != "library" {
            self.navigate("library");
        }
        if !self.artists.groups.borrow().contains_key(id) {
            return;
        }
        *self.parent_query.borrow_mut() = object::<gtk::SearchEntry>(&self.ui, "search")
            .text()
            .to_string();
        self.album.borrow_mut().take();
        *self.artists.selected.borrow_mut() = Some(id.into());
        object::<gtk::SearchEntry>(&self.ui, "search").set_text("");
        self.refresh();
    }
}
