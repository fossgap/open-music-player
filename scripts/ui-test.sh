#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
test_data=$(mktemp -d "${TMPDIR:-/tmp}/open-music-ui.XXXXXX")
trap 'rm -rf "$test_data"' EXIT
export OPEN_MUSIC_DATA_DIR="$test_data"
export XDG_DATA_HOME="$test_data/xdg"
export OPEN_MUSIC_FAKE_SINK=1 GTK_A11Y=none GTK_USE_PORTAL=0 GIO_USE_VFS=local
xvfb-run -a dbus-run-session -- env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --locked ui_browsing_preserves_scroll_selection_and_collections -- --ignored --test-threads=1
