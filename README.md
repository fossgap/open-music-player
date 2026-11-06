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
| Collections | Favorites, playlist artwork grid, create/rename/delete playlists, M3U/M3U8/PLS/XSPF import, queue, shuffle, repeat one/all |
| Playback | GStreamer audio, play/pause, previous/next, seeking, volume, mute, automatic next track |
| Podcasts | Apple Podcasts/iTunes and gPodder search and discovery, RSS/Atom subscriptions, feed refresh, per-episode resume positions |
| Internet radio | Search and discovery through Radio Browser, station artwork, favorites, playback |
| Streaming | Direct HTTP(S) audio and supported streaming playlists |
| YouTube Music | Song search, personal browser-session connection, native playback, two-way playlist sync and private playlist publishing |
| Desktop | MPRIS controls and metadata, StatusNotifierItem tray menu, background playback, mini player |
| Artwork and lyrics | Embedded covers, folder covers, feed/station images, softly blended artwork backdrop, embedded lyrics and `.lrc`/`.txt` sidecars |
| Visualization | Live waveform from decoded PCM audio; no synthetic playback animation |
| Persistence | Library, sources, playlists, queue, track, position, volume, repeat, shuffle, and preferences |
| Languages | English, French, Spanish, German, Italian, Portuguese, Russian, Chinese, Japanese, Korean; new service copy falls back to English where untranslated |

**Service boundaries:** YouTube Music uses the unofficial `ytmusicapi` and `yt-dlp` libraries, with no API key or Premium subscription requirement in Open Audio Player. Availability depends on your account, region and YouTube; sessions can expire and provider changes can require dependency updates. Podcast downloads, synchronized lyric highlighting, and automatic filesystem watching are not included. This is an initial implementation, not a published stable release.

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

### Enable YouTube Music

Install Python 3.10+ with venv support, `secret-tool` and a running desktop Secret Service keyring (GNOME Keyring or compatible). On Debian/Ubuntu:

```sh
sudo apt install python3-venv libsecret-tools
./scripts/setup-youtube-music.sh
```

Packaged installations include `open-audio-setup-youtube`. Run it again to update the optional dependencies. It installs `ytmusicapi` and `yt-dlp[default]` into a private virtual environment beside your library; it does not change system Python. Playback also needs **Deno 2.3+** (preferred) or **Node.js 22+** on `PATH`, as described in [yt-dlp’s runtime setup](https://github.com/yt-dlp/yt-dlp/wiki/EJS). The helper enables whichever is available.

1. Open **YouTube Music**, search for a song or artist and press Enter. Double-click a result to play it through GStreamer; queue, favorites, pause, seek and next/previous work as usual. Public search does not need an account.
2. Select **Connect account**, open YouTube Music in your browser and sign in there. Choose that browser in Open Audio Player and connect. An optional profile path selects a nondefault profile. A Netscape-format cookie file is an alternative when the browser profile cannot be read. Close the browser before importing if its cookie database is locked.
3. Open **Your playlists → Sync playlists**. Online playlists appear in an artwork grid. Open one to browse or play its tracks. The global **Playlists** view also includes these linked collections.
4. Edit a linked playlist locally, then sync to upload its name and track changes. Online-only changes download on the next sync. Editing a read-only playlist or changing the same playlist on both sides produces a conflict and preserves both versions. Use **Replace local copy from YouTube Music** in its menu to explicitly accept the online version.
5. To publish a new local collection, add only YouTube Music tracks and choose **Publish privately to YouTube Music** from its card menu. It creates a private online playlist and links it for later sync. Local audio files cannot be uploaded through this feature.

Connection uses the first signed-in account in the selected browser profile; use a separate profile to select a different account. Google passwords are never entered into this app. Only YouTube-domain cookies are retained, in your desktop keyring. The library stores the account label, catalog metadata and playlist links; expiring audio URLs and credentials are not saved there. Imported cookie files are left untouched; protect or remove your export after connecting.

Sync runs when requested. Order and repeated tracks are preserved. Unavailable entries are reported, and incomplete playlists are left unchanged; the current safety limits are 250 playlists and 5,000 entries per playlist. Uploads add replacement entries before removing old ones, but network failures can leave a partial online update: check the online copy and refresh before retrying. Removing a linked playlist locally stops syncing it without deleting it online. **Account → Disconnect** clears the saved session and removes the local YouTube Music tracks and linked playlists; your online library is unchanged. Reconnect and sync to restore a removed local copy.

The embedded Python helper uses argument vectors and JSON pipes, never a shell. It resolves audio only when playback starts and does not download music files. `OPEN_AUDIO_PYTHON` can select an existing interpreter containing both libraries instead of the managed environment.

## Listening

1. Select **Add music** and choose a folder. Scanning runs in the background; additional folders are managed in **Settings**.
2. Single-click to select a track or album. Double-click a track to play, or double-click an album to open its tracks. Use **Back** to return to the album grid. Playback keeps the list's scroll position; playing a track from a filtered queue preserves the whole queue.
3. Use the star to save a favorite. The track menu adds to the queue or a playlist. Removing a local track only removes its index entry; refreshing its folder can discover it again.
4. Select **Podcasts → Discover podcasts**. Choose **Apple Podcasts** (the iTunes directory) or **gPodder**, search by title, topic, or creator, then select **Subscribe**. **Your episodes** lists subscribed episodes. You can also add any RSS or Atom feed URL. Use **Refresh** to fetch new episodes. Removing a subscription stops refreshes and retains previously indexed episodes.
5. Select **Live radio → Discover radio**, or type a station name and press Enter. Radio results are provided by the community-maintained [Radio Browser directory](https://www.radio-browser.info/).
6. Select **Streaming → Add stream** and paste an HTTP(S) audio address. A station URL can also be opened here. Use the dedicated **YouTube Music** page to search and play music from that service.

Click the current cover to open the artwork, waveform, and lyrics panel. Put `cover.jpg`, `cover.png`, `folder.jpg`, or `folder.png` beside an album, or use embedded artwork. A lyrics sidecar must share the audio filename: `Song.flac` and `Song.lrc`. Refresh after adding a sidecar. LRC text is displayed as supplied.

The session is restored without automatically starting audio. Press Play to continue. Podcast episodes also retain individual positions.

### Playlists

**Playlists** opens a grid of your collections, showing a member's artwork and the playlist name. Click a playlist to see its tracks; double-click a track to play it. Use **New playlist** to create a collection and a track's options menu to add it. Rename or delete a playlist from its card menu or while viewing its tracks. In a compact window, these actions move into **Playlist options** beside the heading. Deletion asks for confirmation and retains the music files, library, and playing queue.

The **Import playlist** button beside **New playlist** accepts UTF-8 `.m3u`, `.m3u8`, `.pls`, and `.xspf` files. Relative paths resolve beside the playlist file; absolute paths, `file://` URLs, and HTTP(S) audio links are supported. Imports preserve order and existing track metadata and favorites. Missing, unsupported, and duplicate entries are skipped with a count. An imported name that already exists gets a numbered suffix. Importing creates a playlist without starting playback.

Directory discovery uses an initial podcast search on Apple and the public top list on gPodder; it does not require an account. Results and feed availability depend on those services. A failed directory request offers connection and provider recovery; direct RSS subscription remains available.

Closing the main window keeps audio playing **only while playback is active and a compatible system tray is available**. Otherwise the application exits cleanly. On GNOME, a StatusNotifier/AppIndicator extension is needed for tray integration; MPRIS and the mini player work independently of the tray. Choose **Quit** to stop playback and exit explicitly.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl + Space | Play / pause |
| Alt + Left / Right | Previous / next |
| Alt + Up | Back to the album or playlist grid |
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

`OPEN_AUDIO_DATA_DIR=/some/directory` selects an isolated library, useful for testing. The app makes no directory or feed requests on first launch. Network requests begin when you explicitly discover/search podcasts or radio, subscribe/refresh a feed, or play an online source. Searches go to the selected directory; result artwork may be fetched from its image hosts. There is no analytics collection. Existing playlists receive persistent identifiers automatically when an older library is loaded.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/test-youtube-music.py
python3 scripts/check-locales.py
```

Install the Rust `rustfmt` and `clippy` components if your toolchain does not include them. GTK layouts are in `resources/*.ui`; behavior is in Rust and styling in `resources/style.css`. Layouts and translation catalogues are embedded in the binary, so changes require a rebuild.

### Test the actual application

Install `xvfb`, `xauth`, `dbus-x11`, `libglib2.0-bin` (for `gdbus`), `xdotool`, Python 3, and ImageMagick, then:

```sh
make smoke
make ui-test
```

The integration test generates original audio, starts local feed and podcast-directory fixtures, and drives the real application over D-Bus and keyboard input. It checks library and playlist import, metadata, artwork, playback, seek, pause, favorites, queue, playlist creation/renaming, both podcast directories, subscriptions, empty/error states, MPRIS, mini mode, dialogs, compact layouts, and session restoration. The GTK regression test scrolls long lists and checks that activation preserves scroll and selection, album browsing does not start playback, filtered queue playback retains its members, and playlist deletion respects cancellation. All library and audio data are temporary. External services and real audio hardware are not required.

The unit suite contains 19 tests; the display-dependent GTK regression runs separately through `make ui-test`. Validation uses native Linux and the Rust 1.85/GTK 4.8 Debian development container, with strict Clippy checks. The same integration suite can exercise native, release, and AppImage builds. The Arch recipe has not been built on Arch.

For review screenshots:

```sh
cargo build --locked
OPEN_AUDIO_SCREENSHOTS=/tmp/open-audio-screens make smoke
```

Set `OPEN_AUDIO_BINARY` to an absolute binary path to run the same checks against a release build or AppImage. Use `APPIMAGE_EXTRACT_AND_RUN=1` when FUSE is unavailable; this keeps the AppImage runtime's library and plugin environment intact.

`OPEN_AUDIO_ITUNES_API` and `OPEN_AUDIO_GPODDER_API` optionally replace the respective API base URLs for a private mirror or offline test server. Normal use needs neither setting.

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
  app.rs         GTK controllers, playback, background work
  app/browse.rs  Track/album views, playlist management, podcast browser
  app/tests.rs   GTK scroll, selection, and collection regressions
  audio.rs       GStreamer playback and decoded-audio waveform
  desktop.rs     MPRIS, StatusNotifierItem, and tray menu
  i18n.rs        Embedded translation catalogues
  library.rs     Folder scanning, FFmpeg metadata, artwork, lyrics
  model.rs       Tracks, collections, queue, persisted session
  playlists.rs   M3U/M3U8, PLS, and XSPF parsing and import
  process.rs     Bounded subprocess execution without a shell
  services.rs    Podcast directories/feeds, Radio Browser, artwork, YouTube
  storage.rs     XDG data location and atomic state storage
resources/       GtkBuilder layouts, D-Bus interfaces, CSS, icon
locales/         Ten translation catalogues
packaging/       Desktop entry, AppStream metadata, RPM, Arch
scripts/         Packaging, fixtures, checks, integration tests
```

The [original product brief](docs/brief.md), including the Rhythmbox and Clementine references, is preserved.

Implementation references: [GTK list factories](https://gtk-rs.org/gtk4-rs/stable/0.10/docs/gtk4/struct.ListItemFactory.html), [GStreamer playbin](https://gstreamer.freedesktop.org/documentation/playback/playbin.html), [MPRIS specification](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html), [Apple Search API](https://performance-partners.apple.com/search-api), [gPodder API](https://gpoddernet.readthedocs.io/en/latest/api/), and [Radio Browser API](https://docs.radio-browser.info/).
