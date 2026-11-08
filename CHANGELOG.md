# Changelog

## 0.1.3 — 2026-11-08

- Simplify Settings by removing redundant descriptions for themes, both background controls, download folders, artist and album information, online artwork and lyrics, and folder watching.
- Remove the unused strings from all ten translation catalogues.

## 0.1.2 — 2026-11-08

- Bundle sidebar-toggle and queue icons so they remain available across desktop icon themes.
- Use the Open Music Player logo in the system tray, with embedded images at multiple sizes for portable launches.
- Require the SVG loader in Debian packages so embedded icons render on minimal installations.
- Verify bundled icon lookup without system themes and check the tray image sent over D-Bus in desktop package tests.
- Make headless window-close tests independent of titlebar rendering timing.

## 0.1.1 — 2026-11-08

First public packaged release.

- Add Linux x86_64 DEB, Fedora RPM, Arch package, AppImage, source archive, and SHA-256 checksums.
- Verify installation and playback on Debian, Kali, Fedora, Arch, and a minimal Ubuntu AppImage host before publishing.
- Complete the AppImage runtime, including GTK, GStreamer, FFmpeg, fonts, and supporting libraries.
- Add the user wiki and a data privacy guide covering external services, transmitted information, default settings, and local storage.
- Declare GPL-3.0-only licensing and document Rust 1.89+ as the source-build requirement.
- Make desktop regression tests account for GTK minimum-size constraints and runner timing.
