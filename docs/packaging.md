# Packaging

[README](../README.md) · [Installation](wiki/installation.md) · [CI and releases](ci-release.md)

Release automation builds on Debian 12 Bookworm for Linux x86_64. Building locally
on a newer distribution can increase the required glibc/native-library versions.
Native packages depend on the host GTK/GStreamer/FFmpeg; the AppImage bundles them.

## Native formats

After installing the build dependencies:

```sh
make deb          # dpkg-deb and dpkg-shlibdeps (dpkg-dev)
make rpm          # rpmbuild
make arch         # tar and zstd; produces an installable binary package
```

Artifacts go to `dist/`. DEB library dependencies are computed from the built
binary. RPM dependencies include Fedora's `ffmpeg-free`. Optional codecs can
extend native-package format support. All packages include the desktop entry,
icon, AppStream metadata, license, user wiki, and optional YouTube setup helper.
They do not bundle account credentials or install Python packages automatically.
`CARGO_TARGET_DIR` is respected; `OPEN_MUSIC_BINARY` can select an already-built
binary when invoking `scripts/package.sh` directly.

To build through Arch's native tools, first commit the intended source:

```sh
make arch-source
cd dist
makepkg -si
```

This separate target creates a committed-source archive and a `PKGBUILD` with the
actual SHA-256. The checked-in PKGBUILD is a template; use the generated one.

## AppImage

The Dockerfile includes the native packaging prerequisites: `patchelf`, `file`,
Adwaita icons, GIO TLS modules, librsvg/GObject Introspection development packages,
font configuration, and MIME data. Bootstrap the pinned x86_64 tools explicitly:

```sh
bash scripts/fetch-appimage-tools.sh
export PATH="$PWD/target/appimage-tools:$PATH"
export LINUXDEPLOY="$PWD/target/appimage-tools/linuxdeploy"
export APPIMAGETOOL="$PWD/target/appimage-tools/appimagetool"
export APPIMAGE_RUNTIME="$PWD/target/appimage-tools/runtime-x86_64"
make appimage
```

The helper verifies linuxdeploy, its GTK/GStreamer plugins, appimagetool, and the
type-2 runtime before use. A mismatched download stops the build; investigate and
review upstream changes before updating a pin. Custom verified tools can be
supplied through the variables above. `package.sh` itself does not download tools.

AppImage packaging includes FFmpeg/FFprobe, GStreamer plugins/scanners, GTK
resources, Adwaita icons, GIO HTTPS modules, font configuration, MIME data, and
runtime copyright notices. Host glibc, the display/audio session, and optional
account/YouTube dependencies are still required. The image is tested after
extraction on a minimal Ubuntu container before publication.

## Verification and publication

Use the complete [release workflow](ci-release.md) to build from a committed
snapshot, create checksums, and test DEB, RPM, Arch, and AppImage in clean target
containers. A successful local build alone does not establish distribution
compatibility. The workflow publishes only after all five installation jobs pass.

## Local installation

```sh
make build
sudo make install
# Remove program and documentation, retaining listening data:
sudo make uninstall
```

`PREFIX` defaults to `/usr`; `DESTDIR` supports staging for a package. Distribution
packages should be removed using their package manager instead of `make uninstall`.
