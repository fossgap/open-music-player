# Data privacy

[User guide](README.md) · [Service requirements](services.md) · [Account setup](streaming.md)

This page describes the requests made by the code in this revision. Open Music
Player has no project-operated account server, analytics, advertising SDK, crash
upload, or automatic application-update check. It does contact external services
for the features below, including **metadata lookups enabled by default**.
Anonymous access means no sign-in is required; it does not make a request anonymous
to the recipient.

## Defaults and automatic requests

| Setting or action | Default and effect |
| --- | --- |
| **Find missing covers automatically** | **On.** The current music track and visible tracks/albums/playlists can trigger MusicBrainz and Cover Art Archive requests when artwork is missing. |
| **Find missing lyrics automatically** | **On.** The current local, Spotify, or YouTube Music track can trigger LRCLIB requests, including when a saved current track is restored without playback. |
| **Find artist and album information automatically** | **On.** The current music track can trigger MusicBrainz, Wikidata, and English Wikipedia requests, including after restoring a session. |
| **Automatic refresh (minutes)** | **0 (off).** A positive interval refreshes folders, subscribed feeds, connected playlists, and the open discovery/search view. Playlist refresh uses **two-way sync** and can upload eligible local edits. |
| **Watch music folders for changes** | **On.** Watches and scans locally; scanning can expose new tracks to the enabled metadata lookups. |
| Saved Spotify connection | Reconnects at startup in the background. Disabling automatic refresh does not disable this reconnection. |
| **Keep running when the window closes** | **On.** Hiding the window does not stop playback, transfers, or enabled background work. Use **Quit** to exit. |

An empty first launch does not search online directories or fetch feeds. Local
playback needs no network, but **a library of local files is not automatically
an offline-only session** while metadata lookups are on. Cache hits can avoid a
request. Turning a lookup off cancels its pending work and retains downloaded
content; it cannot recall a request already sent.

## What recipients receive

Every contacted host can see your public IP address, request timing, requested
resource, and protocol/client headers. The app's Rust HTTP clients generally
identify `OpenMusicPlayer/<version>`; metadata/download requests also identify
the project URL. GStreamer, FFmpeg, ytmusicapi, yt-dlp, and librespot use their own
protocols or client headers. DNS resolvers and network operators may see hostnames
or connection metadata. Plain HTTP feeds/streams do not encrypt their contents.
Redirects and media/image CDNs can introduce additional recipients.

| Service / usual hosts | When contacted | Information sent |
| --- | --- | --- |
| **Apple Podcasts / iTunes** — `itunes.apple.com` | Podcast discovery/search; global **Search online**; scheduled refresh of an open podcast search | Search text, podcast media/entity filters, result limit, and fixed `country=US`. Empty discovery uses the word `podcast`. The country value is not device geolocation. No Apple sign-in or local subscription-list upload. |
| **gPodder directory** — `gpodder.net` | Selected podcast discovery/search, global online search, or scheduled discovery refresh | Search text and image-size parameter; empty discovery asks for the public top 50 list. No gPodder account or subscription synchronization. |
| **Radio Browser** — `de1.api.radio-browser.info`, then `nl1.api.radio-browser.info`, then `at1.api.radio-browser.info` on failures | Radio discovery/search, global online search, or scheduled refresh of the radio view | Station-name query, limit, hide-broken filter, and click-count sorting. A failed request can send the same query to another mirror. The app does not call Radio Browser's station-click reporting endpoint. |
| **Podcast publishers / feed hosts** — URLs you add or select | Previewing a show's episodes, subscribing, manual refresh, or enabled automatic refresh | Feed URL, including any query-string token supplied in it. Hosts can infer interest in or subscription to that feed. Playback/download additionally contacts the episode's media host and any redirects. Unsubscribing stops future subscription refreshes, while indexed episodes remain locally. |
| **Radio stations, direct audio hosts, and media CDNs** | Playing, downloading, recording, seeking, or preparing the next online track | Selected stream/episode/media URL and its query parameters; byte-range requests and validators for applicable seeks/resumes; connection duration and request timing. HLS/DASH can fetch manifests, segments, and their referenced resources. |
| **Artwork hosts** — URLs returned by directories, feeds, music services, and metadata providers | Loading uncached service artwork for results or saved tracks | The image URL (possibly including tokens) and ordinary request headers. Merely displaying a result can fetch its image. These requests are separate from missing-cover lookups; turning off MusicBrainz cover lookup does not suppress service images. |
| **MusicBrainz** — `musicbrainz.org/ws/2` | Automatic or retried cover/information lookup; loading further discography pages | Available embedded MusicBrainz recording/release/artist identifiers; otherwise artist/album-artist, album, and, for recording lookup, track title. Entity IDs and paging parameters fetch related releases and discographies. Duration helps match candidates locally. No audio bytes, acoustic fingerprint, folder path, or full-library upload. |
| **Cover Art Archive** — `coverartarchive.org`, with returned image hosts/redirects such as Internet Archive | Finding a missing album cover | Matched MusicBrainz release or release-group ID, then the chosen cover image URL. No audio upload. |
| **LRCLIB** — `lrclib.net/api` | Automatic or retried lyrics lookup for the current music track | Track title, artist, album when available, and duration when known. The fallback search sends title and artist. No audio upload or lyrics contribution/upload. |
| **Wikidata** — `www.wikidata.org/w/api.php` | Artist/album information when MusicBrainz supplies a Wikidata relationship without a direct English Wikipedia link | Wikidata entity ID and a request for its English Wikipedia sitelink. |
| **English Wikipedia** — `en.wikipedia.org/w/api.php` | Retrieving an artist biography or album summary through a MusicBrainz/Wikidata relationship | Article title and parameters for its introductory extract, page properties, and URL. The article title can identify what you are listening to. |
| **YouTube Music / Google / YouTube media and image hosts** — typically `music.youtube.com`, `youtube.com`, `googlevideo.com`, `ytimg.com` and returned CDNs | Song search, album lookup, account connection, playlist sync/publishing, stream resolution, playback/download, and eligible next-track preparation | Search terms, artist/album names for album lookup, video/album/playlist IDs, requested media URLs, and client/runtime requests made by ytmusicapi/yt-dlp. With a connected account, YouTube session cookies and derived authorization identify that account. Playlist writes send playlist names, video IDs, additions/removals, and order. New playlists are created private. |
| **Spotify** — `accounts.spotify.com`, `api.spotify.com`, Spotify playback infrastructure and returned CDNs | Browser sign-in, startup reconnection/token refresh, search/album lookup, sync/publishing, playback and preloading | OAuth client ID, scopes, redirect URI, state and PKCE challenge; authorization code/verifier or refresh token during token exchange; bearer access token for Web API requests; search text, artist/album queries, account/playlist/track IDs, playlist names and ordered track changes. Librespot sends authenticated playback/session/device protocol data and fetches audio. Spotify can associate use with your account. |
| **Python package index and file hosts** — normally `pypi.org` and `files.pythonhosted.org` | Only when you run `open-music-setup-youtube` (or its source script) to install/update optional dependencies | Requested package names/versions and pip's interpreter/platform/client headers. A configured pip index or mirror replaces these hosts. The helper installs ytmusicapi, yt-dlp, SecretStorage, and dependencies into a private environment. |

Global search is local while typing. **Enter / Search online** sends the query
to Apple, gPodder, Radio Browser, YouTube Music, and a connected Spotify account.
Choosing **Play** or **Add album to queue** in the discography first looks for
local matches, then can search the selected/current streaming service and fall
back to the other available service. This sends artist and album names even if
the track you started with is a local file.

Provider URLs can be overridden with the `OPEN_MUSIC_*_API` environment variables
listed in the [development guide](../development.md). An override changes the
recipient, not the information required for that request. The table lists normal
defaults, not a fixed network allowlist. Opening a source/article/service link
launches your browser, whose cookies and privacy settings then apply.

## Accounts, playlist changes, and credentials

**YouTube Music:** account connection reads the browser profile or cookie file
you select. Only unexpired YouTube-domain cookies are retained in the desktop
Secret Service keyring. Google passwords are not entered into the app. Browser
sign-in selects the first signed-in account in that profile. The helper gives
the player a short-lived media URL and allowlisted `User-Agent`, `Referer`, and
`Origin` headers; it does not pass session cookies or authorization headers to
arbitrary media hosts. Media URLs can themselves contain access tokens. Exported
cookie files are left where you put them; protect or delete them after import.

**Spotify:** browser authorization uses PKCE and a loopback callback at
`http://127.0.0.1:5588/login`. The requested scopes are `streaming`,
`user-read-private`, `playlist-read-private`, `playlist-read-collaborative`,
`playlist-modify-private`, and `playlist-modify-public`. Existing owned playlists
can therefore be updated regardless of visibility; newly published playlists are
private. Refresh tokens, client ID, and account ID are stored in the desktop
keyring. Access tokens are used in memory. No Spotify password is entered into
the player.

Sync is two-way: renames and eligible track/order changes made locally may be
uploaded during manual sync or enabled automatic refresh. Conflicts are reported
and preserved, but an interrupted multi-request upload can leave a partial remote
update. Removing a linked playlist locally does not delete it online.

**Account → Disconnect** removes the saved session and that provider's local
tracks and linked playlists. It does not delete the remote account/playlists or
erase provider logs. Local disconnection is not a promise of server-side token
revocation; use the provider's account/security controls to revoke access or end
browser sessions as needed. Already downloaded files and cached content may
remain on disk.

## Choose a more private setup

Before importing music, open Settings and turn off:

- **Online artwork and lyrics → Find missing covers automatically**.
- **Online artwork and lyrics → Find missing lyrics automatically**.
- **Artist and album information → Find artist and album information automatically**.

Keep **Automatic refresh (minutes)** at **0**, disconnect online accounts, and use
local files/playlists with local artwork and lyrics. Avoid online discovery,
remote feed/stream URLs, and streaming discography actions. Saved online items
can still refer to remote images or media. There is no single offline-mode switch;
for a firm network boundary, use OS-level network isolation. These choices do not
affect local scanning, tag editing, visualizations, or local playback.

## Local storage and removal

The default data directory is `$XDG_DATA_HOME/open-music-player`, or
`~/.local/share/open-music-player` when XDG_DATA_HOME is unset or relative.
`OPEN_MUSIC_DATA_DIR` selects a different directory.

| Location | Contents |
| --- | --- |
| `library.json` | Local file paths, metadata, folders, subscriptions/feed URLs, online track URLs and identifiers, account labels, playlist links and sync state, favorites, queue/current track, listening positions, download history, and preferences. Feed/stream URLs may contain private query tokens. Credentials and expiring YouTube playback URLs are not stored here. |
| `artwork/` | Cached local/service artwork. |
| `metadata/` | Retrieved covers, lyrics, artist/album information, and lookup/entity cache records. Information responses generally expire after 30 days; misses after seven days. Downloaded content can remain after disabling lookup. |
| `youtube-python/` | Optional Python environment created by the setup helper. |
| Desktop keyring | YouTube cookies/session and Spotify refresh credentials, separated by application data-directory namespace. Backing up `library.json` does not back up usable account sessions. |
| Download folder | Completed downloads and recordings; default `~/Downloads/Open Music Player`, or the folder chosen in Settings. |

The library and caches are ordinary local files, not encrypted by the application.
Your OS account permissions and disk encryption protect them. **Library only**
tag edits stay in the library; choosing **Audio files** writes tags into your
files. **Remove from library** keeps audio on disk; **Move file to Trash** and
**Delete download** have separate file-removal effects described in the
[listening](listening.md) and [download](playback.md) guides.

To reset local data, disconnect accounts first, quit the app, then back up or
remove the data directory. Downloaded audio and exported cookie files are stored
separately and need separate cleanup. Removing local data does not remove data
held by third parties.

MPRIS publishes the current track's metadata, file/stream URI, artwork reference, playback state,
and controls on your local desktop session bus. Desktop extensions and other
programs in that session can access it. Folder monitoring, notifications/tray,
FFmpeg metadata extraction, and keyring access are local operations, not FOSS Gap
telemetry. Bug reports are manual: redact account labels, paths, feed tokens,
cookies, and private playlist information before sharing logs or `library.json`.

## Scope and source references

This is an implementation inventory, not a statement about providers' retention,
profiling, or deletion policies. Consult the service's own privacy information
when deciding whether to use it. The player cannot guarantee what a remote host
logs, how long it retains requests, or which CDN it returns.

Provider privacy information: [Apple](https://www.apple.com/legal/privacy/en-ww/),
[MetaBrainz / MusicBrainz](https://metabrainz.org/privacy),
[Google / YouTube](https://policies.google.com/privacy),
[Spotify](https://www.spotify.com/legal/privacy-policy/),
[Wikimedia (Wikipedia and Wikidata)](https://foundation.wikimedia.org/wiki/Policy:Privacy_policy),
and [PyPI](https://policies.python.org/pypi.org/Privacy-Notice/).
Podcast publishers, radio stations, and image/media hosts each have their own
policies; consult the host serving the URL, not only the discovery directory.

Implementation: [defaults](../../src/model.rs), [directories/feeds/artwork](../../src/services.rs),
[metadata providers](../../src/enrichment/providers.rs),
[artist information](../../src/enrichment/providers/information.rs),
[automatic refresh](../../src/app/updates.rs),
[Spotify authorization](../../src/spotify/auth.rs),
[Spotify catalog](../../src/spotify/catalog.rs),
[YouTube helper](../../resources/youtube_music.py),
[downloads](../../src/downloads/http.rs), and
[optional setup](../../scripts/setup-youtube-music.sh).
