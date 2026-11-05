#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
format=${1:?Usage: scripts/package.sh deb|rpm|arch|appimage}
case "$format" in
  deb|rpm|arch|appimage) ;;
  *) printf 'Unknown format: %s\n' "$format" >&2; exit 2 ;;
esac
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
mkdir -p dist
root=$(mktemp -d "${TMPDIR:-/tmp}/open-audio-package.XXXXXX")
trap 'rm -rf "$root"' EXIT
if [ "$format" != arch ]; then
  make DESTDIR="$root/root" PREFIX=/usr install
fi

case "$format" in
  deb)
    architecture=$(dpkg --print-architecture)
    mkdir -p "$root/root/DEBIAN"
    cat > "$root/root/DEBIAN/control" <<EOF
Package: open-audio-player
Version: $version
Section: sound
Priority: optional
Architecture: $architecture
Maintainer: Open Audio Player contributors
Depends: libgtk-4-1 (>= 4.8), libgstreamer1.0-0 (>= 1.20), gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-pulseaudio | gstreamer1.0-alsa, ffmpeg, ca-certificates
Recommends: gstreamer1.0-libav, gstreamer1.0-plugins-bad, gstreamer1.0-plugins-ugly, yt-dlp
Description: A native home for music, podcasts, and radio
 Rust and GTK player with a local library, podcasts, internet radio,
 playlists, desktop media controls, and a mini player.
EOF
    dpkg-deb --root-owner-group --build "$root/root" "dist/open-audio-player_${version}_${architecture}.deb"
    ;;
  rpm)
    mkdir -p "$root/rpm/"{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS,tmp,db}
    cp -a "$root/root" "$root/rpm/SOURCES/root"
    rpmbuild --define "_topdir $root/rpm" --define "_tmppath $root/rpm/tmp" --define "_dbpath $root/rpm/db" --define '_build_id_links none' -bb packaging/open-audio-player.spec
    find "$root/rpm/RPMS" -name '*.rpm' -exec cp '{}' dist/ \;
    ;;
  arch)
    mkdir -p "$root/source/open-audio-player-$version"
    tar --exclude=./target --exclude=./dist --exclude=./.git --exclude=./.impeccable --exclude=./.worktrees --exclude=./.agents --exclude=./.codex --exclude=./.aws --exclude=__pycache__ --exclude='*.pyc' -cf - . | tar -xf - -C "$root/source/open-audio-player-$version"
    tar -czf "dist/open-audio-player-$version.tar.gz" -C "$root/source" "open-audio-player-$version"
    cp packaging/PKGBUILD dist/PKGBUILD
    checksum=$(sha256sum "dist/open-audio-player-$version.tar.gz" | cut -d ' ' -f 1)
    sed -i "s/^sha256sums=.*/sha256sums=('$checksum')/" dist/PKGBUILD
    printf 'Source package ready. On Arch: cd dist && makepkg -si\n'
    ;;
  appimage)
    : "${LINUXDEPLOY:?Set LINUXDEPLOY to your linuxdeploy AppImage}"
    for plugin in gtk gstreamer; do
      command -v "linuxdeploy-plugin-$plugin" >/dev/null || command -v "linuxdeploy-plugin-$plugin.sh" >/dev/null || { printf 'linuxdeploy-plugin-%s is required on PATH\n' "$plugin" >&2; exit 1; }
    done
    appdir="$root/OpenAudioPlayer.AppDir"
    mv "$root/root" "$appdir"
    # GTK's built-in fallback omits some transport/navigation symbols.
    icon_theme="$(pkg-config --variable=prefix gtk4)/share/icons/Adwaita"
    [ -d "$icon_theme" ] || { printf 'Install adwaita-icon-theme before building an AppImage\n' >&2; exit 1; }
    cp -a "$icon_theme" "$appdir/usr/share/icons/"
    if [ -f /usr/share/doc/adwaita-icon-theme/copyright ]; then
      install -Dm644 /usr/share/doc/adwaita-icon-theme/copyright "$appdir/usr/share/doc/adwaita-icon-theme/copyright"
    fi
    gio_modules=$(pkg-config --variable=giomoduledir gio-2.0)
    [ -f "$gio_modules/libgiognutls.so" ] || { printf 'Install glib-networking before building an AppImage\n' >&2; exit 1; }
    mkdir -p "$appdir/usr/lib/gio"
    cp -a "$gio_modules" "$appdir/usr/lib/gio/modules"
    # The GTK/GStreamer plugins install resources, scanners, and AppRun hooks.
    install -Dm755 "$(command -v ffprobe)" "$appdir/usr/bin/ffprobe"
    install -Dm755 "$(command -v ffmpeg)" "$appdir/usr/bin/ffmpeg"
    export DEPLOY_GTK_VERSION=4
    export OUTPUT="$PWD/dist/OpenAudioPlayer-$version-$(uname -m).AppImage"
    "$LINUXDEPLOY" --appdir "$appdir" --executable "$appdir/usr/bin/open-audio-player" --executable "$appdir/usr/bin/ffprobe" --executable "$appdir/usr/bin/ffmpeg" --desktop-file packaging/io.github.openaudio.Player.desktop --icon-file resources/io.github.openaudio.Player.svg --custom-apprun packaging/AppRun --plugin gtk --plugin gstreamer --output appimage
    ;;
  *) printf 'Unknown format: %s\n' "$format" >&2; exit 2 ;;
esac
