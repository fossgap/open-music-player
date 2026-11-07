mod deck;
mod gain;
#[cfg(test)]
mod tests;
pub(crate) mod visualization;

use crate::model::PlaybackSettings;
use deck::Deck;
use std::{cell::Cell, collections::HashMap};

#[derive(Debug)]
pub enum Event {
    End,
    Advanced(String),
    Error(String),
    Buffering(i32),
    Waveform(Vec<f64>),
    Spectrum(Vec<f64>),
    Ready,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NextTrack {
    pub id: String,
    pub uri: String,
    pub headers: HashMap<String, String>,
    pub duration: f64,
    pub following: Vec<NextTrack>,
}

impl NextTrack {
    pub fn same_source(&self, other: &Self) -> bool {
        self.id == other.id && self.uri == other.uri && self.headers == other.headers
    }
    fn chain(&self) -> std::collections::VecDeque<NextTrack> {
        let mut head = self.clone();
        let following = std::mem::take(&mut head.following);
        std::iter::once(head).chain(following).collect()
    }
}

pub struct Audio {
    deck: Deck,
    standby: Option<Deck>,
    outgoing: Option<Deck>,
    next: Option<NextTrack>,
    fade_seconds: f64,
    fade_end: Option<f64>,
    failed_preload: Option<String>,
    settings: PlaybackSettings,
    volume: Cell<f64>,
    pub playing: bool,
    pub loaded: bool,
    spotify: Option<crate::spotify::playback::Playback>,
    spotify_active: bool,
    rate: f64,
}

impl Audio {
    pub fn details(&self) -> crate::audio_info::Details {
        if !self.loaded {
            return Default::default();
        }
        if self.spotify_active {
            // This is the native source PCM rate, not the device/output rate.
            return crate::audio_info::Details {
                sample_rate: Some(44_100),
                ..Default::default()
            };
        }
        self.deck.details()
    }

    pub fn new() -> Result<Self, String> {
        gst::init().map_err(|e| e.to_string())?;
        Ok(Self {
            deck: Deck::new()?,
            standby: None,
            outgoing: None,
            next: None,
            fade_seconds: 0.0,
            fade_end: None,
            failed_preload: None,
            settings: PlaybackSettings::default(),
            volume: Cell::new(0.75),
            playing: false,
            loaded: false,
            spotify: None,
            spotify_active: false,
            rate: 1.0,
        })
    }

    pub fn configure(&mut self, settings: PlaybackSettings) {
        if self.settings != settings {
            self.standby.take();
            self.failed_preload = None;
        }
        self.settings = settings;
        self.deck.gain(settings.replay_gain);
        if let Some(deck) = self.outgoing.as_ref() {
            deck.gain(settings.replay_gain);
        }
        if let Some(spotify) = self.spotify.as_mut() {
            spotify.configure(settings);
        }
    }

    pub fn spotify_ready(&self) -> bool {
        self.spotify.is_some()
    }
    pub fn attach_spotify(&mut self, mut playback: crate::spotify::playback::Playback) {
        if self.spotify_active {
            self.stop();
        }
        playback.configure(self.settings);
        playback.volume(self.volume.get());
        self.spotify = Some(playback);
    }
    pub fn detach_spotify(&mut self) {
        if self.spotify_active {
            self.stop();
        }
        self.spotify.take();
    }
    pub fn load_spotify(
        &mut self,
        uri: &str,
        position: f64,
        duration: f64,
        autoplay: bool,
    ) -> Result<(), String> {
        self.stop();
        self.spotify
            .as_mut()
            .ok_or("Connect your Spotify account first.")?
            .load(uri, position, duration, autoplay)?;
        // Missing optional tempo support must not prevent normal playback.
        let _ = self.spotify.as_mut().unwrap().set_rate(self.rate);
        self.spotify_active = true;
        self.loaded = true;
        self.playing = autoplay;
        Ok(())
    }

    pub fn load_queued(
        &mut self,
        uri: &str,
        position: f64,
        autoplay: bool,
        headers: HashMap<String, String>,
        next: Option<NextTrack>,
        fade_seconds: f64,
    ) -> Result<(), String> {
        self.stop();
        self.next = next.clone();
        self.fade_seconds = fade_seconds;
        self.deck.gain(self.settings.replay_gain);
        self.deck.set_rate(self.rate, false)?;
        self.deck.volume(self.volume.get());
        self.deck.load(
            NextTrack {
                id: String::new(),
                uri: uri.into(),
                headers,
                duration: 0.0,
                following: Vec::new(),
            },
            position,
            autoplay,
            if self.settings.gapless && fade_seconds == 0.0 {
                next
            } else {
                None
            },
            false,
        )?;
        self.loaded = true;
        self.playing = autoplay;
        Ok(())
    }

    /// Called when the selected successor changes and on ticks to prepare a fade near its end.
    pub fn prepare_next(&mut self, next: Option<NextTrack>, fade_seconds: f64) {
        if self.spotify_active {
            if let Some(spotify) = self.spotify.as_mut() {
                spotify.prepare_next(next);
            }
            return;
        }
        if !self.loaded {
            return;
        }
        if self.next != next || self.fade_seconds != fade_seconds {
            self.standby.take();
            self.failed_preload = None;
            self.next = next.clone();
            self.fade_seconds = fade_seconds;
        }
        let native_next = if self.settings.gapless && fade_seconds == 0.0 {
            next.clone()
        } else {
            None
        };
        let _ = self.deck.set_next(native_next, self.playing);
        if fade_seconds <= 0.0 || self.outgoing.is_some() || self.standby.is_some() {
            return;
        }
        let Some(next) = next else {
            return;
        };
        let duration = self.duration();
        if duration <= 0.0
            || duration - self.position() > fade_seconds + 10.0
            || self.failed_preload.as_ref() == Some(&next.id)
        {
            return;
        }
        let prepared = Deck::new().and_then(|mut deck| {
            deck.gain(self.settings.replay_gain);
            deck.volume(self.volume.get());
            deck.load(next.clone(), 0.0, false, None, true)?;
            Ok(deck)
        });
        match prepared {
            Ok(deck) => self.standby = Some(deck),
            Err(_) => self.failed_preload = Some(next.id),
        }
    }

    pub fn set_playing(&mut self, playing: bool) -> Result<(), String> {
        if !self.loaded {
            return Ok(());
        }
        if self.spotify_active {
            self.spotify.as_mut().unwrap().set_playing(playing);
        } else {
            self.deck.set_playing(playing)?;
            if let Some(deck) = self.outgoing.as_ref() {
                deck.set_playing(playing)?;
            }
        }
        self.playing = playing;
        Ok(())
    }
    pub fn rate(&self) -> f64 {
        if !self.loaded {
            self.rate
        } else if self.spotify_active {
            self.spotify.as_ref().unwrap().rate()
        } else {
            self.deck.rate()
        }
    }
    pub fn record_spotify(&self, capture: crate::downloads::Capture) -> Result<(), String> {
        if !self.spotify_active || !self.loaded {
            return Err("Start Spotify playback before recording.".into());
        }
        self.spotify.as_ref().unwrap().record(capture);
        Ok(())
    }
    pub fn can_change_rate(&self) -> bool {
        !self.loaded
            || if self.spotify_active {
                self.spotify.as_ref().unwrap().can_change_rate()
            } else {
                self.deck.can_change_rate()
            }
    }
    pub fn set_rate(&mut self, rate: f64) -> Result<(), String> {
        if !rate.is_finite() || !(0.5..=2.0).contains(&rate) {
            return Err("Choose a playback speed between 0.5× and 2×.".into());
        }
        if self.spotify_active {
            self.spotify.as_mut().unwrap().set_rate(rate)?;
        } else {
            self.standby.take();
            self.outgoing.take();
            self.fade_end = None;
            self.deck.clear_fade();
            self.deck.set_next(None, self.playing)?;
            self.deck.set_rate(rate, self.playing)?;
        }
        self.rate = rate;
        Ok(())
    }
    pub fn stop(&mut self) {
        if self.spotify_active {
            if let Some(spotify) = self.spotify.as_mut() {
                spotify.stop();
            }
        }
        self.spotify_active = false;
        self.standby.take();
        self.outgoing.take();
        self.next = None;
        self.fade_end = None;
        self.failed_preload = None;
        self.deck.stop();
        self.playing = false;
        self.loaded = false;
    }
    pub fn volume(&self, value: f64) {
        let value = value.clamp(0.0, 1.0);
        self.volume.set(value);
        if let Some(spotify) = self.spotify.as_ref() {
            spotify.volume(value);
        }
        self.deck.volume(value);
        for deck in [&self.standby, &self.outgoing].into_iter().flatten() {
            deck.volume(value);
        }
    }
    pub fn position(&self) -> f64 {
        if self.spotify_active {
            self.spotify.as_ref().unwrap().position()
        } else {
            self.deck.position()
        }
    }
    pub fn duration(&self) -> f64 {
        if self.spotify_active {
            self.spotify.as_ref().unwrap().duration()
        } else {
            self.deck.duration()
        }
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid seek position".into());
        }
        if self.spotify_active {
            self.spotify.as_ref().unwrap().seek(seconds);
            return Ok(());
        }
        self.outgoing.take();
        self.fade_end = None;
        self.deck.clear_fade();
        self.deck.seek(seconds, self.playing)
    }

    pub fn poll(&mut self) -> Vec<Event> {
        if self.spotify_active {
            let spotify = self.spotify.as_mut().unwrap();
            let events = spotify.poll();
            self.playing = spotify.playing;
            if events.iter().any(|e| matches!(e, Event::Error(_))) {
                self.loaded = false;
            }
            return events;
        }
        let mut events = self.deck.poll(self.playing);
        if events.iter().any(|e| matches!(e, Event::Advanced(_))) {
            self.next = None;
        }
        if events.iter().any(|e| matches!(e, Event::Error(_))) {
            self.stop();
            return events;
        }
        if let Some(standby) = self.standby.as_mut() {
            if standby
                .poll(false)
                .iter()
                .any(|e| matches!(e, Event::End | Event::Error(_)))
            {
                self.failed_preload = standby.current.as_ref().map(|t| t.id.clone());
                self.standby.take();
            }
        }
        let remaining = (self.duration() - self.position()).max(0.0);
        if self.playing
            && self
                .standby
                .as_ref()
                .is_some_and(|d| d.ready && !d.buffering)
            && remaining <= self.fade_seconds
            && self.duration() > 0.0
        {
            let incoming = self.standby.take().unwrap();
            let seconds = remaining
                .min(self.fade_seconds)
                .min(incoming.duration() / 2.0)
                .max(0.01);
            incoming.fade(0.0, seconds, 0.0, 1.0);
            if incoming.set_playing(true).is_ok() {
                self.deck.fade(self.position(), seconds, 1.0, 0.0);
                let id = incoming.current.as_ref().unwrap().id.clone();
                self.outgoing = Some(std::mem::replace(&mut self.deck, incoming));
                self.fade_end = Some(seconds);
                self.next = None;
                events.retain(|e| !matches!(e, Event::End | Event::Ready));
                events.push(Event::Advanced(id));
                events.push(Event::Ready);
            }
        }
        if let Some(outgoing) = self.outgoing.as_mut() {
            let _ = outgoing.poll(self.playing);
        }
        if self.fade_end.is_some_and(|end| self.position() >= end) {
            self.outgoing.take();
            self.fade_end = None;
            self.deck.clear_fade();
        }
        if events.iter().any(|e| matches!(e, Event::End)) {
            self.playing = false;
        }
        events
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
    }
}
