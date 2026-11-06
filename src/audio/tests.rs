use super::*;
use crate::model::ReplayGain;
use gst::prelude::*;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn wav(path: &Path, seconds: f64) -> NextTrack {
    let samples = (seconds * 48000.0) as u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + samples * 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend(96000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((samples * 2).to_le_bytes());
    for _ in 0..samples {
        bytes.extend(12000i16.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
    NextTrack {
        id: path.file_name().unwrap().to_string_lossy().into(),
        uri: url::Url::from_file_path(path).unwrap().into(),
        headers: HashMap::new(),
        duration: seconds,
        following: Vec::new(),
    }
}

#[derive(Default)]
struct Capture {
    frames: usize,
    peaks: Vec<(Instant, f64)>,
}

fn capture(deck: &Deck) -> Arc<Mutex<Capture>> {
    let captured = Arc::new(Mutex::new(Capture::default()));
    let copy = captured.clone();
    let sink = deck.output.clone();
    sink.connect("handoff", false, move |values| {
        let buffer = values[1].get::<gst::Buffer>().unwrap();
        let pad = values[2].get::<gst::Pad>().unwrap();
        let caps = pad.current_caps().unwrap();
        let caps = caps.structure(0).unwrap();
        let format = caps.get::<String>("format").unwrap();
        let channels = caps.get::<i32>("channels").unwrap() as usize;
        let bytes = buffer.map_readable().unwrap();
        let (size, peak) = match format.as_str() {
            "S16LE" => (
                2,
                bytes
                    .as_slice()
                    .chunks_exact(2)
                    .map(|b| f64::from(i16::from_le_bytes(b.try_into().unwrap())).abs() / 32768.0)
                    .fold(0.0, f64::max),
            ),
            "F32LE" => (
                4,
                bytes
                    .as_slice()
                    .chunks_exact(4)
                    .map(|b| f64::from(f32::from_le_bytes(b.try_into().unwrap())).abs())
                    .fold(0.0, f64::max),
            ),
            "F64LE" => (
                8,
                bytes
                    .as_slice()
                    .chunks_exact(8)
                    .map(|b| f64::from_le_bytes(b.try_into().unwrap()).abs())
                    .fold(0.0, f64::max),
            ),
            _ => panic!("Unexpected PCM format {format}"),
        };
        let mut captured = copy.lock().unwrap();
        captured.frames += bytes.len() / size / channels;
        captured.peaks.push((Instant::now(), peak));
        None
    });
    captured
}

fn poll_until(audio: &mut Audio, condition: impl Fn(&Audio, &[Event]) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut all = Vec::new();
    while Instant::now() < deadline {
        let events = audio.poll();
        for event in &events {
            if let Event::Error(e) = event {
                panic!("Playback error: {e}");
            }
        }
        all.extend(events);
        if condition(audio, &all) {
            return all;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Audio condition timed out at position {}", audio.position());
}

#[test]
fn playback_speed_changes_output_duration_and_survives_pause_and_seek() {
    let dir = tempfile::tempdir().unwrap();
    let track = wav(&dir.path().join("speed.wav"), 2.0);
    for rate in [0.5, 2.0] {
        let mut audio = Audio::new().unwrap();
        audio.set_rate(rate).unwrap();
        let captured = capture(&audio.deck);
        let start = Instant::now();
        audio
            .load_queued(&track.uri, 0.0, true, HashMap::new(), None, 0.0)
            .unwrap();
        poll_until(&mut audio, |_, events| {
            events.iter().any(|e| matches!(e, Event::End))
        });
        let seconds = captured.lock().unwrap().frames as f64 / 48000.0;
        assert!(
            (seconds - 2.0 / rate).abs() < 0.15,
            "rate {rate}: {seconds}s PCM"
        );
        assert!((start.elapsed().as_secs_f64() - 2.0 / rate).abs() < 0.8);
        assert_eq!(audio.rate(), rate);
    }
    let mut audio = Audio::new().unwrap();
    audio
        .load_queued(&track.uri, 0.0, true, HashMap::new(), None, 0.0)
        .unwrap();
    poll_until(&mut audio, |a, _| a.position() > 0.2);
    audio.set_playing(false).unwrap();
    audio.set_rate(1.5).unwrap();
    audio.seek(0.7).unwrap();
    poll_until(&mut audio, |a, _| {
        a.deck.ready && (a.position() - 0.7).abs() < 0.05
    });
    std::thread::sleep(Duration::from_millis(100));
    audio.poll();
    assert!((audio.position() - 0.7).abs() < 0.05);
    assert_eq!(audio.rate(), 1.5);
    assert!(!audio.playing);
    audio.set_playing(true).unwrap();
    poll_until(&mut audio, |a, _| a.position() > 1.0);
    assert_eq!(audio.rate(), 1.5);
    for invalid in [0.0, 3.0, f64::NAN] {
        assert!(audio.set_rate(invalid).is_err());
    }
}

#[test]
fn unseekable_stream_uses_normal_speed_without_losing_the_preference() {
    let server = crate::downloads::tests::Server::new(
        crate::downloads::tests::wav(5),
        "audio/wav",
        false,
        Duration::from_millis(15),
    );
    let mut audio = Audio::new().unwrap();
    audio.set_rate(1.5).unwrap();
    audio
        .load_queued(&server.uri, 0.0, true, HashMap::new(), None, 0.0)
        .unwrap();
    poll_until(&mut audio, |a, _| a.position() > 0.15);
    assert_eq!(audio.rate(), 1.0);
    assert!(!audio.can_change_rate());
    assert!(audio.set_rate(2.0).is_err());
    let dir = tempfile::tempdir().unwrap();
    let track = wav(&dir.path().join("after-radio.wav"), 2.0);
    audio
        .load_queued(&track.uri, 0.0, true, HashMap::new(), None, 0.0)
        .unwrap();
    poll_until(&mut audio, |a, _| a.position() > 0.15);
    assert_eq!(audio.rate(), 1.5);
    assert!(audio.can_change_rate());
}

#[test]
fn gapless_handoff_keeps_all_pcm_and_changes_current_once() {
    let dir = tempfile::tempdir().unwrap();
    let first = wav(&dir.path().join("one.wav"), 0.8);
    let next = wav(&dir.path().join("two.wav"), 0.8);
    let mut audio = Audio::new().unwrap();
    audio.volume(1.0);
    let captured = capture(&audio.deck);
    audio
        .load_queued(
            &first.uri,
            0.0,
            true,
            HashMap::new(),
            Some(next.clone()),
            0.0,
        )
        .unwrap();
    let events = poll_until(&mut audio, |_, events| {
        events.iter().any(|e| matches!(e, Event::End))
    });
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Advanced(id) if id == &next.id))
            .count(),
        1
    );
    assert_eq!(events.iter().filter(|e| matches!(e, Event::End)).count(), 1);
    let captured = captured.lock().unwrap();
    assert_eq!(
        captured.frames, 76800,
        "Neither end nor beginning may be discarded"
    );
    assert!(captured.peaks.iter().all(|(_, p)| *p > 0.3));
    let intervals: Vec<_> = captured
        .peaks
        .windows(2)
        .map(|w| w[1].0.duration_since(w[0].0).as_secs_f64())
        .collect();
    assert!(
        intervals.iter().all(|i| *i < 0.16),
        "No output-device restart at the boundary: {intervals:?}"
    );
}

#[test]
fn consecutive_gapless_transitions_work_without_restarting_output() {
    let dir = tempfile::tempdir().unwrap();
    let first = wav(&dir.path().join("one.wav"), 0.8);
    let mut second = wav(&dir.path().join("two.wav"), 0.8);
    let third = wav(&dir.path().join("three.wav"), 0.8);
    second.following.push(third.clone());
    let mut audio = Audio::new().unwrap();
    let captured = capture(&audio.deck);
    audio
        .load_queued(
            &first.uri,
            0.0,
            true,
            HashMap::new(),
            Some(second.clone()),
            0.0,
        )
        .unwrap();
    let events = poll_until(&mut audio, |_, events| {
        events.iter().any(|e| matches!(e, Event::Advanced(_)))
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Advanced(id) if id == &second.id))
    );
    audio.prepare_next(Some(third.clone()), 0.0);

    let events = poll_until(&mut audio, |_, events| {
        events.iter().any(|e| matches!(e, Event::End))
    });

    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Advanced(id) if id == &third.id))
    );
    assert_eq!(captured.lock().unwrap().frames, 115200);
}

#[test]
fn gapless_duration_stays_with_the_audible_track() {
    let dir = tempfile::tempdir().unwrap();
    let first = wav(&dir.path().join("long.wav"), 2.0);
    let next = wav(&dir.path().join("short.wav"), 0.5);
    let mut audio = Audio::new().unwrap();
    audio
        .load_queued(&first.uri, 0.0, true, HashMap::new(), Some(next), 0.0)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        let events = audio.poll();
        if events.iter().any(|e| matches!(e, Event::Advanced(_))) {
            assert!((audio.duration() - 0.5).abs() < 0.01);
            return;
        }
        if audio.position() > 0.1 {
            assert!(
                (audio.duration() - 2.0).abs() < 0.01,
                "Prepared song changed the playing duration at {}",
                audio.position()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Missing handoff");
}

#[test]
fn crossfade_overlaps_audio_and_pause_seek_stop_control_both_decks() {
    let dir = tempfile::tempdir().unwrap();
    let first = wav(&dir.path().join("one.wav"), 3.0);
    let next = wav(&dir.path().join("two.wav"), 3.0);
    let mut audio = Audio::new().unwrap();
    audio.volume(1.0);
    let outgoing = capture(&audio.deck);
    audio
        .load_queued(
            &first.uri,
            0.0,
            true,
            HashMap::new(),
            Some(next.clone()),
            1.0,
        )
        .unwrap();
    poll_until(&mut audio, |a, _| a.duration() > 0.0);
    audio.prepare_next(Some(next.clone()), 1.0);
    let incoming = capture(audio.standby.as_ref().unwrap());
    let events = poll_until(&mut audio, |_, e| {
        e.iter().any(|e| matches!(e, Event::Advanced(_)))
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Advanced(id) if id == &next.id))
    );
    assert!(audio.outgoing.is_some());
    poll_until(&mut audio, |a, _| a.position() > 0.45);
    audio.set_playing(false).unwrap();
    let position = audio.position();
    std::thread::sleep(Duration::from_millis(120));
    assert!((audio.position() - position).abs() < 0.03);
    assert_eq!(
        audio.outgoing.as_ref().unwrap().pipeline.current_state(),
        gst::State::Paused
    );
    let out = outgoing.lock().unwrap();
    let incoming = incoming.lock().unwrap();
    assert!(out.peaks.iter().any(|(_, p)| *p > 0.05 && *p < 0.3));
    assert!(incoming.peaks.iter().any(|(_, p)| *p > 0.02 && *p < 0.3));
    let in_first = incoming.peaks.iter().find(|(_, p)| *p > 0.01).unwrap().0;
    assert!(
        out.peaks.last().unwrap().0 > in_first,
        "Both songs must be audible during the overlap"
    );
    drop(incoming);
    drop(out);
    audio.volume(0.4);
    assert!(
        (audio
            .outgoing
            .as_ref()
            .unwrap()
            .pipeline
            .property::<f64>("volume")
            - 0.4)
            .abs()
            < 0.000001
    );
    audio.seek(1.5).unwrap();
    assert!(audio.outgoing.is_none());
    audio.stop();
    assert!(audio.standby.is_none() && !audio.loaded && !audio.playing);
}

#[test]
fn replacing_prepared_successor_never_plays_removed_song() {
    let dir = tempfile::tempdir().unwrap();
    let first = wav(&dir.path().join("one.wav"), 1.2);
    let removed = wav(&dir.path().join("removed.wav"), 0.6);
    let replacement = wav(&dir.path().join("replacement.wav"), 0.6);
    let mut audio = Audio::new().unwrap();
    audio
        .load_queued(
            &first.uri,
            0.0,
            true,
            HashMap::new(),
            Some(removed.clone()),
            0.0,
        )
        .unwrap();
    poll_until(&mut audio, |a, _| a.position() > 0.15);
    audio.prepare_next(Some(replacement.clone()), 0.0);
    let events = poll_until(&mut audio, |_, e| e.iter().any(|e| matches!(e, Event::End)));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Advanced(id) if id == &removed.id))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Advanced(id) if id == &replacement.id))
    );
}

#[test]
fn replay_gain_adjusts_decoded_samples_and_resets_for_untagged_audio() {
    let dir = tempfile::tempdir().unwrap();
    let plain = wav(&dir.path().join("plain.wav"), 0.4);
    let flac = dir.path().join("tagged.flac");
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(dir.path().join("plain.wav"))
            .args([
                "-metadata",
                "REPLAYGAIN_TRACK_GAIN=-6.0 dB",
                "-metadata",
                "REPLAYGAIN_TRACK_PEAK=0.4",
                "-metadata",
                "REPLAYGAIN_ALBUM_GAIN=-3.0 dB"
            ])
            .arg(&flac)
            .status()
            .unwrap()
            .success()
    );
    let mut audio = Audio::new().unwrap();
    audio.volume(1.0);
    audio.configure(PlaybackSettings {
        replay_gain: ReplayGain::Track,
        ..PlaybackSettings::default()
    });
    let captured = capture(&audio.deck);
    let uri = url::Url::from_file_path(&flac).unwrap();
    audio
        .load_queued(uri.as_str(), 0.0, true, HashMap::new(), Some(plain), 0.0)
        .unwrap();
    poll_until(&mut audio, |_, e| e.iter().any(|e| matches!(e, Event::End)));
    let captured = captured.lock().unwrap();
    let peaks: Vec<_> = captured.peaks.iter().map(|(_, p)| *p).collect();
    assert!(
        (peaks[0] - 12000.0 / 32768.0 * 10f64.powf(-6.0 / 20.0)).abs() < 0.002,
        "First tagged buffer already normalized: {peaks:?}"
    );
    assert!(
        (peaks.last().unwrap() - 12000.0 / 32768.0).abs() < 0.002,
        "Untagged successor must not inherit the gain: {peaks:?}"
    );
}
