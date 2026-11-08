#!/usr/bin/env bash
# Build and verify in Docker. Only the GitHub publish job creates releases.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-check}
case "$mode" in check|release) ;; *) echo 'Usage: scripts/ci.sh check|release' >&2; exit 2;; esac
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
[[ -n "$version" ]]
if [[ "$mode" == release && ${GITHUB_REF_TYPE:-} == tag ]]; then
    [[ ${GITHUB_REF_NAME:-} == "v$version" ]] || { echo 'Release tag must match Cargo.toml version.' >&2; exit 1; }
fi
output=${OPEN_MUSIC_CI_OUTPUT:-dist/release}
if [[ "$mode" == release ]]; then
    [[ $(uname -m) == x86_64 ]] || { echo 'Release builds currently target x86_64.' >&2; exit 1; }
    [[ ! -e "$output" ]] || { echo "Choose a new output directory: $output" >&2; exit 1; }
    git diff --quiet HEAD -- || { echo 'Commit tracked changes before building release artifacts.' >&2; exit 1; }
fi
# Build from the current context so the binary and source archive cannot diverge
# because of a stale local image. Docker reuses dependency layers where possible.
image=${OPEN_MUSIC_CI_IMAGE:-open-music-player-ci}
if [[ "$mode" == release ]]; then
    git archive HEAD | docker build -t "$image" -
else
    docker build -t "$image" .
fi
container=$(docker create --tmpfs /tmp:rw,exec --shm-size=1g \
    --env GTK_A11Y=none --env SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)" \
    "$image" bash scripts/ci-inside.sh "$mode")
trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT
docker start -a "$container"
if [[ "$mode" == release ]]; then
    mkdir -p "$output"
    docker cp "$container:/workspace/dist/release/." "$output/"
    git archive --format=tar.gz --prefix="open-music-player-$version/" HEAD > "$output/open-music-player-$version-source.tar.gz"
    (cd "$output" && sha256sum ./*.deb ./*.rpm ./*.pkg.tar.zst ./*.AppImage ./*-source.tar.gz > SHA256SUMS && sha256sum -c SHA256SUMS)
fi
