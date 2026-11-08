# YouTube Music and Spotify

[User guide](README.md) · [Installation](installation.md) · [Data privacy](data-privacy.md)

## YouTube Music

Install Python 3.10+ with venv support, `secret-tool` and a running desktop Secret Service keyring (GNOME Keyring or compatible). On Debian/Ubuntu:

```sh
sudo apt install python3-venv libsecret-tools
open-music-setup-youtube
```

Packaged installations include `open-music-setup-youtube`. From a source checkout, use `bash scripts/setup-youtube-music.sh` instead. Run it again to update the optional dependencies. It installs `ytmusicapi` and `yt-dlp[default]` into a private virtual environment beside your library; it does not change system Python. Playback also needs **Deno 2.3+** (preferred) or **Node.js 22+** on `PATH`, as described in [yt-dlp’s runtime setup](https://github.com/yt-dlp/yt-dlp/wiki/EJS). The helper enables whichever is available.

1. Open **Streaming → YouTube Music**, search for a song or artist and press Enter. Double-click a result to play it through GStreamer; queue, favorites, pause, seek and next/previous work as usual. Public search does not need an account.
2. Select **Connect account**, open YouTube Music in your browser and sign in there. Choose that browser in Open Music Player and connect. An optional profile path selects a nondefault profile. A Netscape-format cookie file is an alternative when the browser profile cannot be read. Close the browser before importing if its cookie database is locked.
3. Open **Your playlists → Sync playlists**. Online playlists appear in an artwork grid. Open one to browse or play its tracks. The global **Playlists** view also includes these linked collections.
4. Edit a linked playlist locally, then sync to upload its name and track changes. Online-only changes download on the next sync. Editing a read-only playlist or changing the same playlist on both sides produces a conflict and preserves both versions. Use **Replace local copy from YouTube Music** in its menu to explicitly accept the online version.
5. To publish a new local collection, add only YouTube Music tracks and choose **Publish privately to YouTube Music** from its card menu. It creates a private online playlist and links it for later sync. Local audio files cannot be uploaded through this feature.

Connection uses the first signed-in account in the selected browser profile; use a separate profile to select a different account. Google passwords are never entered into this app. Only YouTube-domain cookies are retained, in your desktop keyring. The library stores the account label, catalog metadata and playlist links; expiring audio URLs and credentials are not saved there. Imported cookie files are left untouched; protect or remove your export after connecting.

Sync runs when requested. Order and repeated tracks are preserved. Unavailable entries are reported, and incomplete playlists are left unchanged; the current safety limits are 250 playlists and 5,000 entries per playlist. Uploads add replacement entries before removing old ones, but network failures can leave a partial online update: check the online copy and refresh before retrying. Removing a linked playlist locally stops syncing it without deleting it online. **Account → Disconnect** clears the saved session and removes the local YouTube Music tracks and linked playlists; your online library is unchanged. Reconnect and sync to restore a removed local copy.

The embedded Python helper uses argument vectors and JSON pipes, never a shell. It resolves audio when playback or a download starts; Open Music Player transfers the resolved audio stream. `OPEN_MUSIC_PYTHON` can select an existing interpreter containing both libraries instead of the managed environment.

## Spotify

1. Open **Streaming → Spotify → Connect account → Connect with Spotify**. A **Spotify Premium** account and an unlocked desktop Secret Service keyring (such as GNOME Keyring) are required. Spotify support is compiled into the application; no separate librespot executable, Python package or setup script is needed.
2. Sign in and approve access in your browser. The app uses authorization-code **PKCE**, a random state check, a loopback-only callback on `http://127.0.0.1:5588/login`, a three-minute deadline and a cancel button. No Spotify password is entered or stored in Open Music Player. The refresh token stays in the desktop keyring; only account labels, catalog metadata and playlist links go in `library.json`.
3. Search for a song or artist and press Enter. Double-click a track to play. librespot decodes the audio and sends PCM to GStreamer, keeping volume, waveform, queue, shuffle/repeat, mini player and MPRIS controls shared with other sources. Search currently displays up to ten results. Track and linked-playlist menus include **Open in Spotify**.
4. Select **Your playlists → Sync playlists** to import accessible playlists. Edit a linked playlist locally and sync to upload track order and name changes. Online-only changes download; conflicting changes and local edits made during a sync are preserved. **Replace local copy from Spotify** explicitly discards local edits. Use **Publish privately to Spotify** on a local playlist containing only Spotify tracks to create an online copy.

Uploads are restricted to owned playlists containing only available Spotify tracks. Local files and podcast episodes cannot be uploaded; skipped entries are reported and make the imported playlist read-only for uploading. Sync fetches complete paginated collections, up to 500 playlists and 5,000 entries per playlist, preserving order and repeats. Large uploads use Spotify’s 100-item batches; an interrupted upload can leave a partial online update while keeping the local copy. Inspect the online copy before retrying. Removing a linked playlist locally stops its synchronization without deleting it online; disconnect and reconnect to import it again.

**Account → Disconnect** removes saved sign-in, Spotify tracks and linked playlists from this computer. Existing sign-in is restored in the background at startup, without starting playback. Search and sync show connection, permission and rate-limit failures instead of treating failed requests as empty collections.

By default, sign-in uses librespot’s public OAuth client. If Spotify denies catalog or playlist access, **Advanced connection options** accepts a Spotify developer Client ID (`OPEN_MUSIC_SPOTIFY_CLIENT_ID` can prefill it). Register the exact callback above; no client secret is needed. Spotify’s [current development-mode rules](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide) limit apps/users and may restrict playlist contents to owned or collaborative playlists. A custom Client ID does not bypass those limits. Spotify can also change unofficial-client access.

Offline tests cover authentication callback validation, playlist conflicts, stale responses, PCM output and the GTK flows. A real Premium account is still needed to verify end-to-end browser sign-in, live Spotify playback and online playlist writes; these have not been validated with a live account by the offline test suite.
