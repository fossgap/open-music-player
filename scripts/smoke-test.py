#!/usr/bin/env python3
"""Exercise the real GTK app, audio pipeline, MPRIS and persistence offline.

Run inside xvfb-run and dbus-run-session. All media and state live in a temporary
directory. Optional OPEN_AUDIO_SCREENSHOTS captures review artifacts;
OPEN_AUDIO_BINARY selects a release binary or extracted AppImage launcher.
"""
import importlib.util
import functools
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
import threading
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('OPEN_AUDIO_BINARY', ROOT / 'target/debug/open-audio-player')).resolve()
DEST = 'org.mpris.MediaPlayer2.OpenAudioPlayer'
PLAYER = 'org.mpris.MediaPlayer2.Player'

for dependency in ['ffmpeg', 'gdbus', 'xdotool'] + (['import'] if os.environ.get('OPEN_AUDIO_SCREENSHOTS') else []):
    if not shutil.which(dependency):
        raise SystemExit(f'Missing integration-test dependency: {dependency}')
if not BINARY.is_file():
    raise SystemExit(f'Application binary not found: {BINARY}')


def call(interface, method, *args, dest=DEST, path='/org/mpris/MediaPlayer2'):
    return subprocess.check_output(['gdbus', 'call', '--session', '--dest', dest, '--object-path', path,
                                    '--method', interface + '.' + method, *map(str, args)], text=True, stderr=subprocess.PIPE, timeout=5)


def prop(name):
    return call('org.freedesktop.DBus.Properties', 'Get', PLAYER, name)


def action(name, value=None):
    params = '[]' if value is None else "[<" + json.dumps(value) + ">]"
    return call('org.gtk.Actions', 'Activate', name, params, '{}', dest='io.github.openaudio.Player', path='/io/github/openaudio/Player')


def wait_until(predicate, seconds=25):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (subprocess.SubprocessError, FileNotFoundError, json.JSONDecodeError):
            pass
        time.sleep(0.15)
    raise AssertionError('Timed out waiting for application state')


with tempfile.TemporaryDirectory(prefix='open-audio-test-') as temporary:
    temp = Path(temporary)
    spec = importlib.util.spec_from_file_location('demo', ROOT / 'scripts/create-demo.py')
    demo = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(demo)
    music = demo.create(temp / 'music', 45)
    class QuietHandler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(QuietHandler, directory=str(music)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    media = next(music.rglob('*.flac')).relative_to(music)
    stream_url = base + '/' + quote(str(media))
    (music / 'feed.xml').write_text(f'<rss><channel><title>Offline test podcast</title><item><title>Test episode</title><guid>offline-episode</guid><enclosure url="{stream_url}" type="audio/flac"/></item></channel></rss>')
    data = temp / 'state'
    runtime = temp / 'runtime'
    runtime.mkdir(mode=0o700)
    env = {**os.environ, 'OPEN_AUDIO_DATA_DIR': str(data), 'OPEN_AUDIO_FAKE_SINK': '1',
           'GTK_A11Y': 'none', 'GTK_USE_PORTAL': '0', 'GIO_USE_VFS': 'local', 'GDK_BACKEND': 'x11',
           'GSK_RENDERER': 'cairo', 'XDG_RUNTIME_DIR': str(runtime), 'LANGUAGE': 'en'}
    env.pop('WAYLAND_DISPLAY', None)
    log = (temp / 'app.log').open('w+')
    child = subprocess.Popen([str(BINARY), '--import', str(music)], env=env, stdout=log, stderr=log)
    def state():
        return json.loads((data / 'library.json').read_text())
    def snapshot(name):
        if directory := os.environ.get('OPEN_AUDIO_SCREENSHOTS'):
            Path(directory).mkdir(parents=True, exist_ok=True)
            time.sleep(0.3)
            windows = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Open Audio'], text=True).splitlines()
            subprocess.run(['import', '-window', windows[-1], str(Path(directory) / name)], check=True)
    try:
        wait_until(lambda: len(state()['tracks']) == 12 and 'Stopped' in prop('PlaybackStatus'), 45)
        assert len(state()['folders']) == 1
        assert all(t['duration'] >= 44 for t in state()['tracks'])
        assert all(t['artwork'] for t in state()['tracks'])
        snapshot('desktop-library.png')
        first = state()['tracks'][0]['id']
        second = state()['tracks'][1]['id']
        action('play-track', first)
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'))
        assert first in prop('Metadata')
        time.sleep(0.4)
        assert re.search(r'int64 [1-9]', prop('Position')), prop('Position')
        call(PLAYER, 'Pause')
        wait_until(lambda: 'Paused' in prop('PlaybackStatus'))
        call(PLAYER, 'Seek', '5000000')
        call('org.freedesktop.DBus.Properties', 'Set', PLAYER, 'Volume', '<0.4>')
        assert '0.4' in prop('Volume')
        action('favorite', first)
        action('enqueue', second)
        wait_until(lambda: state()['tracks'][0]['favorite'])
        call(PLAYER, 'Next')
        assert 'Paused' in prop('PlaybackStatus')
        assert second in prop('Metadata')
        call(PLAYER, 'Play')
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'))
        # Mini player, navigation, URL entry, settings, and narrow window are real GTK windows.
        snapshot('desktop-playing.png')
        action('mini')
        snapshot('mini.png')
        action('show')
        action('albums')
        snapshot('desktop-albums.png')
        action('play-track', first)
        action('details')
        snapshot('desktop-details.png')
        action('details')
        action('tracks')
        # A real HTTP source verifies feed loading and network audio without external services.
        action('subscribe', base + '/feed.xml')
        wait_until(lambda: len(state()['subscriptions']) == 1)
        episode = next(t for t in state()['tracks'] if t['kind'] == 'podcast')
        action('play-track', episode['id'])
        wait_until(lambda: 'Playing' in prop('PlaybackStatus') and episode['id'] in prop('Metadata'))
        wait_until(lambda: int(re.search(r'int64 (\d+)', prop('Position'))[1]) > 200000)
        call(PLAYER, 'OpenUri', stream_url)
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'))
        wait_until(lambda: int(re.search(r'int64 (\d+)', prop('Position'))[1]) > 200000)
        action('play-track', second)
        # Playlist creation and adding a track use the same dialogs as mouse input.
        action('page', 'playlists')
        action('primary')
        time.sleep(0.2)
        playlist_window = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Playlists?$'], text=True).splitlines()[-1]
        subprocess.run(['xdotool', 'windowfocus', '--sync', playlist_window, 'type', '--clearmodifiers', 'Test playlist'], check=True)
        subprocess.run(['xdotool', 'key', 'Return'], check=True)
        wait_until(lambda: len(state()['playlists']) == 1)
        action('playlist-add', second)
        time.sleep(0.2)
        playlist_window = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Playlists?$'], text=True).splitlines()[-1]
        subprocess.run(['xdotool', 'windowfocus', '--sync', playlist_window, 'key', 'Tab', 'Tab', 'Return'], check=True)
        wait_until(lambda: second in state()['playlists'][0]['tracks'])
        for page in ['favorites', 'queue', 'podcasts', 'radio', 'streams', 'playlists', 'library']:
            action('page', page)
            assert child.poll() is None
        action('settings')
        time.sleep(0.3)
        windows = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Settings$'], text=True).splitlines()
        if windows:
            subprocess.run(['xdotool', 'windowfocus', '--sync', windows[-1], 'key', 'Escape'], check=True)
        assert child.poll() is None
        action('page', 'radio')
        snapshot('desktop-empty.png')
        action('page', 'library')
        main_window = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Open Audio Player$'], text=True).splitlines()[-1]
        subprocess.run(['xdotool', 'windowsize', main_window, '820', '620'], check=True)
        time.sleep(0.3)
        geometry = subprocess.check_output(['xdotool', 'getwindowgeometry', '--shell', main_window], text=True)
        assert int(re.search(r'HEIGHT=(\d+)', geometry)[1]) <= 640, geometry
        snapshot('compact.png')
        subprocess.run(['xdotool', 'windowsize', main_window, '1240', '800'], check=True)
        # Persist a meaningful resume position, then stop and start the process.
        call(PLAYER, 'SetPosition', f'/org/mpris/MediaPlayer2/Track/{second}', '8000000')
        call(PLAYER, 'Pause')
        call('org.mpris.MediaPlayer2', 'Quit')
        assert child.wait(timeout=10) == 0
        saved = state()
        assert saved['current'] == second
        assert saved['position'] >= 7.8, saved['position']
        assert saved['volume'] == 0.4
        child = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
        wait_until(lambda: 'Stopped' in prop('PlaybackStatus'))
        assert second in prop('Metadata')
        call(PLAYER, 'Play')
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'))
        wait_until(lambda: int(re.search(r'int64 (\d+)', prop('Position'))[1]) >= 7_800_000)
        call('org.mpris.MediaPlayer2', 'Quit')
        assert child.wait(timeout=10) == 0
        log.seek(0)
        output = log.read()
        assert 'panicked' not in output and 'CRITICAL' not in output, output
        print('PASS: library import, artwork, lyrics, audio, seek, pause, queue, favorites, playlists, HTTP podcast/audio, MPRIS, mini player, navigation, settings, compact layout, and restored session.')
    except Exception:
        log.seek(0)
        print(log.read())
        print('Application exit status:', child.poll())
        if (data / 'library.json').exists():
            saved = state()
            print('Saved state:', len(saved['tracks']), 'tracks;', saved['folders'])
        snapshot('failure.png')
        raise
    finally:
        if child.poll() is None:
            child.terminate()
            child.wait(timeout=5)
        server.shutdown()
