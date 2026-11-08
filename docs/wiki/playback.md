# Playback, downloads, and shortcuts

[User guide](README.md) · [Installation](installation.md) · [Data privacy](data-privacy.md)

## Downloads and recordings

Open an online track’s **options menu**, or use the **save button to the right of the playback controls**, to save audio. The player button adapts to the current source: download finite audio, record radio/live streams or Spotify, cancel a transfer, stop and save a recording, resume a cancelled download, or show its saved copy. Local files keep this button disabled. The **Downloads** sidebar shows progress, size, completed files and errors; completed entries offer **Play saved audio** and **Open location**.

- **Podcasts and YouTube Music:** **Download audio** saves a complete file, preserving the source format where available. Playback of that track then prefers the downloaded copy; if the file is moved or deleted, ordinary playback falls back to the online source.
- **Radio:** **Record stream** records independently of the player, so listening, pausing or changing tracks does not interrupt it. **Stop and save** finalizes a FLAC file. FLAC preserves the captured audio without adding another lossy encoding step; it cannot improve the source quality.
- **Direct streams:** choose **Download audio** for a finite file or **Record stream** for a live broadcast. HLS/DASH audio downloads are decoded to FLAC; a live manifest runs until cancelled.
- **Spotify:** **Play and record** captures decoded playback into FLAC using the connected Premium account. This is a playback recording, not Spotify’s offline-download feature. Pausing pauses capture; manual track changes stop it, while natural gapless transitions can continue the same recording. Speed and volume controls do not alter the captured source PCM.

**Settings → Download folder** chooses where new jobs save files; by default this is `Downloads/Open Music Player` under your user directory. Existing files stay in place. Up to three network jobs run at once, with playback recording handled separately. Unknown-length sources show indeterminate progress. **Cancel download** keeps a partial file for **Resume** during this session. Resume continues from the saved bytes when the server supports byte ranges with a matching strong ETag; otherwise it safely starts again. HLS/DASH downloads restart. **Delete download** cancels and discards unfinished transfers, or moves a completed file to the system Trash and removes its entry and offline playback link. Trash failures leave the item intact. **Cancel and discard** deletes an unfinished recording; **Stop and save** keeps it. Live recordings cannot resume missed audio. Quitting discards unfinished transfers. Transfers do not resume across restarts; completed history and file locations persist. Existing filenames are never overwritten. A recording stays separate from the original station or track.

Use the **speed button to the left of the playback controls** for 0.5–2× playback, with pitch preserved. Seekable GStreamer sources use `scaletempo`; Spotify uses the optional GStreamer SoundTouch `pitch` plugin. Unsupported/live streams play at normal speed. Non-default speeds use ordinary track transitions instead of gapless/crossfade. Click the **time to the left of the seek bar** to switch elapsed/remaining time; unknown remaining duration displays `—:—`. Both preferences persist and speed is also exposed through MPRIS.

## Playback settings

**Settings → Playback** saves these preferences between sessions:

| Option | Default | Behavior |
|---|---|---|
| Gapless playback | On | Prepares the next queued song without restarting the audio output. Works between GStreamer music sources and between Spotify tracks; changing playback backends uses a normal transition. |
| Crossfade | Off (0 seconds) | Overlaps music for 1–12 seconds, with shorter fades for short songs. Applies to local music, YouTube Music and finite direct music streams. Podcasts, radio and repeat-one playback never crossfade. Spotify crossfade is not implemented. |
| ReplayGain | Off | Track gain evens out tagged tracks; Album gain preserves an album's relative levels and falls back to track gain when album tags are missing. Existing ReplayGain tags are read during playback, with peak protection when peak tags are present. Untagged audio is unchanged; no analysis or file rewriting takes place. Spotify uses librespot's own loudness data, with changes applied on the next track. |

Crossfade takes precedence over gapless for eligible transitions. Manual next/previous starts the selected song immediately; seeking ends any active overlap. Pause, mute and volume affect both songs during a fade. Queue edits, shuffle and repeat determine the prepared successor. If a speculative preload fails, ordinary next-track playback retries it.

YouTube Music resolves the next playback URL shortly before the transition, retaining its request headers. Network delays, expiring URLs and missing loudness tags can limit streaming transitions or normalization. These features add no provider accounts, subscriptions or API keys.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl + Space | Play / pause |
| Alt + Left / Right | Previous / next |
| Alt + Up | Back to the album or playlist grid |
| Ctrl / Shift + click | Select separate tracks / a range |
| Ctrl + A | Select all tracks in the focused list |
| Escape | Clear track selection; close a settings or playlist dialog |
| Delete | Remove selected entries from the focused track list; keep files |
| Ctrl + Shift + Up / Down | Move selected playlist/queue entries one position |
| Ctrl + Shift + Home / End | Move selected playlist/queue entries to the top/bottom |
| Ctrl + O | Add a music folder |
| Ctrl + F | Focus search |
| Ctrl + K | Open global search |
| Ctrl + R | Refresh the current source |
| Ctrl + L | Artwork and lyrics |
| Ctrl + M | Mini player |
| Ctrl + , | Settings |
| Ctrl + Q | Quit |
