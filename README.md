<div align="center">
  <img src="resources/io.github.openaudio.Player.svg" width="72" alt="Open Audio Player icon">
  <h1>Open Audio Player</h1>
  <p>A native Linux home for your music, podcasts, and radio.</p>
  <p><strong>Rust · GTK 4 · GStreamer</strong></p>
</div>

Open Audio keeps a local library, internet audio, and desktop playback controls in one quiet interface. Choose your music folders, put something on, and keep listening in the mini player or system tray.

![Open Audio Player with generated demonstration music](docs/player.png)

*The screenshot uses original, generated test tones and geometric artwork. No commercial music is bundled.*

[Get started](#get-started) · [Listening](#listening) · [Development](#development) · [Packaging](#packaging) · [Project layout](#project-layout)

## What works

| Area | Included |
| --- | --- |
| Local library | Recursive folder discovery, metadata, search, track and album views, manual refresh, duplicate detection |
| Collections | Favorites, named playlists, queue, shuffle, repeat one/all, removal from lists without deleting files |
| Playback | GStreamer audio, play/pause, previous/next, seeking, volume, mute, automatic next track |
| Podcasts | RSS and Atom subscriptions, audio enclosures, feed refresh, per-episode resume positions |
| Internet radio | Search and discovery through Radio Browser, station artwork, favorites, playback |
| Streaming | Direct HTTP(S) audio and supported streaming playlists; YouTube audio through optional `yt-dlp` |
| Desktop | MPRIS controls and metadata, StatusNotifierItem tray menu, background playback, mini player |
| Artwork and lyrics | Embedded covers, folder covers, feed/station images, softly blended artwork backdrop, embedded lyrics and `.lrc`/`.txt` sidecars |
| Visualization | Live waveform from decoded PCM audio; no synthetic playback animation |
| Persistence | Library, sources, playlists, queue, track, position, volume, repeat, shuffle, and preferences |
| Languages | English, French, Spanish, German, Italian, Portuguese, Russian, Chinese, Japanese, Korean |

**Service boundaries:** Spotify opens its official web player; native Spotify playback, account authentication, and DRM handling are not implemented. YouTube resolution requires a working `yt-dlp` installation and is subject to the availability of the video. Podcast downloads, synchronized lyric highlighting, and automatic filesystem watching are not included. This is an initial implementation, not a published stable release.

## Get started

Requires Linux, **Rust 1.85+**, **GTK 4.8+**, GStreamer with its base/good plugins, and FFmpeg. Audio format support depends on the installed GStreamer codecs. The development container supplies the native dependencies.

On Debian 12 / Ubuntu 24.04 or newer:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav gstreamer1.0-pulseaudio ffmpeg
```

With a current Rust toolchain installed:

```sh
cargo run --locked
```

Or build an optimized binary:

```sh
cargo build --release --locked
./target/release/open-audio-player
```

To add a folder at launch:

```sh
cargo run --locked -- --import "$HOME/Music"
```

For YouTube audio, install `yt-dlp` separately and keep it up to date. The application invokes it directly with an argument list, never through a shell. It does not download or store YouTube audio files.

## Listening

1. Select **Add music** and choose a folder. Scanning runs in the background; additional folders are managed in **Settings**.
2. Double-click a track, select an album, or use **Play all**. Search matches titles, artists, and albums.
3. Use the star to save a favorite. The track menu adds to the queue or a playlist. Removing a local track only removes its index entry; refreshing its folder can discover it again.
4. Select **Podcasts → Add podcast** and paste an RSS or Atom feed URL. Use **Refresh** to fetch new episodes. Removing a subscription stops refreshes and retains previously indexed episodes.
5. Select **Live radio → Discover radio**, or type a station name and press Enter. Radio results are provided by the community-maintained [Radio Browser directory](https://www.radio-browser.info/).
6. Select **Streaming → Add stream** and paste an HTTP(S) audio address or YouTube video URL. A station URL can also be opened here.

Click the current cover to open the artwork, waveform, and lyrics panel. Put `cover.jpg`, `cover.png`, `folder.jpg`, or `folder.png` beside an album, or use embedded artwork. A lyrics sidecar must share the audio filename: `Song.flac` and `Song.lrc`. Refresh after adding a sidecar. LRC text is displayed as supplied.

The session is restored without automatically starting audio. Press Play to continue. Podcast episodes also retain individual positions.

Closing the main window keeps audio playing **only while playback is active and a compatible system tray is available**. Otherwise the application exits cleanly. On GNOME, a StatusNotifier/AppIndicator extension is needed for tray integration; MPRIS and the mini player work independently of the tray. Choose **Quit** to stop playback and exit explicitly.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl + Space | Play / pause |
| Alt + Left / Right | Previous / next |
| Ctrl + O | Add a music folder |
| Ctrl + F | Focus search |
| Ctrl + R | Refresh the current source |
| Ctrl + L | Artwork and lyrics |
| Ctrl + M | Mini player |
| Ctrl + , | Settings |
| Ctrl + Q | Quit |
| Escape | Close a settings or playlist dialog |

### Your data

The library is stored in `$XDG_DATA_HOME/open-audio-player/library.json`, defaulting to `~/.local/share/open-audio-player/library.json`. Cached artwork lives beside it. Saves use a temporary file, synchronization, and atomic replacement. A corrupt or unsupported library is reported and left untouched.

`OPEN_AUDIO_DATA_DIR=/some/directory` selects an isolated library, useful for testing. The app makes no directory or feed requests on first launch. Network requests begin when you explicitly discover radio, subscribe/refresh a feed, or play an online source. There is no analytics collection.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/check-locales.py
```

Install the Rust `rustfmt` and `clippy` components if your toolchain does not include them. GTK layouts are in `resources/*.ui`; behavior is in Rust and styling in `resources/style.css`. Layouts and translation catalogues are embedded in the binary, so changes require a rebuild.

### Test the actual application

Install `xvfb`, `xauth`, `dbus-x11`, `libglib2.0-bin` (for `gdbus`), `xdotool`, Python 3, and ImageMagick, then:

```sh
make smoke
```

The integration test generates original audio, starts a local HTTP feed server, and drives the real application over D-Bus and keyboard input. It checks import, metadata, artwork, playback, seek, pause, favorites, queue, playlists, podcast/stream playback, MPRIS, mini mode, dialogs, and session restoration. All library and audio data are temporary. External services and real audio hardware are not required.

Validation for this implementation: 12 unit tests and strict Clippy checks pass, including with Rust 1.85 and GTK 4.8 in the Debian development container. Native, release, and AppImage builds pass the integration suite. The AppImage was exercised in a minimal Debian container without system GTK or GStreamer plugins, and its bundled TLS backend was checked. Debian and RPM packages build successfully; the Arch recipe has not been built on Arch.

For review screenshots:

```sh
cargo build --locked
OPEN_AUDIO_SCREENSHOTS=/tmp/open-audio-screens make smoke
```

Set `OPEN_AUDIO_BINARY` to an absolute binary path to run the same checks against a release build or AppImage. Use `APPIMAGE_EXTRACT_AND_RUN=1` when FUSE is unavailable; this keeps the AppImage runtime's library and plugin environment intact.

To explore with a separate demonstration library:

```sh
python3 scripts/create-demo.py /tmp/open-audio-demo-music
OPEN_AUDIO_DATA_DIR=/tmp/open-audio-demo-state \
  cargo run --locked -- --import /tmp/open-audio-demo-music
```

### Docker and devcontainer

```sh
docker build -t open-audio-player-dev .
docker run --rm open-audio-player-dev
```

The default container command runs formatting, lint, unit, translation, and virtual-display integration checks. Open the repository in a Dev Containers compatible editor to use `.devcontainer/devcontainer.json`. The container is configured for headless testing; interactive windows and real audio require explicitly forwarding your display and audio session. Run natively for ordinary desktop listening.

### Translations

`locales/en.json` is the source catalogue; the other nine files use the same English keys. `scripts/check-locales.py` checks parity and literal UI coverage. Missing keys fall back to English. The system language is the default, or select a language in Settings and restart. Technical errors from external libraries and operating-system dialogs can remain in their own language.

## Packaging

Packaging recipes are supplied for all four requested formats. Build on the oldest distribution you intend to support: native library and glibc compatibility follow the build environment.

```sh
make deb       # requires dpkg-deb; writes a .deb to dist/
make rpm       # requires rpmbuild; writes an RPM to dist/
make arch      # writes a source archive and PKGBUILD to dist/
# On Arch Linux:
cd dist && makepkg -si
```

For AppImage, provide [linuxdeploy](https://github.com/linuxdeploy/linuxdeploy), its [GTK plugin](https://github.com/linuxdeploy/linuxdeploy-plugin-gtk), and its [GStreamer plugin](https://github.com/linuxdeploy/linuxdeploy-plugin-gstreamer). Install `patchelf`, `file`, `adwaita-icon-theme`, `glib-networking`, and the librsvg/GObject Introspection development packages (`librsvg2-dev libgirepository1.0-dev` on Debian). Put the two plugin executables on `PATH` (their `.sh` filenames are also accepted):

```sh
LINUXDEPLOY=/absolute/path/to/linuxdeploy-x86_64.AppImage make appimage
```

The recipe includes the application, GTK runtime via the plugin, Adwaita icons, FFmpeg, GIO's HTTPS module, and the build host's GStreamer plugins and their dependent libraries. `yt-dlp` remains an optional host dependency. Test an AppImage on a clean target system before distributing it. Arch packages and cross-distribution compatibility have not been release-qualified.

For a local installation:

```sh
make build
sudo make install
# Remove the installed binary and desktop assets; listening data is retained:
sudo make uninstall
```

A project distribution license has not yet been selected; package metadata intentionally does not invent one. Select a license and review bundled dependencies before publishing releases.

## Project layout

```text
src/
  app.rs         GTK controllers, lists, dialogs, background work
  audio.rs       GStreamer playback and decoded-audio waveform
  desktop.rs     MPRIS, StatusNotifierItem, and tray menu
  i18n.rs        Embedded translation catalogues
  library.rs     Folder scanning, FFmpeg metadata, artwork, lyrics
  model.rs       Tracks, collections, queue, persisted session
  process.rs     Bounded subprocess execution without a shell
  services.rs    Podcast feeds, Radio Browser, artwork, YouTube
  storage.rs     XDG data location and atomic state storage
resources/       GtkBuilder layouts, D-Bus interfaces, CSS, icon
locales/         Ten translation catalogues
packaging/       Desktop entry, AppStream metadata, RPM, Arch
scripts/         Packaging, fixtures, checks, integration tests
```

The [original product brief](docs/brief.md), including the Rhythmbox and Clementine references, is preserved. [PRODUCT.md](PRODUCT.md) records the scope and [DESIGN.md](DESIGN.md) describes the visual system.

Implementation references: [GTK list factories](https://gtk-rs.org/gtk4-rs/stable/0.10/docs/gtk4/struct.ListItemFactory.html), [GStreamer playbin](https://gstreamer.freedesktop.org/documentation/playback/playbin.html), [MPRIS specification](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html), and [Radio Browser API](https://docs.radio-browser.info/).
