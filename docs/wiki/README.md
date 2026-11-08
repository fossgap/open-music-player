# Open Music Player user guide

[Project](../../README.md) · [Releases](https://github.com/fossgap/open-music-player/releases) · [Report a problem](https://github.com/fossgap/open-music-player/issues)

Listen to local music, podcasts, radio, and online music in a native Linux player.
This wiki is versioned with the application, so a release's source archive includes
the instructions for that version. Installed packages also include it under
`/usr/share/doc/open-music-player/wiki/`.

| Guide | What you will find |
| --- | --- |
| [Installation](installation.md) | Packages, checksums, AppImage, source builds, and updates |
| [Listening and your library](listening.md) | Music folders, search, tags, playlists, podcast subscriptions, radio, artwork, lyrics, and artist information |
| [Playback, downloads, and shortcuts](playback.md) | Queue controls, gapless/crossfade, ReplayGain, recordings, downloads, and keyboard shortcuts |
| [YouTube Music and Spotify](streaming.md) | Optional setup, account connection, two-way playlist sync, publishing, and disconnection |
| [Online services](services.md) | Anonymous access and account requirements |
| [Data privacy](data-privacy.md) | Every external service, information sent, automatic requests, local storage, and privacy controls |
| [Troubleshooting](troubleshooting.md) | Playback, codecs, accounts, desktop integration, and useful bug reports |

## First use

1. [Install](installation.md) and launch **Open Music Player** from your applications menu.
2. Review [data privacy](data-privacy.md) before importing music if you prefer to disable automatic metadata lookups.
3. Choose **Add music**, select a folder, and double-click a track to play it.
4. Add favorites or playlists, or open **Podcasts**, **Live radio**, or **Streaming**.
5. Use **Settings** for your language, theme, music/download folders, playback, and background behavior.

The default interface language follows the system. You can choose English, French,
Spanish, German, Italian, Portuguese, Russian, Chinese, Japanese, or Korean in
Settings; restart after changing it. Missing translations fall back to English.

Closing the window keeps the app running by default. Use **Quit** or **Ctrl + Q**
to exit, or turn off **Keep running when the window closes** in Settings.

For contributors: [development](../development.md), [packaging](../packaging.md),
and [CI and releases](../ci-release.md).
