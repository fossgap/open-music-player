# Installation and updates

[User guide](README.md) · [Data privacy](data-privacy.md) · [Troubleshooting](troubleshooting.md)

## Download a package

See [GitHub Releases](https://github.com/fossgap/open-music-player/releases) for
published packages. Release builds target **Linux x86_64**, with a Debian 12
Bookworm baseline (glibc 2.36, GTK 4.8). Until a version is published, build from
source or use the artifacts from a successful manual Release CI run.

| System | File | Install command |
| --- | --- | --- |
| Debian 12+, Ubuntu 24.04+, Kali | `.deb` | `sudo apt install ./open-music-player_*_amd64.deb` |
| Fedora | `.rpm` | `sudo dnf install ./open-music-player-[0-9]*.x86_64.rpm` |
| Arch Linux | `.pkg.tar.zst` | `sudo pacman -U ./open-music-player-[0-9]*-x86_64.pkg.tar.zst` |
| Other compatible Linux systems | `.AppImage` | See below |

Run commands in the download directory with only the desired version present,
or replace the wildcard with the exact filename. Native packages declare their
runtime dependencies. Launch **Open Music Player** from the desktop menu or run
`open-music-player`.

## Verify a download

Download `SHA256SUMS` from the same release. With every release artifact in the
directory, run:

```sh
sha256sum --strict -c SHA256SUMS
```

For a single downloaded package, use its exact filename:

```sh
grep -F ' ./open-music-player_0.1.0_amd64.deb' SHA256SUMS | sha256sum --strict -c -
```

Replace the example version when needed. The command must report `OK` and exit
successfully. A missing checksum entry is an error, not successful verification.
Checksums detect corruption; they are not independent signatures or proof of a
trusted publisher. Obtain both files from the project's GitHub release. See
[release checks](../ci-release.md) for the installation tests that gate publication.

## AppImage

```sh
chmod +x OpenMusicPlayer-*-x86_64.AppImage
./OpenMusicPlayer-0.1.0-x86_64.AppImage
```

Use your actual version in the second command. The image bundles GTK, GStreamer,
codecs from the build environment, and FFmpeg. A compatible Linux/glibc system,
display/audio session, and certificates are still required. Optional YouTube
Python dependencies, a JavaScript runtime, and the desktop keyring remain host
requirements.

Without FUSE, extract it into a new directory and run its launcher:

```sh
./OpenMusicPlayer-0.1.0-x86_64.AppImage --appimage-extract
./squashfs-root/AppRun
```

The optional YouTube setup command is then
`./squashfs-root/usr/bin/open-music-setup-youtube`. See the
[streaming guide](streaming.md) before connecting an account.

## Build from source

Install Rust 1.89+ with rustfmt and Clippy, then on Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev libgtk-4-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav gstreamer1.0-pulseaudio ffmpeg
git clone https://github.com/fossgap/open-music-player.git
cd open-music-player
cargo run --locked
```

For an optimized local installation:

```sh
make build
sudo make install
```

`sudo make uninstall` removes that installation and its documentation, retaining
your library and downloaded music. Package-managed installations should be
removed through their package manager instead.

## Updates and removal

Install a newer native package with the same command, or replace the AppImage.
Open Music Player does not download application updates automatically. Back up
your [data directory](data-privacy.md#local-storage-and-removal) with the app
closed before upgrading. Removing the package does not remove your library,
music files, downloaded audio, or desktop-keyring credentials; disconnect
accounts before uninstalling if you want to remove their stored sessions.
