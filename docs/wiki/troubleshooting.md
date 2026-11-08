# Troubleshooting

[User guide](README.md) · [Installation](installation.md) · [Data privacy](data-privacy.md)

## No audio or an unsupported file

Check the desktop output device and application volume/mute state. Native
packages use installed GStreamer codecs; additional formats may require the
distribution's libav, bad, or ugly plugin packages. FFmpeg/FFprobe must be on
`PATH` for scanning and saving audio. Spotify speed control uses the optional
SoundTouch `pitch` plugin. Live or unseekable streams may not support seeking or
speed changes.

Try a local WAV or FLAC file to separate a codec/output problem from a provider
problem. Start the app in a terminal to capture errors. With an extracted
AppImage, launch `squashfs-root/AppRun`, which sets its runtime paths.

## Library entries or tags are unexpected

Use Refresh to rescan your folders. Unavailable folders keep their indexed
tracks; removing a folder from Settings stops watching without deleting its
entries. **Remove from library** excludes a file from later scans until you
explicitly import it or its folder again. **Reload file tags** removes local
overrides but cannot undo tags already written to the audio file.

Back up the data directory with the app closed before troubleshooting a damaged
library. The app reports corrupt or unsupported state without replacing it.
For an isolated test, run:

```sh
OPEN_MUSIC_DATA_DIR=/tmp/open-music-dev open-music-player
```

## Artwork, lyrics, or biography missing

Check the three online lookup switches in Settings. Matching needs sufficient
artist/album/title tags or MusicBrainz identifiers. Ambiguous matches are rejected;
completely untagged files are not identified by acoustic fingerprinting. Use the
detail panel's retry/refresh controls for missing or partial results. Provider
outages, rate limits, and unavailable covers or lyrics do not stop playback.

## Online accounts or playlist sync fail

Follow the [streaming guide](streaming.md), including Python/JavaScript runtime
requirements for YouTube and a running, unlocked Secret Service keyring. Re-run
the YouTube setup helper to update its optional dependencies; close the browser
before importing a locked cookie database. A connected session can expire.

Spotify playback requires Premium. Catalog permissions and developer-client
restrictions can still prevent searches or playlist access. Reconnecting or
using your own approved Client ID does not bypass provider restrictions. The
offline tests do not certify live account playback or remote playlist writes.

After an interrupted upload, inspect the online playlist before retrying. To
accept the remote version intentionally, use **Replace local copy from Spotify**
or **Replace local copy from YouTube Music**. Disconnecting removes that service's
local tracks and linked playlists; reconnect and sync to fetch them again.

## Window closes but the app is still running

This is the default background behavior. Use **Ctrl + Q** or **Quit**, or disable
**Keep running when the window closes**. Reopen the app from the launcher to show
it again. GNOME needs a StatusNotifier/AppIndicator extension for its tray icon;
MPRIS controls and background playback work independently of that icon.

## Report a problem

Open a [GitHub issue](https://github.com/fossgap/open-music-player/issues) with
the app version or commit, distribution/version, package format, desktop/session
(Wayland or X11), reproduction steps, and the relevant error text. Include a
screenshot for a visible problem. Redact private file paths, account names,
feed/stream URL tokens, cookies, and playlist contents. Do not attach credentials
or your entire unredacted library.
