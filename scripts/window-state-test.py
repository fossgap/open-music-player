#!/usr/bin/env python3
"""Test window-size and maximize restoration with a real window manager.

Requires xfwm4. Run after cargo build, in an isolated display and D-Bus session:
    xvfb-run -a dbus-run-session -- python3 scripts/window-state-test.py
All application and window-manager preferences live in a temporary directory.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/open-music-player'
for dependency in ['xfwm4', 'xdotool', 'xprop', 'gdbus']:
    if not shutil.which(dependency):
        raise SystemExit(f'Missing window-test dependency: {dependency}')


def run(*args, env=None):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE, env=env, timeout=5)


def wait_until(predicate):
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (subprocess.SubprocessError, IndexError):
            pass
        time.sleep(0.1)
    raise AssertionError('Timed out waiting for window state')


def visible(title='Open Music Player'):
    result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^' + re.escape(title) + '$'], capture_output=True, text=True, timeout=5)
    return result.stdout.splitlines()


def size():
    geometry = run('xdotool', 'getwindowgeometry', '--shell', visible()[-1])
    return tuple(int(re.search(r'^' + key + r'=(\d+)', geometry, re.M)[1]) for key in ['WIDTH', 'HEIGHT'])


def maximized():
    state = run('xprop', '-id', visible()[-1], '_NET_WM_STATE')
    return all('_NET_WM_STATE_MAXIMIZED_' + axis in state for axis in ['HORZ', 'VERT'])


def toggle_maximized():
    target = visible()[-1]
    run('xdotool', 'windowfocus', '--sync', target)
    run('xdotool', 'mousemove', '--window', target, str(size()[0] // 2), '22', 'click', '--repeat', '2', '--delay', '90', '1')


def mpris(method):
    return run('gdbus', 'call', '--session', '--dest', 'org.mpris.MediaPlayer2.OpenMusicPlayer', '--object-path', '/org/mpris/MediaPlayer2', '--method', 'org.mpris.MediaPlayer2.' + method)


with tempfile.TemporaryDirectory(prefix='open-music-window-') as directory:
    root = Path(directory)
    runtime = root / 'runtime'
    runtime.mkdir(mode=0o700)
    env = {**os.environ, 'OPEN_MUSIC_DATA_DIR': str(root), 'OPEN_MUSIC_FAKE_SINK': '1',
           'XDG_RUNTIME_DIR': str(runtime), 'XDG_CONFIG_HOME': str(root / 'config'),
           'XDG_DATA_HOME': str(root / 'data'), 'XDG_CACHE_HOME': str(root / 'cache'),
           'GDK_BACKEND': 'x11', 'GSK_RENDERER': 'cairo', 'GTK_A11Y': 'none',
           'GTK_USE_PORTAL': '0', 'GIO_USE_VFS': 'local', 'LANGUAGE': 'en'}
    env.pop('WAYLAND_DISPLAY', None)
    path = root / 'library.json'
    path.write_text(json.dumps({'fetch_covers': False, 'fetch_lyrics': False, 'fetch_information': False}))
    child = None
    with (root / 'test.log').open('w+') as log:
        manager = subprocess.Popen(['xfwm4', '--compositor=off', '--sm-client-disable'], env=env, stdout=log, stderr=log)

        def launch(hidden=False):
            global child
            child = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
            wait_until(lambda: mpris('Raise') if not hidden else run(
                'gdbus', 'call', '--session', '--dest', 'org.mpris.MediaPlayer2.OpenMusicPlayer',
                '--object-path', '/org/mpris/MediaPlayer2', '--method', 'org.freedesktop.DBus.Properties.Get',
                'org.mpris.MediaPlayer2.Player', 'PlaybackStatus'))
            if not hidden:
                wait_until(visible)

        def quit_player():
            mpris('Quit')
            assert child.wait(timeout=10) == 0
            return json.loads(path.read_text())

        try:
            wait_until(lambda: 'window id #' in run('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'))
            launch()
            run('xdotool', 'windowsize', visible()[-1], '1140', '740')
            wait_until(lambda: size() == (1140, 740))
            time.sleep(0.3)
            normal = quit_player()['window']
            assert not normal['maximized']

            launch()
            wait_until(lambda: size() == (1140, 740))
            toggle_maximized()
            wait_until(maximized)
            expanded = quit_player()['window']
            assert expanded == {**normal, 'maximized': True}, (normal, expanded)

            launch()
            wait_until(maximized)
            toggle_maximized()
            wait_until(lambda: not maximized() and size() == (1140, 740))
            run('gdbus', 'call', '--session', '--dest', 'io.github.openmusic.Player',
                '--object-path', '/io/github/openmusic/Player', '--method', 'org.gtk.Actions.Activate', 'mini', '[]', '{}')
            wait_until(lambda: visible('Open Music · Mini') and not visible())
            assert quit_player()['window'] == normal

            saved = json.loads(path.read_text())
            saved.update(start_in_background=True, window=expanded)
            path.write_text(json.dumps(saved))
            launch(hidden=True)
            assert not visible()
            assert quit_player()['window'] == expanded, 'Hidden startup must preserve saved maximize/normal size'

            launch(hidden=True)
            assert not visible()
            mpris('Raise')
            wait_until(maximized)
            toggle_maximized()
            wait_until(lambda: not maximized() and size() == (1140, 740))
            assert quit_player()['window'] == normal
            print('PASS: resized window, maximize/restart/restore, mini-player quit and hidden startup retain the correct normal size.')
        except Exception:
            log.seek(0)
            print(log.read())
            raise
        finally:
            if child is not None and child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
            manager.terminate()
            manager.wait(timeout=5)
