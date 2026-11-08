#!/usr/bin/env bash
# Use an actual WM for resize/close semantics on different GTK releases.
set -euo pipefail
wm_log=$(mktemp)
xfwm4 --compositor=off --sm-client-disable >"$wm_log" 2>&1 &
wm_pid=$!
cleanup() {
    kill "$wm_pid" 2>/dev/null || true
    wait "$wm_pid" 2>/dev/null || true
    rm -f "$wm_log"
}
trap cleanup EXIT
ready=0
for ((attempt=0; attempt<50; attempt++)); do
    if [[ $(xprop -root _NET_SUPPORTING_WM_CHECK) == *'window id #'* ]]; then
        ready=1
        break
    fi
    sleep 0.1
done
if [[ "$ready" != 1 ]]; then cat "$wm_log" >&2; exit 1; fi
OPEN_MUSIC_TEST_WM=1 python3 /workspace/scripts/smoke-test.py
