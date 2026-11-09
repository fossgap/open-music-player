#!/usr/bin/env python3
"""Capture live README screenshots from the app running normally.

The application runs with your real library (~/.local/share/open-music-player)
and the real live services: Radio Browser stations, Apple Podcasts discovery,
and YouTube Music search are fetched over the network exactly as in normal
use. Nothing is mocked. The only test aids are the Xvfb display, a silent
audio sink (OPEN_MUSIC_FAKE_SINK=1, so capture does not play through your
speakers), and D-Bus actions in place of mouse clicks.

Run with `make screenshots`. Requires a populated library, network access,
and YouTube Music support (scripts/setup-youtube-music.sh or
OPEN_MUSIC_PYTHON).

Usage: python3 scripts/capture-screenshots.py [OUTPUT_DIR]
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get(
    'OPEN_MUSIC_BINARY', ROOT / 'target/debug/open-music-player')).resolve()
OUT = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'docs/screenshots'
DATA = Path(os.environ.get('OPEN_MUSIC_DATA_DIR',
                           Path.home() / '.local/share/open-music-player'))
DEST = 'org.mpris.MediaPlayer2.OpenMusicPlayer'
PLAYER = 'org.mpris.MediaPlayer2.Player'

for dependency in ['ffmpeg', 'gdbus', 'xdotool', 'import']:
    if not shutil.which(dependency):
        raise SystemExit(f'Missing screenshot dependency: {dependency}')
if not BINARY.is_file():
    raise SystemExit(f'Application binary not found: {BINARY} (run: cargo build --locked)')
if not (DATA / 'library.json').is_file():
    raise SystemExit(f'No library at {DATA} — run the app and add music first')


def call(interface, method, *args, dest=DEST, path='/org/mpris/MediaPlayer2'):
    return subprocess.check_output(
        ['gdbus', 'call', '--session', '--dest', dest, '--object-path', path,
         '--method', interface + '.' + method, *map(str, args)],
        text=True, stderr=subprocess.PIPE, timeout=10)


def prop(name):
    return call('org.freedesktop.DBus.Properties', 'Get', PLAYER, name)


def action(name, value=None):
    params = '[]' if value is None else '[<' + json.dumps(value) + '>]'
    return call('org.gtk.Actions', 'Activate', name, params, '{}',
                dest='io.github.openmusic.Player', path='/io/github/openmusic/Player')


def window(title):
    return subprocess.check_output(
        ['xdotool', 'search', '--onlyvisible', '--name', '^' + re.escape(title) + '$'],
        text=True).splitlines()[-1]


def keys(*sequence):
    subprocess.run(['xdotool', 'key', '--clearmodifiers', *sequence], check=True)


def wait_until(predicate, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            if predicate():
                return True
        except (subprocess.SubprocessError, FileNotFoundError, json.JSONDecodeError):
            pass
        time.sleep(0.2)
    return False


def require(predicate, seconds, what):
    if not wait_until(predicate, seconds):
        raise AssertionError(f'Timed out waiting for {what}')


def snapshot_player(name, settle=0.8):
    time.sleep(settle)
    windows = subprocess.check_output(
        ['xdotool', 'search', '--onlyvisible', '--name', '^Open Music'], text=True).splitlines()
    subprocess.run(['import', '-window', windows[-1], str(OUT / name)], check=True)
    print('captured', name, flush=True)


def search(term):
    """Type into the page search entry, like a user pressing Ctrl+F."""
    subprocess.run(['xdotool', 'windowfocus', '--sync', window('Open Music Player')], check=True)
    keys('ctrl+f', 'ctrl+a')
    subprocess.run(['xdotool', 'type', '--clearmodifiers', term], check=True)
    keys('Return')


OUT.mkdir(parents=True, exist_ok=True)
state_path = DATA / 'library.json'


def state():
    return json.loads(state_path.read_text())


env = {**os.environ, 'OPEN_MUSIC_FAKE_SINK': '1',
       'GTK_A11Y': 'none', 'GTK_USE_PORTAL': '0', 'GIO_USE_VFS': 'local',
       'GDK_BACKEND': 'x11', 'GSK_RENDERER': 'cairo', 'LANGUAGE': 'en'}
env.pop('WAYLAND_DISPLAY', None)
# Point the YouTube bridge at an existing setup venv when present.
for candidate in [Path('/tmp/open-audio-youtube-test/bin/python'),
                  DATA / 'youtube-python/bin/python']:
    if 'OPEN_MUSIC_PYTHON' not in env and candidate.is_file():
        env['OPEN_MUSIC_PYTHON'] = str(candidate)

with tempfile.TemporaryDirectory(prefix='open-music-screenshots-') as temporary:
    temp = Path(temporary)
    runtime = temp / 'runtime'
    runtime.mkdir(mode=0o700)
    env['XDG_RUNTIME_DIR'] = str(runtime)
    log = (temp / 'app.log').open('w+')
    child = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
    try:
        music_tracks = [t for t in state()['tracks'] if t.get('kind') == 'music']
        assert music_tracks, 'Library has no music tracks to show'
        first = music_tracks[0]
        require(lambda: 'Stopped' in prop('PlaybackStatus') or 'Paused' in prop('PlaybackStatus'),
                45, 'app startup')
        main_window = window('Open Music Player')
        subprocess.run(['xdotool', 'windowsize', main_window, '1280', '860'], check=True)
        time.sleep(0.6)

        # Hero: real track playing, track list, Now playing with real
        # artwork, fetched lyrics and the live waveform.
        action('tracks')
        action('play-track', first['id'])
        require(lambda: 'Playing' in prop('PlaybackStatus'), 30, 'playback')
        require(lambda: int(re.search(r'int64 (\d+)', prop('Position'))[1]) > 200000,
                15, 'playback position')
        call(PLAYER, 'SetPosition',
             f"/org/mpris/MediaPlayer2/Track/{first['id']}", '30000000')
        action('details')
        snapshot_player('main-library.png', settle=9)  # let lyrics/artwork load

        # Visualizations on the real playing track.
        action('toggle-visualization')
        snapshot_player('visualization-spectrum.png', settle=2)
        action('toggle-visualization')
        snapshot_player('visualization-mood.png', settle=2.5)
        action('toggle-visualization')
        action('toggle-visualization')
        action('details')

        # Podcasts: live Apple Podcasts discovery.
        action('page', 'podcasts')
        action('podcast-search')
        snapshot_player('podcasts.png', settle=10)

        # Live radio: search Radio Browser for jazz, then play a station.
        action('page', 'radio')
        search('jazz')
        time.sleep(8)
        radios = [t for t in state()['tracks'] if t.get('kind') == 'radio']
        if radios:
            station = radios[0]
            action('play-track', station['id'])
            wait_until(lambda: 'Playing' in prop('PlaybackStatus')
                       and station['id'] in prop('Metadata'), 25)
        snapshot_player('radio.png', settle=3)

        # Album-art visualization: Skyrock live from Radio Browser.
        search('Skyrock')
        time.sleep(8)
        skyrock = sorted(
            (t for t in state()['tracks']
             if t.get('kind') == 'radio' and 'skyrock' in t.get('title', '').lower()),
            key=lambda t: t['title'].strip().lower() != 'skyrock')
        assert skyrock, 'Skyrock not found in Radio Browser results'
        playing = False
        for station in skyrock[:3]:
            action('play-track', station['id'])
            playing = wait_until(
                lambda s=station: 'Playing' in prop('PlaybackStatus')
                and s['id'] in prop('Metadata'), 25)
            if playing:
                break
        assert playing, 'Skyrock did not start playing'
        action('details')
        for _ in range(4):
            if wait_until(lambda: state().get('visualization') == 'album_art', 2):
                break
            action('toggle-visualization')
        require(lambda: state().get('visualization') == 'album_art', 10, 'album-art mode')
        snapshot_player('visualization-album-art.png', settle=6)
        action('details')
        action('toggle-visualization')  # album art -> waveform, as found

        # Streaming: live YouTube Music search.
        action('play-track', first['id'])
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'), 20)
        action('page', 'youtube')
        search('Daft Punk')
        snapshot_player('streaming.png', settle=20)

        # Mini player with the real track.
        action('play-track', first['id'])
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'), 20)
        action('mini')
        snapshot_player('mini-player.png')
        action('show')

        call('org.mpris.MediaPlayer2', 'Quit')
        assert child.wait(timeout=10) == 0
        log.seek(0)
        output = log.read()
        assert 'panicked' not in output, output[-4000:]
        print(f'PASS: live screenshots written to {OUT}')
    except Exception:
        log.seek(0)
        print(log.read()[-6000:])
        print('Application exit status:', child.poll())
        raise

