# Repository Guidelines

## Project Structure & Module Organization

- `src/app.rs` and `src/app/` contain GTK application behavior, browsing, settings, and service controls.
- `src/model.rs`, `storage.rs`, and `library.rs` manage persisted state and local music; `audio.rs` handles GStreamer playback.
- `src/services.rs`, `enrichment/`, `spotify/`, and `youtube_music.rs` implement external services. The Python YouTube Music bridge lives in `resources/youtube_music.py`.
- `resources/` contains GTK `.ui` layouts, CSS, and icons; `locales/` contains ten JSON translation catalogues.
- `scripts/` provides tests, demos, and packaging helpers; `packaging/` contains distribution recipes. Rust tests live beside implementation code or in module-specific `tests.rs` files.

## Build, Test, and Development Commands

Install Rust 1.89+, GTK 4.8+, GStreamer development libraries/plugins, and FFmpeg; see `README.md` for distribution dependencies.

- `make run`: build and launch the development application.
- `make build`: produce the release binary in `target/release/`.
- `make check`: check Rust formatting, run strict Clippy, and validate translation catalogues.
- `make test`: run Rust tests and Python `unittest` tests for the streaming bridge.
- `make ui-test`: run GTK regression tests under Xvfb and an isolated D-Bus session.
- `make smoke`: exercise the real application, playback, directories, and persistence using local fixtures.

Layouts and translations are embedded; rebuild after editing them. CI runs checks inside the repository's Docker image.

## Coding Style & Naming Conventions

Use `cargo fmt` for Rust formatting. Use four-space indentation in Rust/Python and two spaces in JSON. Prefer `snake_case` for functions, modules, and GTK widget IDs; use `PascalCase` for Rust types. Keep network and scanning work off the GTK thread. Add UI strings to every locale catalogue, and preserve compatibility with existing serialized library data.

## Testing Guidelines

Add focused regression tests for behavior changes, using descriptive names such as `playlist_reads_reject_changed_revisions`. Prefer temporary libraries, local HTTP fixtures, and mocked providers over real accounts. Run `make check` and `make test`; run both desktop suites for UI or playback changes. Capture review images with `OPEN_MUSIC_SCREENSHOTS=/tmp/open-music-screens make smoke`.

## Commit & Pull Request Guidelines

Use concise imperative subjects, following history: `Add cached cover and lyrics providers`. Keep commits focused. PRs should explain the problem, resulting behavior, validation, and relevant issues; include screenshots for visible changes. Exclude unrelated workspace edits.

## Security & Configuration

Use `OPEN_MUSIC_DATA_DIR=/tmp/open-music-dev` for isolated manual testing. Store credentials in the desktop keyring, never in source, logs, or `library.json`.

Seek free, publicly available providers that support anonymous access. Avoid sources requiring subscriptions or API keys unless explicitly requested by the user.
