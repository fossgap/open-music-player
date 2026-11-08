# Development

[README](../README.md) · [CI and releases](ci-release.md) · [Packaging](packaging.md)

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/test-youtube-music.py
python3 scripts/check-locales.py
```

The minimum Rust version is 1.89: the current Lofty tag library uses `Seek` for `std::io::Take`, [stabilized in Rust 1.89](https://doc.rust-lang.org/std/io/struct.Take.html#impl-Seek-for-Take%3CT%3E).

Install the Rust `rustfmt` and `clippy` components if your toolchain does not include them. GTK layouts are in `resources/*.ui`; behavior is in Rust and styling in `resources/style.css`. Layouts and translation catalogues are embedded in the binary, so changes require a rebuild.

### Test the actual application

Install `xvfb`, `xauth`, `dbus-x11`, `libglib2.0-bin` (for `gdbus`), `xdotool`, Python 3, and ImageMagick, then:

```sh
make smoke
make ui-test
```

For window-manager checks, also install `xfwm4` and `xprop`, build with `cargo build --locked`, then run `xvfb-run -a dbus-run-session -- python3 scripts/window-state-test.py`. This verifies resize/maximize restoration, quitting from the mini player, and hidden startup with temporary application and window-manager preferences.

The integration test generates original audio, starts local feed and podcast-directory fixtures, and drives the real application over D-Bus and keyboard input. It checks library and playlist import, metadata, artwork, playback, seek, pause, favorites, queue, playlist creation/renaming, both podcast directories, subscription toggles, empty/error states, MPRIS, mini mode, dialogs, compact layouts, and session restoration. The GTK regression test checks card Play all, wheel volume/seek limits, refresh scheduling, unsubscribe races, and artwork/lyrics against local provider fixtures, settings switches, attribution, retries, and compact detail-panel scrolling. It checks multi-selection, bulk playlist actions, typed file/track drops, duplicate-entry removal, file-manager requests, and live folder changes. It also scrolls long lists and checks that activation preserves scroll and selection, album browsing does not start playback, filtered queue playback retains its members, and playlist deletion respects cancellation. All library and audio data are temporary. External services and real audio hardware are not required.

The unit suite includes offline cover/lyric matching, positive/negative caching, invalid/oversized images, rate-limit backoff, cancellation, and Spotify authentication, catalog, sync and PCM-output checks. Tag regressions verify decoded-audio preservation across formats, unrelated metadata, artwork/lyrics round trips, failed/cancelled writes, and overrides surviving rescans. The GTK suite also exercises single/bulk tag editing, review, read-only errors, both themes, and compact layouts. The display-dependent GTK regression runs separately through `make ui-test`. CI targets the Rust 1.89/GTK 4.8 Debian development container; native Linux checks include strict Clippy validation. The same integration suite can exercise native, release, and AppImage builds. Release packages are installed and exercised in clean distribution containers; see [CI and releases](ci-release.md).

For review screenshots:

```sh
cargo build --locked
OPEN_MUSIC_SCREENSHOTS=/tmp/open-music-screens make smoke
```

Set `OPEN_MUSIC_BINARY` to an absolute binary path to run the same checks against a release build or AppImage. Use `APPIMAGE_EXTRACT_AND_RUN=1` when FUSE is unavailable; this keeps the AppImage runtime's library and plugin environment intact.

`OPEN_MUSIC_ITUNES_API`, `OPEN_MUSIC_GPODDER_API` and `OPEN_MUSIC_RADIO_API` optionally replace the respective API base URLs for a private mirror or offline test server. The optional `OPEN_MUSIC_MUSICBRAINZ_API`, `OPEN_MUSIC_COVER_ART_API`, `OPEN_MUSIC_LRCLIB_API`, `OPEN_MUSIC_WIKIPEDIA_API`, and `OPEN_MUSIC_WIKIDATA_API` variables similarly replace metadata API endpoints. Normal use needs none of these settings. Overrides send the same request data to the replacement server; see [data privacy](wiki/data-privacy.md).

To explore with a separate demonstration library:

```sh
python3 scripts/create-demo.py /tmp/open-music-demo-music
OPEN_MUSIC_DATA_DIR=/tmp/open-music-demo-state \
  cargo run --locked -- --import /tmp/open-music-demo-music
```

### Docker and devcontainer

```sh
docker build -t open-music-player-dev .
docker run --rm open-music-player-dev
```

The default container command runs formatting, lint, unit, translation, and virtual-display integration checks. Open the repository in a Dev Containers compatible editor to use `.devcontainer/devcontainer.json`. The container is configured for headless testing; interactive windows and real audio require explicitly forwarding your display and audio session. Run natively for ordinary desktop listening.

### Translations

`locales/en.json` is the source catalogue; the other nine files use the same English keys. `scripts/check-locales.py` checks parity and literal UI coverage. Missing keys fall back to English. The system language is the default, or select a language in Settings and restart. Technical errors from external libraries and operating-system dialogs can remain in their own language.

## Project layout

```text
src/
  app.rs         GTK controllers, playback, background work
  app/browse.rs  Track/album views, playlist management, podcast browser
  app/files.rs   Local file drops, folder watching controls, file manager
  app/selection.rs Multi-selection, bulk actions, playlist/queue reordering
  app/tags.rs    Single/bulk tag editor, review, progress and failure handling
  app/search.rs  Global search view, result actions and podcast previews
  app/tests.rs   GTK scroll, selection, collection and streaming regressions
  app/spotify.rs Spotify account and playlist interface
  app/youtube.rs YouTube Music account and playlist interface
  audio.rs       Playback routing, next-track preparation and crossfades
  audio/         Gapless GStreamer decks, ReplayGain and PCM regression tests
  desktop.rs     MPRIS, StatusNotifierItem, and tray menu
  i18n.rs        Embedded translation catalogues
  enrichment.rs  Background cover/lyric jobs, cancellation, metadata matching
  enrichment/    MusicBrainz, Cover Art Archive, LRCLIB clients, cache and tests
  library.rs     Folder scanning, FFmpeg metadata, artwork, lyrics
  tags.rs        Structured tag fields, validation and library-only overrides
  tags/          Native tag reading/writing, safe replacement and regression tests
  watch.rs       Recursive native GIO folder monitors on a worker thread
  collections.rs Occurrence-aware collection ordering and removal
  model.rs       Tracks, collections, queue, persisted session
  search.rs      Ranked collection search with background indexing
  search/        Independent provider workers and search regression tests
  playlists.rs   M3U/M3U8, PLS, and XSPF parsing and import
  process.rs     Bounded subprocess execution without a shell
  services.rs    Podcast directories/feeds, Radio Browser and artwork
  spotify.rs     Background Spotify account/catalog service
  spotify/       PKCE, Web API catalog/sync and librespot PCM playback
  youtube_music.rs YouTube Music helper bridge and sync merge
  storage.rs     XDG data location and atomic state storage
resources/       GtkBuilder layouts, D-Bus interfaces, CSS, icon
locales/         Ten translation catalogues
packaging/       Desktop entry, AppStream metadata, RPM, Arch
scripts/         Packaging, fixtures, checks, integration tests
```

The [original product brief](brief.md), including the Rhythmbox and Clementine references, is preserved.
