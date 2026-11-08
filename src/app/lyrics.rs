use super::*;

#[derive(Default)]
pub(super) struct Controls {
    key: RefCell<Option<(String, String)>>,
    parsed: RefCell<crate::lyrics::Lyrics>,
    ranges: RefCell<Vec<(i32, i32)>>,
    active: Cell<Option<usize>>,
}

impl App {
    pub(super) fn setup_lyrics(self: &Rc<Self>) {
        let view: gtk::TextView = object(&self.ui, "lyrics_timed");
        let tag = gtk::TextTag::builder()
            .name("current")
            .foreground("#c2dfa3")
            .weight(700)
            .build();
        view.buffer().tag_table().add(&tag);
        let follow: gtk::ToggleButton = object(&self.ui, "lyrics_follow");
        let weak = Rc::downgrade(self);
        follow.connect_toggled(move |button| {
            if button.is_active()
                && let Some(app) = weak.upgrade()
            {
                app.sync_lyrics(true);
            }
        });
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        wheel.set_name(Some("lyrics-wheel"));
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let follow_weak = follow.downgrade();
        wheel.connect_scroll(move |_, _, _| {
            if let Some(follow) = follow_weak.upgrade() {
                follow.set_active(false);
            }
            glib::Propagation::Proceed
        });
        let scroller: gtk::ScrolledWindow = object(&self.ui, "lyrics_scroller");
        scroller.add_controller(wheel);
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(move |_, _, _, _| follow.set_active(false));
        scroller.vscrollbar().add_controller(click);
        let weak = Rc::downgrade(self);
        view.connect_map(move |_| {
            if let Some(app) = weak.upgrade() {
                app.sync_lyrics(true);
            }
        });
    }

    pub(super) fn render_lyrics(&self, track: Option<&Track>, status: &str) {
        let raw = track
            .filter(|t| !t.instrumental)
            .and_then(|t| {
                t.synced_lyrics
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or(t.lyrics.as_deref())
            })
            .filter(|s| !s.trim().is_empty());
        let key = raw.map(|text| (track.unwrap().id.clone(), text.to_string()));
        if *self.lyrics.key.borrow() != key {
            let parsed = raw.map(crate::lyrics::Lyrics::parse).unwrap_or_default();
            let mut text = String::new();
            let mut ranges = Vec::new();
            let mut offset = 0;
            for line in &parsed.lines {
                let end = offset + line.text.chars().count() as i32;
                ranges.push((offset, end));
                text.push_str(&line.text);
                text.push('\n');
                offset = end + 1;
            }
            object::<gtk::TextView>(&self.ui, "lyrics_timed")
                .buffer()
                .set_text(&text);
            *self.lyrics.ranges.borrow_mut() = ranges;
            *self.lyrics.parsed.borrow_mut() = parsed;
            *self.lyrics.key.borrow_mut() = key;
            self.lyrics.active.set(None);
        }
        let timed = !self.lyrics.parsed.borrow().lines.is_empty();
        object::<gtk::ScrolledWindow>(&self.ui, "lyrics_scroller").set_visible(timed);
        object::<gtk::ToggleButton>(&self.ui, "lyrics_follow").set_visible(timed);
        let label: gtk::Label = object(&self.ui, "lyrics_text");
        label.set_visible(!timed);
        if raw.is_some() && !timed {
            label.set_label(&self.lyrics.parsed.borrow().plain);
        } else {
            label.set_label(status);
        }
        self.sync_lyrics(false);
    }

    pub(super) fn sync_lyrics(&self, force: bool) {
        let audio = self.audio.borrow();
        let position = if audio.loaded {
            audio.position()
        } else {
            self.state.borrow().position
        };
        drop(audio);
        let active = self.lyrics.parsed.borrow().active(position);
        if !force && self.lyrics.active.get() == active {
            return;
        }
        self.lyrics.active.set(active);
        let view: gtk::TextView = object(&self.ui, "lyrics_timed");
        let buffer = view.buffer();
        let tag = buffer.tag_table().lookup("current").unwrap();
        buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
        if let Some((start, end)) = active.and_then(|i| self.lyrics.ranges.borrow().get(i).copied())
        {
            let mut start = buffer.iter_at_offset(start);
            buffer.apply_tag(&tag, &start, &buffer.iter_at_offset(end));
            if object::<gtk::ToggleButton>(&self.ui, "lyrics_follow").is_active() {
                view.scroll_to_iter(&mut start, 0.1, true, 0.0, 0.5);
            }
        }
    }
}
