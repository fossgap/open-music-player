#!/usr/bin/env bash
# Explicit packaging bootstrap; fail closed if an upstream asset changes.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -m) == x86_64 ]] || { echo 'Pinned AppImage tools require x86_64.' >&2; exit 1; }
cache="$PWD/target/appimage-tools"
mkdir -p "$cache"
fetch() {
    local destination="$cache/$1" checksum=$2 url=$3
    if [[ ! -f "$destination" ]]; then
        curl --fail --location --retry 3 "$url" -o "$destination.download"
        printf '%s  %s\n' "$checksum" "$destination.download" | sha256sum -c -
        mv "$destination.download" "$destination"
    fi
    printf '%s  %s\n' "$checksum" "$destination" | sha256sum -c -
    chmod 755 "$destination"
}
fetch linuxdeploy c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
fetch linuxdeploy-plugin-gtk.sh b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171 \
    https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/7a3fbc31a9e5075073ff8790f26effbac5f84453/linuxdeploy-plugin-gtk.sh
fetch linuxdeploy-plugin-gstreamer.sh d1cf90701047aa9583c274bb66dd82db352255374fbfb8f3c4c89a05fe23bce0 \
    https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gstreamer/432af639725bc8b99df12351e86bb2e8a6b3585f/linuxdeploy-plugin-gstreamer.sh
fetch appimagetool ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 \
    https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
fetch runtime-x86_64 617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46 \
    https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64
