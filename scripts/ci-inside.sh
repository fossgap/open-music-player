#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-check}
case "$mode" in check|release) ;; *) exit 2;; esac
bash scripts/container-check.sh
if [[ "$mode" == release ]]; then
    cargo build --release --locked
    for format in deb rpm arch; do bash scripts/package.sh "$format"; done
    bash scripts/fetch-appimage-tools.sh
    export PATH="$PWD/target/appimage-tools:$PATH"
    export LINUXDEPLOY="$PWD/target/appimage-tools/linuxdeploy"
    export APPIMAGETOOL="$PWD/target/appimage-tools/appimagetool"
    export APPIMAGE_RUNTIME="$PWD/target/appimage-tools/runtime-x86_64"
    bash scripts/package.sh appimage
    mkdir dist/release
    mv dist/*.deb dist/*.rpm dist/*.pkg.tar.zst dist/*.AppImage dist/release/
fi
