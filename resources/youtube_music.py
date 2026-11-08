"""Private JSON/stdio bridge for ytmusicapi and yt-dlp. No passwords or shell commands.

The Rust executable embeds this file. Only account labels and catalog metadata leave
the helper; the YouTube-only cookie jar is held in Secret Service and memory.
"""

import contextlib
import http.cookiejar
import io
import json
import math
import re
import shutil
import subprocess
import sys
from urllib.parse import urlparse

MAX_TRACKS = 5000
MAX_PLAYLISTS = 250
VIDEO_ID = re.compile(r"^[A-Za-z0-9_-]{11}$")
PLAYLIST_ID = re.compile(r"^[A-Za-z0-9_-]{2,200}$")
BROWSERS = {"firefox", "chrome", "chromium", "brave", "edge", "opera", "vivaldi"}


class BridgeError(Exception):
    """A safe, actionable message; never include provider responses or credentials."""


class QuietLog:
    def debug(self, *_args, **_kwargs):
        pass

    info = warning = error = debug


def keyring(action, namespace, value=None):
    if not re.fullmatch(r"t[0-9a-f]{16}", namespace):
        raise BridgeError("Invalid credential storage identifier.")
    args = ["secret-tool", action]
    if action == "store":
        args += ["--label=Open Music Player — YouTube Music"]
    args += ["application", "open-music-player", "service", "youtube-music", "profile", namespace]
    try:
        result = subprocess.run(
            args, input=value, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            timeout=45, check=False,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        raise BridgeError("Unlock your desktop keyring and install libsecret-tools, then try again.") from None
    if result.returncode != 0:
        raise BridgeError("Could not access the desktop keyring. Unlock it, then reconnect your account.")
    if len(result.stdout) > 1_000_000:
        raise BridgeError("The saved browser session is too large. Reconnect your account.")
    return result.stdout


def youtube_cookies(jar):
    # Never retain Google-wide or unrelated browser cookies.
    return [
        {
            "name": c.name, "value": c.value, "domain": c.domain,
            "path": c.path, "secure": c.secure, "expires": c.expires,
        }
        for c in jar
        if c.domain.lstrip(".") in {"youtube.com", "www.youtube.com", "music.youtube.com"}
        and not c.is_expired()
    ]


def cookie_jar(cookies):
    jar = http.cookiejar.CookieJar()
    for c in cookies:
        if c["domain"].lstrip(".") not in {"youtube.com", "www.youtube.com", "music.youtube.com"}:
            continue
        jar.set_cookie(http.cookiejar.Cookie(
            version=0, name=c["name"], value=c["value"], port=None, port_specified=False,
            domain=c["domain"], domain_specified=True, domain_initial_dot=c["domain"].startswith("."),
            path=c["path"], path_specified=True, secure=c["secure"], expires=c["expires"],
            discard=c["expires"] is None, comment=None, comment_url=None, rest={}, rfc2109=False,
        ))
    return jar


def music_client(cookies=None):
    from ytmusicapi import YTMusic
    from ytmusicapi.helpers import initialize_headers
    if cookies is None:
        return YTMusic()
    values = {c.name: c.value for c in cookie_jar(cookies) if not c.is_expired()}
    if not values.get("__Secure-3PAPISID"):
        raise BridgeError("No signed-in YouTube Music session found. Sign in in your browser, then reconnect.")
    # ytmusicapi recomputes the SAPISID authorization header for every request.
    headers = initialize_headers()
    headers.update({
        "cookie": "; ".join(f"{name}={value}" for name, value in values.items()),
        "authorization": "SAPISIDHASH",
        "x-goog-authuser": "0",
        "x-origin": "https://music.youtube.com",
        "origin": "https://music.youtube.com",
    })
    return YTMusic(headers)


def account_info(client):
    info = client.get_account_info()
    name = info.get("accountName")
    handle = info.get("channelHandle")
    if not name or not handle:
        raise BridgeError("Open YouTube Music in your browser and finish setting up your profile, then reconnect.")
    return {"id": handle, "name": name}


def session(request):
    try:
        data = json.loads(keyring("lookup", request["namespace"]))
    except (ValueError, KeyError):
        raise BridgeError("Your saved session is unavailable. Reconnect YouTube Music.") from None
    if data["account"]["id"] != request.get("account_id"):
        raise BridgeError("The connected account changed. Reconnect YouTube Music.")
    return data


def connect(request):
    from yt_dlp.cookies import YoutubeDLCookieJar, extract_cookies_from_browser
    if request.get("cookie_file"):
        jar = YoutubeDLCookieJar()
        try:
            jar.load(request["cookie_file"], ignore_discard=True, ignore_expires=False)
        except Exception:
            raise BridgeError("Could not read the cookie file. Choose a Netscape-format YouTube cookie file.") from None
    else:
        browser = request.get("browser")
        if browser not in BROWSERS:
            raise BridgeError("Choose a supported browser.")
        try:
            jar = extract_cookies_from_browser(browser, request.get("profile") or None, QuietLog())
        except Exception:
            raise BridgeError("Could not read this browser profile. Close the browser and retry, or import a cookie file.") from None
    cookies = youtube_cookies(jar)
    client = music_client(cookies)
    account = account_info(client)
    # Validate private library access before storing anything.
    client.get_library_playlists(limit=1)
    payload = json.dumps({"account": account, "cookies": cookies}).encode()
    keyring("store", request["namespace"], payload)
    return account


def snapshot(name, ids):
    value = "\0".join([name, *ids]).encode()
    number = 0xCBF29CE484222325
    for byte in value:
        number = ((number ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"t{number:016x}"


def thumbnail(item):
    for image in reversed(item.get("thumbnails") or []):
        uri = image.get("url", "")
        if uri.startswith("https://"):
            return uri
    return None


def track(item):
    video = item.get("videoId", "")
    if not VIDEO_ID.fullmatch(video or "") or item.get("isAvailable") is False:
        return None
    duration = item.get("duration_seconds")
    if duration is None:
        try:
            duration = 0
            for component in (item.get("duration") or "0").split(":"):
                duration = duration * 60 + int(component)
        except ValueError:
            duration = 0
    try:
        duration = float(duration)
    except (TypeError, ValueError):
        duration = 0
    return {
        "video_id": video, "title": item.get("title") or "YouTube Music",
        "artist": ", ".join(a["name"] for a in (item.get("artists") or []) if a.get("name")),
        "album": (item.get("album") or {}).get("name", ""),
        "duration": max(0, duration) if math.isfinite(duration) else 0, "artwork": thumbnail(item),
    }


def album_tracks(client, request):
    wanted = request["album"]
    artist, title = wanted["artist"].strip(), wanted["title"].strip()
    if not artist or not title or len(artist) + len(title) > 500:
        raise BridgeError("Album lookup needs an artist and album name.")

    def normalize(value):
        return "".join(c for c in value.lower() if c.isalnum())

    def matches(item):
        return normalize(item.get("title", "")) == normalize(title) and any(
            normalize(a.get("name", "")) == normalize(artist) for a in item.get("artists") or []
        )

    candidates = [item for item in client.search(f"{artist} {title}", filter="albums", limit=20, ignore_spelling=True)
                  if matches(item) and re.fullmatch(r"[A-Za-z0-9_-]{2,200}", item.get("browseId", ""))]
    year = wanted.get("date", "")[:4]
    candidates.sort(key=lambda item: not (year and str(item.get("year", "")).startswith(year)))
    for candidate in candidates[:3]:
        data = client.get_album(candidate["browseId"])
        if not matches(data):
            continue
        entries = data.get("tracks") or []
        if len(entries) > 1000 or (data.get("trackCount") or 0) > len(entries):
            raise BridgeError("YouTube Music returned an incomplete album. Try again.")
        tracks = []
        for index, entry in enumerate(entries, 1):
            item = dict(entry)
            item["album"] = {"name": data["title"]}
            item["artists"] = item.get("artists") or data.get("artists")
            item["thumbnails"] = item.get("thumbnails") or data.get("thumbnails")
            value = track(item)
            if value:
                value["number"] = index
                value["album_artist"] = ", ".join(a["name"] for a in data.get("artists") or [] if a.get("name"))
                tracks.append(value)
        if tracks:
            return tracks
    return []


def remote_playlist(client, playlist_id):
    if not PLAYLIST_ID.fullmatch(playlist_id):
        raise BridgeError("Invalid YouTube Music playlist.")
    data = client.get_playlist(playlist_id, limit=MAX_TRACKS + 1)
    entries = data.get("tracks", [])
    if len(entries) > MAX_TRACKS or (data.get("trackCount") or 0) > len(entries):
        raise BridgeError("This playlist is too large or incomplete. It was left unchanged.")
    tracks = [value for item in entries if (value := track(item)) is not None]
    ids = [item.get("videoId") or f"unavailable:{index}" for index, item in enumerate(entries)]
    name = data.get("title") or "YouTube Music"
    return {
        "id": playlist_id, "name": name, "tracks": tracks,
        "editable": bool(data.get("owned", False)) and len(tracks) == len(entries),
        "snapshot": snapshot(name, ids), "unavailable": len(entries) - len(tracks),
        "entries": entries,
    }


def checked(result):
    if result is True or result == "STATUS_SUCCEEDED":
        return
    if isinstance(result, dict) and result.get("status") == "STATUS_SUCCEEDED":
        return
    raise BridgeError("YouTube Music did not confirm the playlist change. Refresh before trying again.")


def upload_changes(client, remote, local):
    # Add the requested sequence before removing old entries: a failed addition
    # never empties the user's playlist. Retain setVideoId to distinguish duplicates.
    if not local.get("valid") or len(local["video_ids"]) > MAX_TRACKS or not all(VIDEO_ID.fullmatch(v) for v in local["video_ids"]):
        raise BridgeError("Only playlists containing YouTube Music tracks can be synchronized.")
    if not local["name"].strip() or len(local["name"]) > 100:
        raise BridgeError("Enter a playlist name with 100 characters or fewer.")
    fresh = remote_playlist(client, remote["id"])
    if fresh["snapshot"] != remote["snapshot"] or not fresh["editable"]:
        raise BridgeError("The online playlist changed during synchronization. It was left unchanged.")
    entries = fresh["entries"]
    ids = local["video_ids"]
    if ids != [t.get("videoId") for t in entries]:
        if any(not t.get("setVideoId") for t in entries):
            raise BridgeError("This playlist cannot be edited safely. It was left unchanged.")
        if ids:
            checked(client.add_playlist_items(remote["id"], ids, duplicates=True))
        if entries:
            checked(client.remove_playlist_items(remote["id"], entries))
    if remote["name"] != local["name"]:
        checked(client.edit_playlist(remote["id"], title=local["name"]))
    updated = remote_playlist(client, remote["id"])
    if updated["snapshot"] != snapshot(local["name"], ids):
        raise BridgeError("The playlist changed during synchronization. Refresh before trying again.")
    return updated


def synchronize(client, request):
    if account_info(client)["id"] != request["account_id"]:
        raise BridgeError("Your browser account changed. Reconnect YouTube Music before syncing.")
    summaries = client.get_library_playlists(limit=MAX_PLAYLISTS + 1)
    if len(summaries) > MAX_PLAYLISTS:
        raise BridgeError("Too many playlists to synchronize at once. No playlists were changed.")
    local_by_remote = {p["link"]["id"]: p for p in request.get("playlists", [])}
    ignored = set(request.get("ignored", []))
    ids = list(dict.fromkeys(s["playlistId"] for s in summaries if s.get("playlistId") not in {None, "LM", "SE"}))
    result = {"playlists": [], "conflicts": [], "failed": [], "uploaded": 0}
    for playlist_id in ids:
        if playlist_id in ignored:
            continue
        local = local_by_remote.get(playlist_id)
        name = local["name"] if local else playlist_id
        try:
            remote = remote_playlist(client, playlist_id)
            name = remote["name"]
            if local:
                link = local["link"]
                local_changed = local["snapshot"] != link["local_snapshot"]
                remote_changed = remote["snapshot"] != link["remote_snapshot"]
                same = local["valid"] and remote["snapshot"] == snapshot(local["name"], local["video_ids"])
                if local_changed and not same:
                    if remote_changed or not remote["editable"] or not local["valid"]:
                        result["conflicts"].append(name)
                        continue
                    remote = upload_changes(client, remote, local)
                    result["uploaded"] += 1
                remote["local_id"] = local["local_id"]
            remote.pop("entries")
            result["playlists"].append(remote)
        except Exception:
            # One failed playlist must not discard successful results. Do not leak
            # cookies/headers or private server responses in an error message.
            result["failed"].append(name)
    return result


def resolve(cookies, video_id):
    from yt_dlp import YoutubeDL
    if not VIDEO_ID.fullmatch(video_id):
        raise BridgeError("Invalid YouTube Music track.")
    runtime = next((name for name in ("deno", "node") if shutil.which(name)), None)
    if runtime is None:
        raise BridgeError("Install Deno 2.3+ or Node.js 22+ for YouTube Music playback, then try again.")
    options = {
        "format": "bestaudio[protocol=https]/bestaudio[protocol=http]/bestaudio",
        "noplaylist": True, "skip_download": True, "quiet": True,
        "no_warnings": True, "logger": QuietLog(), "socket_timeout": 20,
        "retries": 1, "extractor_retries": 1, "cachedir": False,
        "js_runtimes": {runtime: {}},
    }
    with YoutubeDL(options) as downloader:
        if cookies:
            for cookie in cookie_jar(cookies):
                downloader.cookiejar.set_cookie(cookie)
        info = downloader.extract_info(f"https://music.youtube.com/watch?v={video_id}", download=False)
        uri = info.get("url", "")
        parsed = urlparse(uri)
        if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username:
            raise BridgeError("YouTube Music did not return a playable audio stream.")
        # Only media request headers leave the helper; session cookies and
        # authorization headers must never be passed to an arbitrary media host.
        headers = {
            k: v for k, v in info.get("http_headers", {}).items()
            if k.lower() in {"user-agent", "referer", "origin"}
            and isinstance(v, str) and "\n" not in v and "\r" not in v
        }
        return {"uri": uri, "headers": headers}


def dispatch(request):
    operation = request.get("op")
    if operation == "disconnect":
        keyring("clear", request["namespace"])
        return {}
    if operation == "connect":
        return connect(request)
    data = session(request) if request.get("account_id") else None
    cookies = data["cookies"] if data else None
    if operation == "resolve":
        return resolve(cookies, request["video_id"])
    client = music_client(cookies)
    if operation == "album":
        return album_tracks(client, request)
    if operation == "search":
        query = request.get("query", "").strip()
        if not query or len(query) > 500:
            raise BridgeError("Enter a music search of 500 characters or fewer.")
        results = client.search(query, filter="songs", limit=50)
        seen = set()
        tracks = []
        for item in results:
            value = track(item)
            if value and value["video_id"] not in seen:
                seen.add(value["video_id"])
                tracks.append(value)
        return tracks[:50]
    if not data:
        raise BridgeError("Connect your YouTube Music account first.")
    if operation == "sync":
        return synchronize(client, request)
    if operation == "pull":
        if account_info(client)["id"] != request["account_id"]:
            raise BridgeError("Your browser account changed. Reconnect YouTube Music.")
        remote = remote_playlist(client, request["playlist_id"])
        remote.pop("entries")
        remote["local_id"] = request["local_id"]
        return remote
    if operation == "publish":
        if account_info(client)["id"] != request["account_id"]:
            raise BridgeError("Your browser account changed. Reconnect YouTube Music.")
        ids = request["video_ids"]
        if not ids or len(ids) > MAX_TRACKS or not all(VIDEO_ID.fullmatch(v) for v in ids):
            raise BridgeError("Only playlists containing YouTube Music tracks can be published.")
        name = request["name"].strip()
        if not name or len(name) > 100:
            raise BridgeError("Enter a playlist name with 100 characters or fewer.")
        # A new private playlist never modifies a pre-existing remote collection.
        playlist_id = client.create_playlist(name, "", privacy_status="PRIVATE")
        if not isinstance(playlist_id, str) or not PLAYLIST_ID.fullmatch(playlist_id):
            raise BridgeError("YouTube Music did not confirm playlist creation. Check your online library before retrying.")
        try:
            checked(client.add_playlist_items(playlist_id, ids, duplicates=True))
            remote = remote_playlist(client, playlist_id)
            if remote["snapshot"] != snapshot(name, ids):
                raise BridgeError("Incomplete playlist")
        except Exception:
            raise BridgeError("A private playlist was created, but its tracks could not be confirmed. Sync your online library before retrying.") from None
        remote.pop("entries")
        remote["local_id"] = request["local_id"]
        return remote
    raise BridgeError("Unknown YouTube Music operation.")


def main():
    try:
        raw = sys.stdin.buffer.read(8_000_001)
        if len(raw) > 8_000_000:
            raise BridgeError("The request is too large.")
        request = json.loads(raw)
        # Third-party libraries sometimes print diagnostics; only our sanitized
        # JSON envelope belongs on stdout. Never return exception repr/traceback.
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            result = dispatch(request)
        response = {"ok": True, "data": result}
    except ModuleNotFoundError:
        response = {"ok": False, "error": "Run open-music-setup-youtube (or scripts/setup-youtube-music.sh from the source tree), then try again."}
    except BridgeError as error:
        response = {"ok": False, "error": str(error)}
    except Exception:
        response = {"ok": False, "error": "YouTube Music could not complete the request. Check your connection, update ytmusicapi and yt-dlp, or reconnect your account."}
    print(json.dumps(response, ensure_ascii=False, allow_nan=False))


if __name__ == "__main__":
    main()
