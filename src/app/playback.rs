use super::*;
use crate::{audio::NextTrack, model::ReplayGain};

#[derive(Default)]
pub(super) struct Preparation {
    key: Option<(u64, String)>,
    resolved: Option<NextTrack>,
    requested: Option<Instant>,
}

impl App {
    pub(super) fn playback_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        let gapless: gtk::Switch = object(ui, "gapless");
        let fade: gtk::SpinButton = object(ui, "crossfade_seconds");
        let gain: gtk::DropDown = object(ui, "replay_gain");
        let settings = self.state.borrow().playback;
        gapless.set_active(settings.gapless);
        fade.set_value(f64::from(settings.crossfade_seconds));
        gain.set_model(Some(&gtk::StringList::new(&[
            &self.tr("Off"),
            &self.tr("Track gain"),
            &self.tr("Album gain"),
        ])));
        gain.set_selected(match settings.replay_gain {
            ReplayGain::Off => 0,
            ReplayGain::Track => 1,
            ReplayGain::Album => 2,
        });
        let weak = Rc::downgrade(self);
        gapless.connect_active_notify(move |switch| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().playback.gapless = switch.is_active();
                app.apply_playback_settings();
            }
        });
        let weak = Rc::downgrade(self);
        fade.connect_value_changed(move |spin| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().playback.crossfade_seconds =
                    spin.value_as_int().clamp(0, 12) as u32;
                app.apply_playback_settings();
            }
        });
        let weak = Rc::downgrade(self);
        gain.connect_selected_notify(move |picker| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().playback.replay_gain = match picker.selected() {
                    1 => ReplayGain::Track,
                    2 => ReplayGain::Album,
                    _ => ReplayGain::Off,
                };
                app.apply_playback_settings();
            }
        });
    }

    fn apply_playback_settings(self: &Rc<Self>) {
        self.audio
            .borrow_mut()
            .configure(self.state.borrow().playback);
        self.prepare_transition();
        self.persist();
    }

    fn transition_candidate(&self) -> Option<(Track, f64)> {
        candidate(&self.state.borrow())
    }

    pub(super) fn next_playback(&self) -> (Option<NextTrack>, f64) {
        let Some((track, fade)) = self.transition_candidate() else {
            return (None, 0.0);
        };
        let mut next = if services::youtube_url(&track.uri) {
            let prep = self.preparation.borrow();
            if prep.key.as_ref() == Some(&(self.play_generation.get(), track.id.clone())) {
                prep.resolved.clone()
            } else {
                None
            }
        } else {
            Some(source(&track))
        };
        if fade == 0.0 && self.state.borrow().playback.gapless {
            if let Some(next) = next.as_mut() {
                next.following = continuations(&self.state.borrow(), &track);
            }
        }
        (next, fade)
    }

    pub(super) fn prepare_transition(self: &Rc<Self>) {
        if !self.audio.borrow().loaded || self.resolving.get() {
            return;
        }
        if let Some((track, fade)) = self.transition_candidate() {
            {
                let mut prep = self.preparation.borrow_mut();
                if prep
                    .key
                    .as_ref()
                    .is_some_and(|key| key != &(self.play_generation.get(), track.id.clone()))
                    || prep
                        .requested
                        .is_some_and(|at| at.elapsed() > Duration::from_secs(120))
                {
                    *prep = Preparation::default();
                }
            }
            if services::youtube_url(&track.uri) {
                let audio = self.audio.borrow();
                let remaining = audio.duration() - audio.position();
                if audio.playing && audio.duration() > 0.0 && remaining <= (fade + 15.0).max(30.0) {
                    let key = (self.play_generation.get(), track.id.clone());
                    let mut prep = self.preparation.borrow_mut();
                    if prep.key.as_ref() != Some(&key) {
                        prep.key = Some(key.clone());
                        prep.resolved = None;
                        prep.requested = Some(Instant::now());
                        let account = self.state.borrow().youtube_account.clone();
                        let tx = self.tx.clone();
                        std::thread::spawn(move || {
                            let result = youtube_music::resolve(account.as_ref(), &track.uri).map(
                                |resolved| NextTrack {
                                    id: track.id,
                                    uri: resolved.uri,
                                    headers: resolved.headers,
                                    duration: track.duration,
                                    following: Vec::new(),
                                },
                            );
                            let _ = tx.send(Message::Prepared(key, result));
                        });
                    }
                }
            }
        } else {
            *self.preparation.borrow_mut() = Preparation::default();
        }
        let (next, fade) = self.next_playback();
        self.audio.borrow_mut().prepare_next(next, fade);
    }

    pub(super) fn prepared(&self, key: (u64, String), result: Result<NextTrack, String>) {
        let mut prep = self.preparation.borrow_mut();
        if prep.key.as_ref() == Some(&key) && key.0 == self.play_generation.get() {
            // A failed speculative request must not interrupt the current song.
            // Ordinary playback retries resolution when the song actually reaches the front.
            prep.resolved = result.ok();
        }
    }

    pub(super) fn advanced(self: &Rc<Self>, id: String) -> bool {
        let expected = self.state.borrow().adjacent(true, true);
        if expected.as_ref() != Some(&id) || self.state.borrow().track(&id).is_none() {
            // The queue may have been edited just as the audio thread committed its handoff.
            self.next(true);
            return false;
        }
        self.play_generation.set(self.play_generation.get() + 1);
        *self.preparation.borrow_mut() = Preparation::default();
        self.set_buffering(100);
        {
            let mut state = self.state.borrow_mut();
            state.current = Some(id);
            state.position = self.audio.borrow().position();
        }
        self.ensure_artwork();
        self.update_player();
        self.refresh();
        self.persist();
        true
    }
}

fn candidate(state: &State) -> Option<(Track, f64)> {
    candidate_after(state, state.current_track()?).map(|(track, fade)| (track.clone(), fade))
}

fn candidate_after<'a>(state: &'a State, current: &Track) -> Option<(&'a Track, f64)> {
    let settings = state.playback;
    if !settings.gapless && settings.crossfade_seconds == 0 {
        return None;
    }
    let next = state.track(&state.adjacent_from(Some(&current.id), true, true)?)?;
    let music = |t: &Track| {
        matches!(t.kind, Kind::Music | Kind::YoutubeMusic)
            || (t.kind == Kind::Stream && t.duration > 0.0)
    };
    let compatible = (music(current) && music(next))
        || (settings.gapless && current.kind == Kind::Spotify && next.kind == Kind::Spotify);
    if !compatible {
        return None;
    }
    let fade = if current.id != next.id && music(current) && music(next) {
        f64::from(settings.crossfade_seconds)
    } else {
        0.0
    };
    Some((next, fade))
}

fn source(track: &Track) -> NextTrack {
    NextTrack {
        id: track.id.clone(),
        uri: track.uri.clone(),
        headers: HashMap::new(),
        duration: track.duration,
        following: Vec::new(),
    }
}

fn continuations(state: &State, first: &Track) -> Vec<NextTrack> {
    let mut current = first;
    let mut following = Vec::new();
    // Bound read-ahead, including repeat-all/one. URLs needing account resolution
    // are prepared just in time by the worker instead of stored in this snapshot.
    for _ in 0..64 {
        let Some((next, fade)) = candidate_after(state, current) else {
            break;
        };
        if fade > 0.0 || services::youtube_url(&next.uri) {
            break;
        }
        following.push(source(next));
        current = next;
    }
    following
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_follow_queue_repeat_and_source_capabilities() {
        let a = Track::new("file:///a.flac", "A", Kind::Music);
        let b = Track::new("file:///b.flac", "B", Kind::Music);
        let podcast = Track::new("https://example.test/episode.mp3", "Episode", Kind::Podcast);
        let radio = Track::new("https://example.test/live", "Radio", Kind::Radio);
        let spotify = Track::new(
            "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            "Spotify",
            Kind::Spotify,
        );
        let youtube = Track::new(
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
            "Music",
            Kind::YoutubeMusic,
        );
        let mut state = State {
            current: Some(a.id.clone()),
            queue: vec![a.id.clone(), b.id.clone()],
            tracks: vec![
                a.clone(),
                b.clone(),
                podcast.clone(),
                radio.clone(),
                spotify.clone(),
                youtube.clone(),
            ],
            ..State::default()
        };
        state.playback.crossfade_seconds = 5;
        assert_eq!(candidate(&state).unwrap().1, 5.0);
        for next in [&podcast, &radio, &spotify] {
            state.queue[1] = next.id.clone();
            assert!(candidate(&state).is_none());
        }
        state.queue[1] = youtube.id.clone();
        assert_eq!(candidate(&state).unwrap().0.id, youtube.id);
        state.repeat = Repeat::One;
        let (next, fade) = candidate(&state).unwrap();
        assert_eq!(next.id, a.id);
        assert_eq!(fade, 0.0);
        state.repeat = Repeat::Off;
        state.playback.gapless = false;
        state.playback.crossfade_seconds = 0;
        assert!(candidate(&state).is_none());
        state.playback.gapless = true;
        state.current = Some(spotify.id.clone());
        state.queue = vec![spotify.id.clone()];
        state.repeat = Repeat::All;
        assert_eq!(candidate(&state).unwrap().1, 0.0);
        state.current = Some(podcast.id.clone());
        state.queue = vec![podcast.id];
        assert!(
            candidate(&state).is_none(),
            "Podcast resume and speech must use the regular path"
        );
    }
}
