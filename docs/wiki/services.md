# Online services and account requirements

[User guide](README.md) · [Installation](installation.md) · [Data privacy](data-privacy.md)

These requirements apply to the features implemented in Open Music Player. Anonymous access means no user account, API key, or saved sign-in is needed.

| Provider | Anonymous access | Account requirements |
| --- | --- | --- |
| [Apple Podcasts / iTunes](https://performance-partners.apple.com/search-api) | Yes: podcast search and discovery | None |
| [gPodder directory](https://gpoddernet.readthedocs.io/en/latest/api/reference/directory.html) | Yes: podcast search and top lists | None; subscriptions are stored locally |
| [Radio Browser](https://api.radio-browser.info/) | Yes: station search and discovery | None |
| [MusicBrainz](https://musicbrainz.org/doc/MusicBrainz_API) / [Cover Art Archive](https://musicbrainz.org/doc/Cover_Art_Archive/API) | Yes: metadata lookups and cover retrieval | None |
| [LRCLIB](https://lrclib.net/docs) | Yes: lyrics retrieval | None; developer registration is optional |
| [YouTube Music](https://ytmusicapi.readthedocs.io/en/stable/usage.html) | Partial: public search and eligible public-track playback | Personal playlist synchronization and publishing require a signed-in Google account |
| [Spotify](https://developer.spotify.com/documentation/web-api/concepts/authorization) | No: the integration requires sign-in | [Premium is required for librespot playback](https://github.com/librespot-org/librespot) |
| Direct RSS/Atom feeds and HTTP(S) audio streams | Yes, when the host serves them publicly | Host-dependent; authenticated feed login is not implemented |

YouTube playback uses [yt-dlp](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies); restricted content or service checks can require sign-in. Public podcast directory access does not unlock a publisher's paid or private episodes.

MusicBrainz, Wikidata, and English Wikipedia also provide artist and album information and discographies. These lookups need no account or API key. See [listening](listening.md) for the feature and [data privacy](data-privacy.md) for request contents, defaults, and controls.

Provider availability and account restrictions can change independently of Open Music Player. Directory access does not grant access to paid feeds or restricted recordings.
