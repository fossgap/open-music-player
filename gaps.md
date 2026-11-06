
## Assistant

**Open Audio Player\u2019s biggest gaps are library management, playlist editing and advanced playback controls.** I catalogued **102 gap items: 70 missing capabilities and 32 areas where basic support already exists but is less complete.**

The full inventory includes the current behavior, comparison apps, source-file evidence and qualifications: **[Download the detailed comparison spreadsheet](/tmp/open-audio-player-research-2026-11-06/feature-gaps.csv)**.

I couldn\u2019t substantiate a definitive Linux-wide \u201cthree most used\u201d ranking. Debian\u2019s opt-in usage reports and Flathub\u2019s download counts measure different populations and produce different rankings. I therefore selected three established, widely distributed benchmarks:

| Application examined | Why it belongs in the comparison | Flathub popularity signal |
|---|---|---|
| **Rhythmbox 3.5.1** | Established GNOME library manager; particularly useful for comparing podcasts and device integration | Approximately [3,000 downloads/month](https://flathub.org/en/apps/org.gnome.Rhythmbox3) |
| **Audacious 4.6.2** | Established lightweight player with extensive playlist, audio and plugin functionality | Approximately [4,300 downloads/month](https://flathub.org/en/apps/org.atheme.audacious) |
| **Strawberry 1.2.31** | Comprehensive collection manager, with strong metadata and audio-output controls | Approximately [9,000 downloads/month](https://flathub.org/en/apps/org.strawberrymusicplayer.strawberry) |

Those figures are download counts, not active users or market shares. Other applications, including GNOME Music and Quod Libet, score higher under some [Debian measurements](https://popcon.debian.org/), so this selection should be understood as a practical benchmark rather than a proven popularity podium.

I examined the three official release source distributions alongside their documentation, and compared them with Open Audio Player\u2019s **current working tree, including the recent playlist and podcast improvements**. This is a source and documentation audit; external accounts and physical devices were not exercised.

Open Audio Player already has playlist creation, renaming, deletion, artwork grids and M3U/M3U8/PLS/XSPF import; album browsing; favorites; queue removal and clearing; shuffle/repeat; podcast discovery, subscriptions and individual resume positions; radio; artwork; local lyrics; a live waveform; MPRIS; tray controls; a mini player; and session persistence. Those are accounted for below.

In the tables, **R = Rhythmbox, A = Audacious, S = Strawberry**. **\u201cPartial\u201d** means a related capability already exists in Open Audio Player. The comparison column gives verified examples, not an exhaustive support matrix.

For **metadata and collection information**, the differences extend well beyond reading song titles. Strawberry\u2019s [tag editor](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/dialogs/edittagdialog.ui) and [collection preferences](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/settings/collectionsettingspage.ui) provide concrete examples.

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 1 | Edit tags for one track or a selection of tracks | R, S, A |
| 2 | Identify recordings and complete tags using acoustic fingerprints, AcoustID and MusicBrainz | S |
| 3 | **Partial:** retain extended tags: genre, year, album artist, composer, performer, disc, grouping, comments and sorting fields | R, S |
| 4 | Group compilation albums using album artist/\u201cVarious Artists\u201d rather than each track\u2019s artist | S |
| 5 | Order multi-disc albums by disc number followed by track number | R, S |
| 6 | Respect sort tags and optionally ignore leading articles such as \u201cThe\u201d | S |
| 7 | **Partial:** star ratings beyond the existing favorite toggle | R, S |
| 8 | Record play counts, skip counts and last-played dates | S; R for play count and last played |
| 9 | Read/write ratings and play counts through audio-file tags | S |
| 10 | **Partial:** track-properties inspector showing codec, bitrate, sample rate, bit depth, size and location | R, S |

For **library browsing and file management**, the main limitation is that Open Audio Player currently offers a relatively fixed view over its indexed tracks. Strawberry\u2019s [grouping controls](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/collection/groupbydialog.ui), [file organizer](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/organize/organizedialog.ui) and [collection watcher](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/collection/collectionwatcher.cpp) illustrate the broader workflows.

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 11 | **Partial:** dedicated artist, genre and composer browsing; configurable grouping hierarchies | R, S |
| 12 | **Partial:** selectable sorting, ascending/descending order and configurable columns | R, S, A |
| 13 | **Partial:** field-specific searches, numeric comparisons and Boolean filters | S |
| 14 | Recently-added views and rating-based filters | S |
| 15 | Automatically watch music folders for additions, changes and removals | R, S |
| 16 | Optional startup refresh and cancellation of an ongoing scan | S |
| 17 | Preserve track identity and associated data when files move or are renamed | S |
| 18 | **Partial:** mark unavailable tracks and offer recovery workflows instead of simply pruning missing entries | R, S |
| 19 | **Partial:** inspect and clean up duplicates beyond repeated file paths | S, A |
| 20 | Copy, move and rename files using metadata-based naming patterns | S |
| 21 | Browse the filesystem and open individual files without first importing a library folder | A, S |
| 22 | Open a track\u2019s containing folder in the file manager | A |
| 23 | Explicitly move files to trash or delete them from disk | R, S |

Two distinctions matter here: the current scanner already reuses unchanged-file metadata, so incremental scanning is **not wholly missing**. Its duplicate prevention is based on paths/identifiers; it does not provide a duplicate-recording cleanup interface. File deletion would also be an optional additional action\u2014the existing non-destructive **Remove** behavior is useful.

For **playlists and the queue**, the recent management work supplies the foundation, but editing remains limited. Audacious\u2019s [playlist actions](https://github.com/audacious-media-player/audacious-plugins/blob/audacious-plugins-4.6.2/src/qtui/menus.cc), Strawberry\u2019s [playlist implementation](https://github.com/strawberrymusicplayer/strawberry/tree/1.2.31/src/playlist) and [smart-playlist builder](https://github.com/strawberrymusicplayer/strawberry/tree/1.2.31/src/smartplaylists) support these comparisons.

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 24 | Multiple selection and bulk add/remove/edit operations | R, S, A |
| 25 | Drag files or tracks into and between collections | A, S |
| 26 | Manually reorder playlist and queue entries | S, A |
| 27 | **Play next** without replacing the existing queue | S |
| 28 | Cut, copy and paste selected tracks between playlists | A, R |
| 29 | **Partial:** export playlists, including relative/absolute path choices | S, A, R |
| 30 | **Partial:** additional playlist formats, notably ASX and WPL | S; A for ASX |
| 31 | Smart playlists defined by saved rules | R, S |
| 32 | Dynamic playlists that automatically replenish their selections | S |
| 33 | Organize playlists into folders and mark playlists as favorites | S |
| 34 | Keep multiple playlists open in tabs | S, A |
| 35 | Undo and redo playlist edits | S |
| 36 | Intentionally include the same track more than once | S, A |
| 37 | Batch sorting, reversal, duplicate removal and unavailable-entry cleanup | A, S |
| 38 | Export multiple playlists in one operation | S |

Open Audio Player currently suppresses repeated tracks when adding/importing them. That prevents accidental duplicates, but also prevents intentional repeated entries in a listening sequence.

For **playback and sound**, using GStreamer does not automatically provide the controls or transition logic offered by mature applications. These gaps are visible in Strawberry\u2019s [backend settings](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/settings/backendsettingspage.ui), [playback modes](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/playlist/playlistsequence.ui) and Audacious\u2019s [effect plugins](https://github.com/audacious-media-player/audacious-plugins/tree/audacious-plugins-4.6.2/src).

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 39 | Gapless transitions between tracks | R, S, A |
| 40 | Audio crossfading between tracks | R, S, A |
| 41 | Fade out/in when stopping, pausing or resuming | S |
| 42 | Equalizer and saved presets | S, A |
| 43 | ReplayGain track/album normalization, preamp and clipping controls | R, S, A |
| 44 | EBU R128 loudness analysis and normalization | S |
| 45 | Select the audio output backend and device inside the app | S, A |
| 46 | Explicit direct-DAC/bit-perfect output configuration | S |
| 47 | Configure resampling, channel conversion, balance and headphone crossfeed | A, S |
| 48 | Audio effects: compression, echo, stereo widening, voice removal, crystalizer, bitcrusher and LADSPA hosting | A |
| 49 | Playback-speed adjustment and independent pitch control | A |
| 50 | Automatic silence removal | A |
| 51 | A\u2013B looping of a section within a track | A |
| 52 | **Partial:** shuffle albums/groups and repeat an album | S; A for album shuffle |
| 53 | **Partial:** stop after this track, stop after every track and an explicit Stop UI | S, A |
| 54 | Previous/next album navigation; automatic short track previews | A for album navigation; S for previews |
| 55 | **Partial:** configurable seek steps, remaining-time display and jump-to-current-track action | S, A |
| 56 | **Partial:** optionally resume playback automatically at startup | S |
| 57 | Configure buffering and DAC warm-up delay | S |
| 58 | Automatically continue after an unavailable or unplayable track | S |

The current player stops its pipeline before loading the next track, so automatic next-track playback is not gapless playback. Conversely, the absence of bit-perfect controls does **not** prove that every current output is being resampled. Output behavior still depends on the installed backend and system configuration.

For **formats, discs and conversion**, the distinction is between what the backend might decode and what the application actually exposes. The relevant references are the [Audacious plugin inventory](https://github.com/audacious-media-player/audacious-plugins), Strawberry\u2019s [CUE parser](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/playlistparsers/cueparser.cpp) and [transcoder](https://github.com/strawberrymusicplayer/strawberry/tree/1.2.31/src/transcoder).

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 59 | CUE sheets that expose tracks within a single audio file | S, A |
| 60 | **Partial:** library recognition of additional formats such as WavPack, Musepack, Speex, TrueAudio, ASF and DSD | S; A for several |
| 61 | MIDI, tracker/module and game-console/chiptune playback workflows | A |
| 62 | Audio-CD playback, track browsing and disc metadata | R, S, A |
| 63 | Audio-CD ripping and burning | R |
| 64 | User-facing audio conversion/transcoding and file output | S, A |
| 65 | Record an internet audio stream | A |

Item 60 concerns the scanner\u2019s extension allowlist; some excluded formats may still be playable through another entry point with suitable GStreamer plugins. CD burning in Rhythmbox depends on its Brasero integration.

For **artwork, lyrics and visualization**, Open Audio Player already handles local material. The missing functionality is predominantly retrieval, editing and richer presentation. See Strawberry\u2019s [cover manager](https://github.com/strawberrymusicplayer/strawberry/tree/1.2.31/src/covermanager), [lyrics controls](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/settings/lyricssettingspage.ui) and [waveform implementation](https://github.com/strawberrymusicplayer/strawberry/tree/1.2.31/src/waveform).

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 66 | **Partial:** search online for album covers and retrieve missing artwork | S, R |
| 67 | Cover manager: replace, embed, save and batch-export artwork | S |
| 68 | **Partial:** retrieve lyrics online and edit/save lyrics | S, R, A |
| 69 | **Partial:** configurable cover-file discovery and artwork-cache management | S |
| 70 | **Partial:** spectrum analyzers, VU meters and alternative visualizations | A, S |
| 71 | **Partial:** whole-track waveform seek bar and moodbar | S |
| 72 | Artist biography and contextual information panel | R |

The existing live waveform is different from a precomputed, navigable representation of the entire song. Online artist-information plugins also depend on their external data sources remaining available.

For **podcasts**, Rhythmbox supplies the relevant comparison: Strawberry explicitly excludes podcast support. Rhythmbox\u2019s [current release notes](https://gnome.pages.gitlab.gnome.org/rhythmbox/) document its recent podcast improvements, while its [download/subscription implementation](/tmp/open-audio-player-research-2026-11-06/rhythmbox/podcast/rb-podcast-manager.c) supplies the detailed evidence.

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 73 | Download episodes for offline listening | R |
| 74 | Scheduled feed refresh and automatic new-episode downloads | R |
| 75 | Download progress, cancellation, download-location settings and downloaded-file management | R |
| 76 | **Partial:** browse subscribed episodes separately by show | R |
| 77 | **Partial:** show descriptions, episode notes and structured publication-date/details views | R |
| 78 | **Partial:** retain played/completed state and display played-episode indicators | R |
| 79 | Import multiple subscriptions through OPML | R |
| 80 | Synchronize subscriptions and episode activity through a gPodder account | R |

Currently, finishing an episode removes its saved resume position; it does not retain a separate \u201cplayed\u201d flag. Also, public gPodder discovery and authenticated gPodder synchronization are separate capabilities.

For **services, networking and devices**, the gaps concern integrated libraries and workflows, rather than simply opening an audio URL. Strawberry provides [server integrations](https://github.com/strawberrymusicplayer/strawberry/tree/1.2.31/src/settings); Rhythmbox provides [device, sharing and remote-control plugins](https://github.com/GNOME/rhythmbox/tree/master/plugins).

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 81 | Last.fm/ListenBrainz scrobbling, including offline submission caching | S, R, A |
| 82 | Browse Subsonic-compatible libraries, including Navidrome; browse Ampache servers | S for Subsonic; A for Ampache |
| 83 | Plex and Jellyfin authentication and library browsing | S |
| 84 | **Partial:** integrated Spotify, Tidal or Qobuz playback and library access | S |
| 85 | Browse and share music libraries over DAAP on the local network | R |
| 86 | Transfer/synchronize music with USB players, MTP/Android devices and supported iPods | R, S |
| 87 | **Partial:** browser/phone remote control with network audio streaming | R |
| 88 | **Partial:** additional built-in radio catalogues and richer station browsing | S, A |
| 89 | Update the displayed artist/title from live stream metadata | R, S, A |
| 90 | **Partial:** network-share/FTP URL workflows and application proxy settings | A for transports; S for proxy settings |

Important qualifications: Strawberry\u2019s commercial-service integrations are **unofficial**, can require paid accounts and optional components, and are not guarantees of continued service access. Mounted network folders already work as local folders in Open Audio Player. Its existing MPRIS support is also real remote control within the desktop session; item 87 concerns a network-accessible remote.

For **desktop behavior and customization**, the missing capabilities are illustrated by Strawberry\u2019s [notification settings](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/settings/notificationssettingspage.ui), [behavior settings](https://github.com/strawberrymusicplayer/strawberry/blob/1.2.31/src/settings/behavioursettingspage.ui), Audacious\u2019s [automation plugin](https://github.com/audacious-media-player/audacious-plugins/blob/audacious-plugins-4.6.2/src/songchange/song_change.cc) and its [recent playback-history additions](https://audacious-media-player.org/news/64-audacious-4-6-released).

| # | Missing or incomplete capability | Examples |
|---|---|---|
| 91 | Native desktop track-change notifications and configurable on-screen displays | R, S, A |
| 92 | **Partial:** editable shortcuts and global bindings for additional actions | S, A |
| 93 | **Partial:** configurable double-click/add-track behavior: append, replace, queue or new playlist | S |
| 94 | **Partial:** file-manager \u201cOpen With\u201d integration and file/URL command-line arguments | A, S |
| 95 | **Partial:** dedicated command-line playlist querying, editing and control | A, R |
| 96 | Request that the desktop prevent automatic suspend during playback | R |
| 97 | **Partial:** light/system themes and configurable colors, fonts and appearance | S, A |
| 98 | Alternative skins and rearrangeable/dockable panels | A |
| 99 | Application plugin manager and extension API | R, A |
| 100 | Run configured actions on track start/end, stream-title changes or playlist completion | A |
| 101 | Discord listening presence | S |
| 102 | Playback-history browser and diagnostic log viewer | A |

Global shortcuts depend on the desktop and X11/Wayland support. GStreamer\u2019s decoder plugins also do not constitute an application plugin system.

For a future implementation roadmap, I would prioritize these groups:

1. **Daily usability:** multi-selection, drag-and-drop, playlist/queue reordering, Play next, export, configurable sorting and file-manager opening.
2. **Collection correctness:** album artist and disc metadata, compilation handling, tag editing, folder watching and moved-file tracking.
3. **Playback quality:** gapless transitions, ReplayGain, equalizer, output-device selection and recovery from unplayable tracks.
4. **Podcasts:** offline downloads, scheduled refresh, show-level browsing, completion indicators and OPML import.
5. **Optional expansion:** scrobbling, server libraries, device synchronization, advanced effects and customization.

The inventory covers released functionality and bundled optional plugins at the workflow level. Features such as timed lyric highlighting, sleep timers, podcast chapters/transcripts and Chromecast were not included in the 102 verified comparison gaps. No application implementation changes were made.
