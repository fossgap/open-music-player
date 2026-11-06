#!/usr/bin/env python3
"""Offline tests: no personal cookies, keyring entries or provider writes."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("bridge", ROOT / "resources/youtube_music.py")
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)
A, B = "abcdefghijk", "ABCDEFGHIJK"


class Provider:
    def __init__(self):
        self.data = {"title": "Mix", "owned": True, "trackCount": 1,
                     "tracks": [{"videoId": A, "setVideoId": "original", "title": "First", "artists": [{"name": "Artist"}], "duration": "3:02"}]}
        self.calls = []
        self.fail_add = False

    def get_account_info(self):
        return {"accountName": "Test", "channelHandle": "@test"}

    def get_library_playlists(self, limit):
        return [{"playlistId": "PL_test"}]

    def get_playlist(self, playlist_id, limit):
        return copy.deepcopy(self.data)

    def add_playlist_items(self, playlist_id, ids, duplicates):
        self.calls.append(("add", ids, duplicates))
        if self.fail_add:
            raise RuntimeError("private provider diagnostic")
        for i, video in enumerate(ids):
            self.data["tracks"].append({"videoId": video, "setVideoId": f"new{i}", "title": "Song"})
        self.data["trackCount"] = len(self.data["tracks"])
        return {"status": "STATUS_SUCCEEDED"}

    def remove_playlist_items(self, playlist_id, entries):
        self.calls.append(("remove", entries))
        remove = {e["setVideoId"] for e in entries}
        self.data["tracks"] = [e for e in self.data["tracks"] if e["setVideoId"] not in remove]
        self.data["trackCount"] = len(self.data["tracks"])
        return "STATUS_SUCCEEDED"

    def edit_playlist(self, playlist_id, title):
        self.calls.append(("rename", title))
        self.data["title"] = title
        return "STATUS_SUCCEEDED"

    def create_playlist(self, name, description, privacy_status):
        self.calls.append(("create", privacy_status))
        self.data.update(title=name, tracks=[], trackCount=0)
        return "PL_test"


def local(name="Mix", ids=None):
    ids = [A] if ids is None else ids
    return {"local_id": "local1", "name": name, "video_ids": ids, "valid": True,
            "snapshot": bridge.snapshot(name, ids),
            "link": {"id": "PL_test", "account_id": "@test", "editable": True,
                     "remote_snapshot": bridge.snapshot("Mix", [A]), "local_snapshot": bridge.snapshot("Mix", [A])}}


def sync(client, playlists=(), ignored=()):
    return bridge.synchronize(client, {"account_id": "@test", "playlists": playlists, "ignored": ignored})


class BridgeTests(unittest.TestCase):
    def test_search_only_requests_songs_and_deduplicates(self):
        class Search:
            def search(self, query, **kwargs):
                self.kwargs = kwargs
                return [{"videoId": A, "title": "Song"}, {"videoId": A}, {"videoId": "invalid"}]
        client = Search()
        with patch.object(bridge, "music_client", return_value=client):
            result = bridge.dispatch({"op": "search", "query": "artist"})
        self.assertEqual(client.kwargs, {"filter": "songs", "limit": 50})
        self.assertEqual([t["video_id"] for t in result], [A])

    def test_remote_import_and_remote_edits_never_write(self):
        client = Provider()
        self.assertEqual(sync(client)["playlists"][0]["tracks"][0]["duration"], 182)
        client.data["title"] = "Online change"
        result = sync(client, [local()])
        self.assertEqual(result["playlists"][0]["name"], "Online change")
        self.assertEqual(client.calls, [])

    def test_push_preserves_order_duplicates_and_adds_before_removing(self):
        client = Provider()
        result = sync(client, [local("Renamed", [B, A, B])])
        self.assertEqual(result["uploaded"], 1)
        self.assertEqual([e["videoId"] for e in client.data["tracks"]], [B, A, B])
        self.assertEqual([c[0] for c in client.calls], ["add", "remove", "rename"])
        self.assertTrue(client.calls[0][2])

    def test_conflict_read_only_and_invalid_local_never_write(self):
        for mode in ["both", "read-only", "local-file"]:
            client = Provider()
            changed = local("Local edit")
            if mode == "both": client.data["title"] = "Online edit"
            if mode == "read-only": client.data["owned"] = False
            if mode == "local-file": changed["valid"] = False
            result = sync(client, [changed])
            self.assertEqual(len(result["conflicts"]), 1, mode)
            self.assertEqual(client.calls, [], mode)

    def test_failed_add_keeps_original_tracks(self):
        client = Provider()
        client.fail_add = True
        result = sync(client, [local("Mix", [B])])
        self.assertEqual(result["failed"], ["Mix"])
        self.assertEqual([e["videoId"] for e in client.data["tracks"]], [A])
        self.assertEqual([c[0] for c in client.calls], ["add"])

    def test_missing_set_ids_never_write(self):
        client = Provider()
        del client.data["tracks"][0]["setVideoId"]
        self.assertEqual(sync(client, [local("Mix", [B])])["failed"], ["Mix"])
        self.assertEqual(client.calls, [])

    def test_incomplete_and_unavailable_playlists_are_not_destructive(self):
        client = Provider()
        client.data["trackCount"] = 2
        self.assertEqual(sync(client)["failed"], ["PL_test"])
        client.data["trackCount"] = 1
        client.data["tracks"][0]["isAvailable"] = False
        remote = sync(client)["playlists"][0]
        self.assertEqual(remote["unavailable"], 1)
        self.assertFalse(remote["editable"])
        self.assertEqual(client.calls, [])

    def test_removed_local_copy_is_not_reimported(self):
        self.assertEqual(sync(Provider(), ignored=["PL_test"])["playlists"], [])

    def test_account_mismatch_prevents_writes(self):
        client = Provider()
        with self.assertRaises(bridge.BridgeError):
            bridge.synchronize(client, {"account_id": "@different", "playlists": [local("New")]})
        self.assertEqual(client.calls, [])

    def test_cookie_filter_drops_other_sites_and_expired_sessions(self):
        cookie = {"name": "__Secure-3PAPISID", "value": "test-only", "domain": ".youtube.com", "path": "/", "secure": True, "expires": None}
        other = dict(cookie, domain=".google.com")
        expired = dict(cookie, name="expired", expires=1)
        jar = bridge.cookie_jar([cookie, other, expired])
        self.assertEqual(bridge.youtube_cookies(jar), [cookie])
        with self.assertRaises(bridge.BridgeError): bridge.keyring("lookup", "not-a-namespace")

    def test_session_mismatch_does_not_expose_secrets(self):
        payload = json.dumps({"account": {"id": "@other"}, "cookies": [{"value": "test-secret"}]}).encode()
        with patch.object(bridge, "keyring", return_value=payload):
            with self.assertRaises(bridge.BridgeError) as caught: bridge.session({"namespace": "t0123456789012345", "account_id": "@test"})
        self.assertNotIn("test-secret", str(caught.exception))

    def test_process_protocol_sanitizes_invalid_input(self):
        result = subprocess.run([sys.executable, str(ROOT / "resources/youtube_music.py")], input=b'{"private":"test-secret"}', stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
        response = json.loads(result.stdout)
        self.assertFalse(response["ok"])
        self.assertNotIn(b"test-secret", result.stdout + result.stderr)

    def test_connect_saves_only_youtube_cookies_after_account_validation(self):
        cookie = {"name": "__Secure-3PAPISID", "value": "test-only", "domain": ".youtube.com", "path": "/", "secure": True, "expires": None}
        jar = bridge.cookie_jar([cookie])
        unrelated = copy.copy(next(iter(jar))); unrelated.domain = ".example.com"; jar.set_cookie(unrelated)
        cookies_module = types.SimpleNamespace(YoutubeDLCookieJar=None, extract_cookies_from_browser=lambda *_args: jar)
        with patch.dict(sys.modules, {"yt_dlp.cookies": cookies_module}), patch.object(bridge, "music_client", return_value=Provider()), patch.object(bridge, "keyring") as keyring:
            account = bridge.connect({"browser": "firefox", "namespace": "t0123456789012345"})
        self.assertEqual(account["id"], "@test")
        action, _namespace, value = keyring.call_args.args
        self.assertEqual(action, "store")
        self.assertEqual(json.loads(value)["cookies"], [cookie])

    def test_failed_account_validation_never_stores_credentials(self):
        cookies_module = types.SimpleNamespace(YoutubeDLCookieJar=None, extract_cookies_from_browser=lambda *_args: [])
        with patch.dict(sys.modules, {"yt_dlp.cookies": cookies_module}), patch.object(bridge, "music_client", side_effect=bridge.BridgeError("Expired session")), patch.object(bridge, "keyring") as keyring:
            with self.assertRaises(bridge.BridgeError): bridge.connect({"browser": "firefox", "namespace": "t0123456789012345"})
        keyring.assert_not_called()

    def test_publish_is_private_and_keeps_duplicates(self):
        client = Provider()
        request = {"op": "publish", "account_id": "@test", "name": "New", "local_id": "local", "video_ids": [B, A, B]}
        with patch.object(bridge, "session", return_value={"cookies": []}), patch.object(bridge, "music_client", return_value=client):
            result = bridge.dispatch(request)
        self.assertEqual(client.calls[0], ("create", "PRIVATE"))
        self.assertEqual([t["video_id"] for t in result["tracks"]], [B, A, B])
        self.assertEqual(result["local_id"], "local")

    def test_partial_publish_reports_creation_without_claiming_success(self):
        client = Provider(); client.fail_add = True
        with patch.object(bridge, "session", return_value={"cookies": []}), patch.object(bridge, "music_client", return_value=client):
            with self.assertRaisesRegex(bridge.BridgeError, "private playlist was created"):
                bridge.dispatch({"op": "publish", "account_id": "@test", "name": "New", "local_id": "local", "video_ids": [A]})

    def test_explicit_pull_never_modifies_online_playlist(self):
        client = Provider()
        with patch.object(bridge, "session", return_value={"cookies": []}), patch.object(bridge, "music_client", return_value=client):
            remote = bridge.dispatch({"op": "pull", "account_id": "@test", "playlist_id": "PL_test", "local_id": "local"})
        self.assertEqual(remote["local_id"], "local")
        self.assertEqual(client.calls, [])

    def test_resolver_returns_only_audio_url_and_safe_headers(self):
        class Downloader:
            def __init__(self, options):
                self.options = options
                self.cookiejar = bridge.cookie_jar([])
            def __enter__(self): return self
            def __exit__(self, *_args): pass
            def extract_info(self, uri, download):
                self.uri = uri
                self.download = download
                return {"url": "https://media.example/audio", "http_headers": {"User-Agent": "player", "Cookie": "test-secret", "Authorization": "test-secret", "Origin": "bad\r\nheader"}}
        downloader = Downloader({})
        with patch.dict(sys.modules, {"yt_dlp": types.SimpleNamespace(YoutubeDL=lambda options: downloader)}), patch.object(bridge.shutil, "which", return_value="/test/node"):
            result = bridge.resolve([], A)
        self.assertEqual(result["headers"], {"User-Agent": "player"})
        self.assertEqual(downloader.uri, f"https://music.youtube.com/watch?v={A}")
        self.assertFalse(downloader.download)


if __name__ == "__main__":
    unittest.main()
