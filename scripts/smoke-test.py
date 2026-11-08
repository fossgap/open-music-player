#!/usr/bin/env python3
"""Exercise the real GTK app, audio pipeline, MPRIS and persistence offline.

Run inside xvfb-run and dbus-run-session. All media and state live in a temporary
directory. Optional OPEN_MUSIC_SCREENSHOTS captures review artifacts;
OPEN_MUSIC_BINARY selects a release binary or extracted AppImage launcher.
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
from urllib.parse import quote, urlparse, unquote, parse_qs

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('OPEN_MUSIC_BINARY', ROOT / 'target/debug/open-music-player')).resolve()
DEST = 'org.mpris.MediaPlayer2.OpenMusicPlayer'
PLAYER = 'org.mpris.MediaPlayer2.Player'

for dependency in ['ffmpeg', 'gdbus', 'xdotool'] + (['import'] if os.environ.get('OPEN_MUSIC_SCREENSHOTS') else []):
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
    return call('org.gtk.Actions', 'Activate', name, params, '{}', dest='io.github.openmusic.Player', path='/io/github/openmusic/Player')


def window(title):
    return subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^' + re.escape(title) + '$'], text=True).splitlines()[-1]


def window_size(target):
    geometry = subprocess.check_output(['xdotool', 'getwindowgeometry', '--shell', target], text=True)
    return tuple(int(re.search(r'^' + axis + r'=(\d+)', geometry, re.M)[1]) for axis in ('WIDTH', 'HEIGHT'))


def visible_player_windows():
    return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^Open Music'],
                          capture_output=True, text=True).stdout.splitlines()


def close_player_window(title):
    target = window(title)
    if os.environ.get('OPEN_MUSIC_TEST_WM'):
        # Use the real window manager's close request across GTK decoration styles.
        subprocess.run(['xdotool', 'windowactivate', '--sync', target], check=True)
        keys('alt+F4')
        return
    # Click GTK's titlebar Close button on the baseline display without a WM.
    geometry = subprocess.check_output(['xdotool', 'getwindowgeometry', '--shell', target], text=True)
    width = int(re.search(r'^WIDTH=(\d+)', geometry, re.M)[1])
    subprocess.run(['xdotool', 'mousemove', '--window', target, str(width - 22), '23', 'click', '1'], check=True)


def keys(*sequence):
    subprocess.run(['xdotool', 'key', '--clearmodifiers', *sequence], check=True)


def type_text(value):
    subprocess.run(['xdotool', 'type', '--clearmodifiers', value], check=True)


def playlist_name(title, value):
    wait_until(lambda: window(title))
    subprocess.run(['xdotool', 'windowfocus', '--sync', window(title)], check=True)
    keys('ctrl+a')
    type_text(value)
    keys('Return')


def show_id(feed):
    # Directory cards expose the same stable feed identifier as the data model.
    value = 0xcbf29ce484222325
    for byte in feed.encode():
        value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return f't{value:016x}'


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


with tempfile.TemporaryDirectory(prefix='open-music-test-') as temporary:
    temp = Path(temporary)
    spec = importlib.util.spec_from_file_location('demo', ROOT / 'scripts/create-demo.py')
    demo = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(demo)
    music = demo.create(temp / 'music', 45)
    directory_requests = []
    class QuietHandler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass
        def do_GET(self):
            if self.path.startswith(('/apple/', '/gpodder/')):
                directory_requests.append(self.path)
                query = parse_qs(urlparse(self.path).query)
                term = query.get('term', query.get('q', ['']))[0]
                if term == 'failure':
                    self.send_error(503, 'Test directory unavailable')
                    return
                shows = [
                    ('Listening notes', 'Open Music Studio', 'Daylight studies'),
                    ('After the recording', 'The demo collection', 'After hours'),
                    ('Small discoveries', 'Open Music Studio', 'Soft signals'),
                ]
                results = []
                for index, (title, author, album) in enumerate(shows):
                    feed = f'{base}/feed-{index}.xml'
                    art = f'{base}/{quote(album)}/cover.png'
                    if self.path.startswith('/apple/'):
                        results.append(dict(collectionName=title, artistName=author, feedUrl=feed,
                                            artworkUrl600=art, collectionViewUrl=base))
                    else:
                        results.append(dict(title=title, author=author, url=feed, logo_url=art, website=base))
                if term == 'no-results':
                    results = []
                payload = {'results': results} if self.path.startswith('/apple/') else results
                encoded = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)
                return
            super().do_GET()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(QuietHandler, directory=str(music)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    media = next(music.rglob('*.flac')).relative_to(music)
    stream_url = base + '/' + quote(str(media))
    (music / 'feed.xml').write_text(f'<rss><channel><title>Offline test podcast</title><item><title>Test episode</title><guid>offline-episode</guid><enclosure url="{stream_url}" type="audio/flac"/></item></channel></rss>')
    for index in range(3):
        (music / f'feed-{index}.xml').write_text(f'<rss><channel><title>Directory show {index}</title><item><title>Directory episode {index}</title><guid>directory-{index}</guid><enclosure url="{stream_url}?episode={index}" type="audio/flac"/></item></channel></rss>')
    data = temp / 'state'
    data.mkdir()
    (data / 'library.json').write_text(json.dumps({'tracks': [], 'folders': [], 'fetch_covers': False, 'fetch_lyrics': False, 'fetch_information': False, 'download_directory': str(temp / 'downloads')}))
    runtime = temp / 'runtime'
    runtime.mkdir(mode=0o700)
    env = {**os.environ, 'OPEN_MUSIC_DATA_DIR': str(data), 'OPEN_MUSIC_FAKE_SINK': '1',
           'GTK_A11Y': 'none', 'GTK_USE_PORTAL': '0', 'GIO_USE_VFS': 'local', 'GDK_BACKEND': 'x11',
           'GSK_RENDERER': 'cairo', 'XDG_RUNTIME_DIR': str(runtime), 'LANGUAGE': 'en',
           'OPEN_MUSIC_ITUNES_API': base + '/apple/', 'OPEN_MUSIC_GPODDER_API': base + '/gpodder/'}
    env.pop('WAYLAND_DISPLAY', None)
    log = (temp / 'app.log').open('w+')
    child = subprocess.Popen([str(BINARY), '--import', str(music)], env=env, stdout=log, stderr=log)
    def state():
        return json.loads((data / 'library.json').read_text())
    def snapshot(name):
        if directory := os.environ.get('OPEN_MUSIC_SCREENSHOTS'):
            Path(directory).mkdir(parents=True, exist_ok=True)
            time.sleep(0.3)
            windows = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Open Music'], text=True).splitlines()
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
        call('org.freedesktop.DBus.Properties', 'Set', PLAYER, 'Rate', '<1.5>')
        assert '1.5' in prop('Rate')
        assert '0.5' in prop('MinimumRate') and '2.0' in prop('MaximumRate')
        assert 'Paused' in prop('PlaybackStatus'), 'Changing speed must preserve pause'
        action('toggle-time')
        wait_until(lambda: state()['playback_rate'] == 1.5 and state()['show_remaining'])
        call('org.freedesktop.DBus.Properties', 'Set', PLAYER, 'Rate', '<1.0>')
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
        action('open-album', first)
        snapshot('desktop-album-tracks.png')
        action('back')
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
        wait_until(lambda: 'Playing' in prop('PlaybackStatus') and second in prop('Metadata'))
        call(PLAYER, 'Pause')
        action('download', episode['id'])
        wait_until(lambda: len(state()['saved_audio']) == 1)
        saved_audio = Path(state()['saved_audio'][0]['path'])
        assert saved_audio.read_bytes() == (music / media).read_bytes()
        assert next(t for t in state()['tracks'] if t['id'] == episode['id'])['download_path'] == str(saved_audio)
        snapshot('desktop-downloads.png')
        # Playlist creation and adding a track use the same dialogs as mouse input.
        action('page', 'playlists')
        action('primary')
        playlist_name('New playlist', 'Evening listening')
        wait_until(lambda: len(state()['playlists']) == 1)
        playlist_id = state()['playlists'][0]['id']
        action('playlist-add', second)
        playlist_name('Add to playlist', '')
        wait_until(lambda: second in state()['playlists'][0]['tracks'])
        action('rename-playlist', playlist_id)
        playlist_name('Rename playlist', 'After hours')
        wait_until(lambda: state()['playlists'][0]['name'] == 'After hours')
        assert state()['playlists'][0]['id'] == playlist_id
        assert state()['playlists'][0]['tracks'] == [second]
        # Relative local files, streams, duplicates and missing entries in one imported playlist.
        local_tracks = [t for t in state()['tracks'] if t['kind'] == 'music']
        imported_ids = [local_tracks[i]['id'] for i in [4, 8, 0]]
        paths = [str(Path(unquote(urlparse(local_tracks[i]['uri']).path)).relative_to(music)) for i in [4, 8, 0]]
        imported_file = music / 'Weekend.m3u8'
        imported_file.write_text('#EXTM3U\n#PLAYLIST:Weekend selections\n' + '\n'.join(paths) + '\n' + stream_url + '\n' + paths[0] + '\nmissing.flac\n')
        action('import-playlist', str(imported_file))
        wait_until(lambda: len(state()['playlists']) == 2)
        imported = state()['playlists'][1]
        assert imported['name'] == 'Weekend selections'
        assert imported['tracks'][:3] == imported_ids
        assert len(imported['tracks']) == 4
        assert next(t for t in state()['tracks'] if t['id'] == first)['favorite']
        assert second in prop('Metadata'), 'Import must not change playback'
        snapshot('desktop-playlist-tracks.png')
        action('new-playlist')
        playlist_name('New playlist', 'A new collection')
        wait_until(lambda: len(state()['playlists']) == 3)
        snapshot('desktop-playlist-empty.png')
        action('back')
        snapshot('desktop-playlists.png')
        # Browser discovery, explicit search, empty/error results, provider switching and subscription.
        action('page', 'podcasts')
        action('podcast-search')
        wait_until(lambda: any('/apple/search?' in url for url in directory_requests))
        time.sleep(0.7)
        snapshot('desktop-podcasts.png')
        action('subscribe-show', show_id(base + '/feed-0.xml'))
        wait_until(lambda: len(state()['subscriptions']) == 2)
        action('subscribe-show', show_id(base + '/feed-0.xml'))
        wait_until(lambda: len(state()['subscriptions']) == 1)
        assert any(t['title'] == 'Directory episode 0' for t in state()['tracks'])
        action('subscribe-show', show_id(base + '/feed-0.xml'))
        wait_until(lambda: len(state()['subscriptions']) == 2)
        def search_podcasts(term):
            subprocess.run(['xdotool', 'windowfocus', '--sync', window('Open Music Player')], check=True)
            keys('ctrl+f', 'ctrl+a')
            type_text(term)
            keys('Return')
            wait_until(lambda: any(quote(term) in url for url in directory_requests))
            time.sleep(0.4)
        search_podcasts('no-results')
        snapshot('desktop-podcasts-empty.png')
        search_podcasts('failure')
        snapshot('desktop-podcasts-error.png')
        search_podcasts('listening')
        action('subscribe-show', show_id(base + '/feed-1.xml'))
        wait_until(lambda: len(state()['subscriptions']) == 3)
        # With an empty search, Tab reaches the native directory selector.
        keys('ctrl+f', 'ctrl+a', 'BackSpace', 'Tab', 'space', 'Home', 'Down', 'Return')
        wait_until(lambda: any('/gpodder/toplist/50.json' in url for url in directory_requests))
        time.sleep(0.5)
        action('subscribe-show', show_id(base + '/feed-2.xml'))
        wait_until(lambda: len(state()['subscriptions']) == 4)
        search_podcasts('listening')
        wait_until(lambda: any('/gpodder/search.json?' in url for url in directory_requests))
        snapshot('desktop-podcasts-gpodder.png')

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
        main_window = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--name', '^Open Music Player$'], text=True).splitlines()[-1]
        subprocess.run(['xdotool', 'windowsize', main_window, '820', '620'], check=True)
        time.sleep(0.3)
        geometry = subprocess.check_output(['xdotool', 'getwindowgeometry', '--shell', main_window], text=True)
        assert int(re.search(r'HEIGHT=(\d+)', geometry)[1]) <= 640, geometry
        snapshot('compact.png')
        action('page', 'playlists')
        snapshot('compact-playlists.png')
        action('open-playlist', imported['id'])
        snapshot('compact-playlist-tracks.png')
        action('page', 'podcasts')
        snapshot('compact-podcasts.png')
        action('page', 'library')
        subprocess.run(['xdotool', 'windowsize', main_window, '1140', '740'], check=True)
        wait_until(lambda: window_size(main_window) == (1140, 740))
        time.sleep(0.3)
        saved_size = window_size(main_window)
        # Persist a meaningful resume position, then stop and start the process.
        call(PLAYER, 'SetPosition', f'/org/mpris/MediaPlayer2/Track/{second}', '8000000')
        call(PLAYER, 'Pause')
        call('org.mpris.MediaPlayer2', 'Quit')
        assert child.wait(timeout=10) == 0
        saved = state()
        assert saved['current'] == second
        assert saved['position'] >= 7.8, saved['position']
        assert saved['volume'] == 0.4
        assert saved['show_remaining'] and saved['playback_rate'] == 1.0
        assert saved['window']['width'] > 0 and saved['window']['height'] > 0
        assert not saved['window']['maximized']
        saved_window = saved['window']
        assert saved['saved_audio'][0]['path'] == str(saved_audio)
        assert len(saved['playlists']) == 3 and len(saved['subscriptions']) == 4
        child = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
        wait_until(lambda: 'Stopped' in prop('PlaybackStatus'))
        wait_until(lambda: window_size(window('Open Music Player')) == saved_size)
        assert second in prop('Metadata')
        call(PLAYER, 'Play')
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'))
        wait_until(lambda: int(re.search(r'int64 (\d+)', prop('Position'))[1]) >= 7_800_000)
        call('org.mpris.MediaPlayer2', 'Quit')
        assert child.wait(timeout=10) == 0
        # Startup and close preferences remain independent, including without a tray.
        saved = state()
        saved.update(start_in_background=True, background=True, theme='sage')
        (data / 'library.json').write_text(json.dumps(saved))
        child = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
        wait_until(lambda: 'Stopped' in prop('PlaybackStatus'))
        assert child.poll() is None and not visible_player_windows()
        assert state()['window'] == saved_window
        subprocess.run([str(BINARY)], env=env, stdout=log, stderr=log, check=True, timeout=10)
        wait_until(lambda: window('Open Music Player'))
        assert window_size(window('Open Music Player')) == saved_size
        close_player_window('Open Music Player')
        wait_until(lambda: not visible_player_windows())
        assert child.poll() is None
        call('org.mpris.MediaPlayer2', 'Raise')
        wait_until(lambda: window('Open Music Player'))
        call(PLAYER, 'Play')
        wait_until(lambda: 'Playing' in prop('PlaybackStatus'))
        call(PLAYER, 'Pause')
        close_player_window('Open Music Player')
        wait_until(lambda: not visible_player_windows())
        assert child.poll() is None
        call('org.mpris.MediaPlayer2', 'Quit')
        assert child.wait(timeout=10) == 0
        assert state()['theme'] == 'sage' and state()['start_in_background']
        for hidden in (True, False):
            saved = state()
            saved.update(start_in_background=hidden, background=False, theme='system')
            (data / 'library.json').write_text(json.dumps(saved))
            child = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
            wait_until(lambda: 'Stopped' in prop('PlaybackStatus'))
            if hidden:
                assert not visible_player_windows()
                call('org.mpris.MediaPlayer2', 'Raise')
            wait_until(lambda: window('Open Music Player'))
            close_player_window('Open Music Player')
            assert child.wait(timeout=10) == 0
        log.seek(0)
        output = log.read()
        assert 'panicked' not in output and 'CRITICAL' not in output, output
        print('PASS: library/audio/MPRIS, albums, playlist create/add/rename/import/grid, Apple and gPodder discovery/search/subscription toggles, empty/error states, mini player, compact layouts, session/window-size restoration, theme persistence, and independent startup/close preferences.')
    except Exception:
        log.seek(0)
        print(log.read())
        print('Application exit status:', child.poll())
        if (data / 'library.json').exists():
            saved = state()
            print('Saved state:', len(saved['tracks']), 'tracks;', saved['folders'])
        if child.poll() is None:
            snapshot('failure.png')
        raise
    finally:
        if child.poll() is None:
            child.terminate()
            child.wait(timeout=5)
        server.shutdown()
