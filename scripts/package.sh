#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
format=${1:?Usage: scripts/package.sh deb|rpm|arch|arch-source|appimage}
case "$format" in
  deb|rpm|arch|arch-source|appimage) ;;
  *) printf 'Unknown format: %s\n' "$format" >&2; exit 2 ;;
esac
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
mkdir -p dist
root=$(mktemp -d "${TMPDIR:-/tmp}/open-music-package.XXXXXX")
trap 'rm -rf "$root"' EXIT
if [ "$format" != arch-source ]; then
  binary=$(realpath "${OPEN_MUSIC_BINARY:-${CARGO_TARGET_DIR:-target}/release/open-music-player}")
  make BINARY="$binary" DESTDIR="$root/root" PREFIX=/usr install
fi

case "$format" in
  deb)
    architecture=$(dpkg --print-architecture)
    mkdir -p "$root/dependencies/debian"
    cat > "$root/dependencies/debian/control" <<EOF
Source: open-music-player
Section: sound
Priority: optional
Maintainer: Open Music Player contributors <noreply@localhost>

Package: open-music-player
Architecture: any
Description: Native Linux music player
EOF
    library_depends=$(cd "$root/dependencies" && dpkg-shlibdeps -O -e"$binary" | sed -n 's/^shlibs:Depends=//p')
    [ -n "$library_depends" ] || { echo 'Could not determine runtime dependencies.' >&2; exit 1; }
    mkdir -p "$root/root/DEBIAN"
    cat > "$root/root/DEBIAN/control" <<EOF
Package: open-music-player
Version: $version
Section: sound
Priority: optional
Architecture: $architecture
Maintainer: Open Music Player contributors <noreply@localhost>
Homepage: https://github.com/fossgap/open-music-player
Depends: $library_depends, libgtk-4-1 (>= 4.8), gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-pulseaudio | gstreamer1.0-alsa, ffmpeg, ca-certificates, adwaita-icon-theme, glib-networking
Recommends: gstreamer1.0-libav, gstreamer1.0-plugins-bad, gstreamer1.0-plugins-ugly, python3, python3-venv, libsecret-tools
Description: A native home for music, podcasts, and radio
 Rust and GTK player with a local library, podcasts, internet radio,
 playlists, desktop media controls, and a mini player.
EOF
    dpkg-deb --root-owner-group --build "$root/root" "dist/open-music-player_${version}_${architecture}.deb"
    ;;
  rpm)
    mkdir -p "$root/rpm/"{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS,tmp,db}
    cp -a "$root/root" "$root/rpm/SOURCES/root"
    rpmbuild --define "app_version $version" --define "_topdir $root/rpm" --define "_tmppath $root/rpm/tmp" --define "_dbpath $root/rpm/db" --define '_build_id_links none' -bb packaging/open-music-player.spec
    find "$root/rpm/RPMS" -name '*.rpm' -exec cp '{}' dist/ \;
    ;;
  arch)
    cat > "$root/root/.PKGINFO" <<EOF
pkgname = open-music-player
pkgbase = open-music-player
pkgver = $version-1
pkgdesc = A native Linux home for music, podcasts, and radio
url = https://github.com/fossgap/open-music-player
builddate = ${SOURCE_DATE_EPOCH:-$(date +%s)}
packager = Open Music Player contributors
size = $(du -sb "$root/root/usr" | cut -f1)
arch = $(uname -m)
license = GPL-3.0-only
depend = glibc
depend = gcc-libs
depend = dbus
depend = gtk4>=4.8
depend = gstreamer
depend = gst-plugins-base
depend = gst-plugins-good
depend = ffmpeg
depend = ca-certificates
depend = adwaita-icon-theme
depend = glib-networking
optdepend = gst-libav: additional audio codecs
optdepend = gst-plugins-bad: additional streams and Spotify playback speed
optdepend = python: YouTube Music support
optdepend = python-pip: YouTube Music setup
optdepend = libsecret: YouTube Music account storage
optdepend = deno: YouTube Music playback
EOF
    tar --zstd --owner=0 --group=0 -cf "dist/open-music-player-$version-1-$(uname -m).pkg.tar.zst" -C "$root/root" .PKGINFO usr
    ;;
  arch-source)
    git diff --quiet HEAD -- || { echo 'Commit tracked changes before creating the source archive.' >&2; exit 1; }
    git archive --format=tar.gz --prefix="open-music-player-$version/" HEAD > "dist/open-music-player-$version.tar.gz"
    sed "s/^pkgver=.*/pkgver=$version/" packaging/PKGBUILD > dist/PKGBUILD
    checksum=$(sha256sum "dist/open-music-player-$version.tar.gz" | cut -d ' ' -f 1)
    sed -i "s/^sha256sums=.*/sha256sums=('$checksum')/" dist/PKGBUILD
    printf 'Source package ready. On Arch: cd dist && makepkg -si\n'
    ;;
  appimage)
    : "${LINUXDEPLOY:?Set LINUXDEPLOY to your linuxdeploy AppImage}"
    for plugin in gtk gstreamer; do
      command -v "linuxdeploy-plugin-$plugin" >/dev/null || command -v "linuxdeploy-plugin-$plugin.sh" >/dev/null || { printf 'linuxdeploy-plugin-%s is required on PATH\n' "$plugin" >&2; exit 1; }
    done
    appdir="$root/OpenMusicPlayer.AppDir"
    mv "$root/root" "$appdir"
    libraries=()
    for name in libfribidi.so.0 libharfbuzz.so.0 libwayland-client.so.0; do
      library=$(ldd "$appdir/usr/bin/open-music-player" | awk -v name="$name" '$1 == name { print $3; exit }')
      [ -f "$library" ] || { echo "Missing runtime library: $name" >&2; exit 1; }
      libraries+=(--library "$library")
    done
    mkdir -p "$appdir/usr/etc" "$appdir/usr/share/doc/open-music-player/runtime"
    cp -a /usr/share/mime "$appdir/usr/share/"
    cp -aL /etc/fonts "$appdir/usr/etc/"
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
    for module in "$gio_modules/"*.so; do libraries+=(--library "$module"); done
    # The GTK/GStreamer plugins install resources, scanners, and AppRun hooks.
    install -Dm755 "$(command -v ffprobe)" "$appdir/usr/bin/ffprobe"
    install -Dm755 "$(command -v ffmpeg)" "$appdir/usr/bin/ffmpeg"
    export DEPLOY_GTK_VERSION=4
    export APPIMAGE_EXTRACT_AND_RUN=1
    "$LINUXDEPLOY" --appdir "$appdir" --executable "$appdir/usr/bin/open-music-player" --executable "$appdir/usr/bin/ffprobe" --executable "$appdir/usr/bin/ffmpeg" --desktop-file packaging/io.github.openmusic.Player.desktop --icon-file resources/io.github.openmusic.Player.svg --custom-apprun packaging/AppRun --plugin gtk --plugin gstreamer "${libraries[@]}"
    # AppRun supplies the library path; retain the exact native-package binary
    # after linuxdeploy has inspected its dependencies and adjusted ELF rpaths.
    install -m755 "$binary" "$appdir/usr/bin/open-music-player"
    # Keep runtime notices for libraries and resources from the build distribution.
    for copyright in /usr/share/doc/*/copyright; do
      package=$(basename "$(dirname "$copyright")")
      cp -L "$copyright" "$appdir/usr/share/doc/open-music-player/runtime/$package.copyright"
    done
    : "${APPIMAGETOOL:?Run scripts/fetch-appimage-tools.sh or provide a verified APPIMAGETOOL}"
    : "${APPIMAGE_RUNTIME:?Provide the verified type-2 runtime}"
    ARCH="$(uname -m)" "$APPIMAGETOOL" --no-appstream --runtime-file "$APPIMAGE_RUNTIME" --mksquashfs-opt -processors --mksquashfs-opt 2 "$appdir" "$PWD/dist/OpenMusicPlayer-$version-$(uname -m).AppImage"
    ;;
  *) printf 'Unknown format: %s\n' "$format" >&2; exit 2 ;;
esac
