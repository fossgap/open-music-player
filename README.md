<p align="center">
  <img src="resources/io.github.openmusic.Player.svg" width="128" height="128" alt="Open Music Player icon">
</p>

<h1 align="center">Open Music Player</h1>

[![Test CI](https://github.com/fossgap/open-music-player/actions/workflows/test-ci.yml/badge.svg)](https://github.com/fossgap/open-music-player/actions/workflows/test-ci.yml)
[![Release CI](https://github.com/fossgap/open-music-player/actions/workflows/release-ci.yml/badge.svg)](https://github.com/fossgap/open-music-player/actions/workflows/release-ci.yml)
[![GitHub release](https://img.shields.io/github/v/release/fossgap/open-music-player)](https://github.com/fossgap/open-music-player/releases)
[![Downloads](https://img.shields.io/github/downloads/fossgap/open-music-player/total)](https://github.com/fossgap/open-music-player/releases)
[![License: GPL-3.0-only](https://img.shields.io/badge/License-GPL--3.0--only-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024-orange.svg)](Cargo.toml)
[![GTK 4.8+](https://img.shields.io/badge/GTK-4.8%2B-blue.svg)](Cargo.toml)
[![Platform: Linux](https://img.shields.io/badge/Platform-Linux-lightgrey.svg)](docs/packaging.md)

> A native GPU accelerated Linux home for your music, podcasts, and radio. Built with Rust, GTK 4, and GStreamer, part of [FOSS Gap](https://github.com/fossgap).

[Features](#features) · [User guide](docs/wiki/README.md) · [Installation](#installation) · [Data privacy](docs/wiki/data-privacy.md) · [Building from source](#building-from-source)

![Open Music Player with generated demonstration music](docs/player.png)


## Features

- Local music folders with recursive watching, drag and drop, search, album-artist grouping, and duplicate detection
- Favorites, playlists, queue, multi-selection, drag/keyboard ordering, and M3U/M3U8/PLS/XSPF playlist import
- Single/bulk tag editing, with library-only overrides or reviewed writes to audio files
- Gapless playback, optional crossfade and ReplayGain, shuffle/repeat, seeking, and 0.5–2× playback speed
- Podcast discovery through Apple Podcasts and gPodder, RSS/Atom subscriptions, and per-episode resume
- Internet radio through Radio Browser, direct HTTP(S) streams, downloads, and recordings
- YouTube Music search/playback and optional account connection; Spotify playback with Premium
- Two-way playlist synchronization and private playlist publishing for connected music services
- Embedded, local, and online artwork/lyrics; synchronized lyrics, artist biographies, album information, and discographies
- Waveform, spectrum, audio-reactive scenes, and album-art visualization
- MPRIS desktop controls, system tray, mini player, background playback, and remembered sessions
- Ten interface languages: English, French, Spanish, German, Italian, Portuguese, Russian, Chinese, Japanese, and Korean

YouTube Music uses community ytmusicapi/yt-dlp libraries; Spotify uses librespot and its Web API. Service availability, account restrictions, and provider changes apply. See [service requirements](docs/wiki/services.md) and [streaming setup](docs/wiki/streaming.md).

## Installation

Release builds target **Linux x86_64**: Debian/Ubuntu/Kali (`.deb`), Fedora (`.rpm`), Arch (`.pkg.tar.zst`), and AppImage. See [GitHub Releases](https://github.com/fossgap/open-music-player/releases) for published artifacts. If no release is available yet, [build from source](#building-from-source).

[Verify the download](docs/wiki/installation.md#verify-a-download) with the release's `SHA256SUMS`, then run the command for your system in the download directory:

```sh
# Debian / Ubuntu / Kali
sudo apt install ./open-music-player_*_amd64.deb

# Fedora
sudo dnf install ./open-music-player-[0-9]*.x86_64.rpm

# Arch Linux
sudo pacman -U ./open-music-player-[0-9]*-x86_64.pkg.tar.zst

# AppImage — use the filename you downloaded
chmod +x OpenMusicPlayer-0.1.1-x86_64.AppImage
./OpenMusicPlayer-0.1.1-x86_64.AppImage
```

The build baseline is Debian 12 (glibc 2.36, GTK 4.8). Native packages declare their runtime dependencies; the AppImage bundles GTK, GStreamer, and FFmpeg. Optional YouTube support needs Python, a JavaScript runtime, and the [setup helper](docs/wiki/streaming.md#youtube-music). Account connections need an unlocked desktop keyring. See [installation and updates](docs/wiki/installation.md) for details and running AppImage without FUSE.

## Usage

1. Select **Add music** and choose a folder, or drop audio files/folders onto the window.
2. Double-click a track to play it. Use its menu to queue it, add it to a playlist, edit tags, or save a favorite.
3. Open **Podcasts**, **Live radio**, or **Streaming** to explore online audio.
4. Click the current cover for artwork, lyrics, visualizations, and **Artist & album** information.
5. Use **Settings** for playback, music/download folders, language, theme, and privacy controls.

**Ctrl + K** opens global search; **Ctrl + Space** toggles playback; **Ctrl + M** opens the mini player. Closing a window keeps the app running by default; **Ctrl + Q** quits. The [user guide](docs/wiki/README.md) covers playlists, downloads, account sync, and all shortcuts.

## Data privacy

There is no application analytics or crash upload. **Automatic cover, lyrics, and artist/album lookups are on by default** and can send music metadata to MusicBrainz, Cover Art Archive, LRCLIB, Wikidata, and Wikipedia, even for local files. Online searches, feeds, images, and playback contact their respective providers. A saved Spotify account reconnects at startup; enabled automatic refresh can upload playlist edits.

Read the [data privacy wiki](docs/wiki/data-privacy.md) for each recipient, the information sent, request triggers, local storage, and steps to limit network access. Settings has independent controls for the three metadata lookups; automatic source refresh is off by default.

## Building from source

Requires Rust 1.89+, GTK 4.8+, GStreamer, FFmpeg, and the native development libraries. On Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev libgtk-4-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav gstreamer1.0-pulseaudio ffmpeg

git clone https://github.com/fossgap/open-music-player.git
cd open-music-player
cargo run --locked
```

With rustfmt and Clippy installed:

```sh
make check                 # Formatting, strict Clippy, translations
make test                  # Rust and Python bridge tests
make ui-test               # GTK regressions under Xvfb
make smoke                 # Offline application/playback integration
make build                 # Optimized binary
make deb                   # Also rpm, arch, appimage; see packaging guide
```

The [Docker environment](Dockerfile) supplies development and test dependencies:

```sh
bash scripts/ci.sh check
```

See [development](docs/development.md), [packaging](docs/packaging.md), [CI and releases](docs/ci-release.md), and the [repository guidelines](AGENTS.md). Report problems through [GitHub Issues](https://github.com/fossgap/open-music-player/issues) with your distribution, version, and reproduction steps; remove private data from logs.

## Powered by

- [GTK](https://www.gtk.org/) and [gtk-rs](https://gtk-rs.org/) — native desktop interface
- [GStreamer](https://gstreamer.freedesktop.org/) and [FFmpeg](https://ffmpeg.org/) — playback, metadata, downloads, and recording
- [Lofty](https://github.com/Serial-ATA/lofty-rs) — audio tags
- [librespot](https://github.com/librespot-org/librespot) — Spotify playback
- [ytmusicapi](https://github.com/sigma67/ytmusicapi) and [yt-dlp](https://github.com/yt-dlp/yt-dlp) — optional YouTube Music support
- [MusicBrainz](https://musicbrainz.org/), [Cover Art Archive](https://coverartarchive.org/), [LRCLIB](https://lrclib.net/), [Wikidata](https://www.wikidata.org/), and [Wikipedia](https://en.wikipedia.org/) — music information, artwork, and lyrics
- [Radio Browser](https://www.radio-browser.info/), [Apple Podcasts](https://podcasts.apple.com/), and [gPodder](https://gpodder.net/) — audio discovery

## License

Open Music Player is licensed under [GPL-3.0-only](LICENSE). Dependencies, runtime components, and retrieved media retain their respective licenses. Wikipedia summaries include their source and CC BY-SA attribution in the application.
