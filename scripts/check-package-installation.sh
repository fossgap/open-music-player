#!/usr/bin/env bash
# Install packages in disposable containers, never on the host.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo 'Usage: scripts/check-package-installation.sh debian|kali|fedora|arch|appimage [ARTIFACT_DIRECTORY]' >&2
    exit 2
fi
case "$1" in
    debian) image=debian:bookworm-slim; format=deb ;;
    kali) image=kalilinux/kali-rolling:latest; format=deb ;;
    fedora) image=registry.fedoraproject.org/fedora:44; format=rpm ;;
    arch) image=archlinux:base; format=arch ;;
    appimage) image=ubuntu:24.04; format=appimage ;;
    *) echo "Unknown installation target: $1" >&2; exit 2 ;;
esac
[[ $(uname -m) == x86_64 ]] || { echo 'Installation checks require x86_64.' >&2; exit 1; }
artifacts=$(realpath "${2:-dist/release}")
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
(cd "$artifacts" && sha256sum --strict -c SHA256SUMS)
docker pull "$image"
docker image inspect "$image" --format '{{json .RepoTags}} {{json .RepoDigests}} {{.Id}}'
container=$(docker create --tmpfs /tmp:rw,exec --shm-size=1g \
    --env OPEN_MUSIC_INSTALL_TEST_CONTAINER=1 --env DEBIAN_FRONTEND=noninteractive \
    --volume "$artifacts:/artifacts:ro" \
    --volume "$PWD/scripts:/workspace/scripts:ro" \
    --volume "$PWD/tests/package-install:/package-test:ro" \
    "$image" bash /package-test/install.sh "$format" "$version")
trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT
docker start -a "$container"
