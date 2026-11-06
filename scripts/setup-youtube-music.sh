#!/usr/bin/env bash
# Install/update optional support in a private venv, without changing system Python.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
requirements="$script_dir/../resources/youtube-requirements.txt"
if [[ ! -f "$requirements" ]]; then
  requirements="$script_dir/../share/open-audio-player/youtube-requirements.txt"
fi
[[ -f "$requirements" ]] || { printf 'YouTube Music requirements file is missing. Reinstall Open Audio Player.\n' >&2; exit 1; }
if [[ -n "${OPEN_AUDIO_DATA_DIR:-}" ]]; then
  data_dir=$OPEN_AUDIO_DATA_DIR
elif [[ "${XDG_DATA_HOME:-}" == /* ]]; then
  data_dir="$XDG_DATA_HOME/open-audio-player"
else
  data_dir="$HOME/.local/share/open-audio-player"
fi
python3 -m venv "$data_dir/youtube-python"
"$data_dir/youtube-python/bin/python" -m pip install --upgrade -r "$requirements"
printf '\nYouTube Music support is ready. Open YouTube Music in the sidebar.\n'
if ! command -v secret-tool >/dev/null; then
  printf 'Account connection also needs secret-tool (Debian/Ubuntu: libsecret-tools) and an unlocked desktop keyring.\n'
fi
if ! command -v deno >/dev/null && ! command -v node >/dev/null; then
  printf 'Playback also needs Deno 2.3+ or Node.js 22+ on PATH.\n'
fi
