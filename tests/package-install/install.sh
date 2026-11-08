#!/usr/bin/env bash
set -euo pipefail
if [[ ${OPEN_MUSIC_INSTALL_TEST_CONTAINER:-} != 1 || ! -f /.dockerenv || ! -d /artifacts ]]; then
    echo 'Use scripts/check-package-installation.sh; this script installs container packages.' >&2
    exit 2
fi
format=${1:?Choose a package format}
version=${2:?Provide the expected version}
one_file() {
    if [[ $# != 1 || ! -f "$1" ]]; then
        echo 'Expected exactly one release artifact for this format.' >&2
        exit 1
    fi
    printf '%s\n' "$1"
}
cat /etc/os-release
case "$format" in
    deb)
        app=$(one_file /artifacts/open-music-player_*_amd64.deb)
        [[ $(dpkg-deb -f "$app" Version) == "$version" ]]
        apt-get update
        apt-get -y --no-install-recommends --no-remove install "$app"
        [[ $(dpkg-query -W -f='${Version}' open-music-player) == "$version" ]]
        apt-get check
        ;;
    rpm)
        app=$(one_file /artifacts/open-music-player-[0-9]*.x86_64.rpm)
        dnf -y --setopt=install_weak_deps=False install "$app"
        [[ $(rpm -q --qf '%{VERSION}' open-music-player) == "$version" ]]
        [[ $(rpm -q --qf '%{VERSION}-%{RELEASE}' open-music-player) == $(rpm -qp --qf '%{VERSION}-%{RELEASE}' "$app") ]]
        dnf check
        dnf -y install util-linux
        ;;
    arch)
        app=$(one_file /artifacts/open-music-player-[0-9]*-x86_64.pkg.tar.zst)
        pacman --noconfirm -Syu
        pacman --noconfirm -U "$app"
        [[ $(pacman -Q open-music-player) == "open-music-player $version-1" ]]
        pacman -Qk open-music-player
        ;;
    appimage)
        app=$(one_file /artifacts/open-music-player_*_amd64.deb)
        appimage=$(one_file /artifacts/OpenMusicPlayer-*-x86_64.AppImage)
        [[ $(dpkg-deb -f "$app" Version) == "$version" ]]
        apt-get update
        # No host GTK, GStreamer or FFmpeg: the AppImage must supply its runtime.
        apt-get -y --no-install-recommends install xvfb xauth dbus-x11 libglib2.0-bin \
            python3 xdotool fonts-dejavu-core ca-certificates util-linux
        install -Dm755 "$appimage" /opt/OpenMusicPlayer.AppImage
        for package in libgtk-4-1 libgstreamer1.0-0 ffmpeg; do
            [[ $(dpkg-query -W -f='${db:Status-Status}' "$package" 2>/dev/null || true) != installed ]]
        done
        ;;
    *) echo "Unknown package format: $format" >&2; exit 2 ;;
esac
useradd -m musictest
binary=/usr/bin/open-music-player
prefix=/usr
if [[ "$format" == appimage ]]; then
    mkdir -p /opt/open-music-extracted
    chown musictest:musictest /opt/open-music-extracted
    runuser -u musictest -- bash -c 'cd /opt/open-music-extracted && /opt/OpenMusicPlayer.AppImage --appimage-extract >/dev/null'
    prefix=/opt/open-music-extracted/squashfs-root/usr
    binary=/opt/open-music-extracted/squashfs-root/AppRun
    dpkg-deb -x "$app" /tmp/deb-inspection
    cmp /tmp/deb-inspection/usr/bin/open-music-player "$prefix/bin/open-music-player"
    # Fixtures use the bundled FFmpeg too; the launcher sets its library path.
    for tool in ffmpeg ffprobe; do
        printf '#!/bin/sh\nLD_LIBRARY_PATH="%s/lib" exec "%s/bin/%s" "$@"\n' "$prefix" "$prefix" "$tool" > "/usr/local/bin/$tool"
        chmod 755 "/usr/local/bin/$tool"
    done
else
    # Check the installed app before test dependencies can mask missing libraries.
    runuser -u musictest -- "$binary" --help >/tmp/installed-help.txt
    ffmpeg -version >/tmp/installed-ffmpeg.txt
    ffprobe -version >/tmp/installed-ffprobe.txt
fi
[[ $(runuser -u musictest -- "$binary" --version) == "Open Music Player $version" ]]
test -x "$prefix/bin/open-music-setup-youtube"
bash -n "$prefix/bin/open-music-setup-youtube"
test -s "$prefix/share/open-music-player/youtube-requirements.txt"
test -s "$prefix/share/licenses/open-music-player/LICENSE"
test -s "$prefix/share/doc/open-music-player/wiki/data-privacy.md"
test -s "$prefix/share/applications/io.github.openmusic.Player.desktop"
test -s "$prefix/share/icons/hicolor/scalable/apps/io.github.openmusic.Player.svg"
test -s "$prefix/share/metainfo/io.github.openmusic.Player.metainfo.xml"
case "$format" in
    deb) apt-get -y --no-install-recommends install xvfb xauth dbus-x11 libglib2.0-bin python3 xdotool fonts-dejavu-core ;;
    rpm) dnf -y install xorg-x11-server-Xvfb xorg-x11-xauth dbus-daemon glib2 python3 xdotool dejavu-sans-fonts ;;
    arch) pacman --noconfirm -S --needed xorg-server-xvfb xorg-xauth dbus glib2 python xdotool ttf-dejavu ;;
esac
runuser -u musictest -- env OPEN_MUSIC_BINARY="$binary" GTK_A11Y=none \
    xvfb-run -a dbus-run-session -- python3 /workspace/scripts/smoke-test.py
printf 'PACKAGE_INSTALLATION_SUITE_PASSED: %s\n' "$format"
