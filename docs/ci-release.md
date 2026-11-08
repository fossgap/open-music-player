# CI and releases

[README](../README.md) · [Development](development.md) · [Packaging](packaging.md)

## Checks

[Test CI](../.github/workflows/test-ci.yml) runs on main-branch pushes, pull
requests, and manual dispatch. It builds the Debian Bookworm Rust/GTK environment
and runs `make check`, `make test`, shell checks, GTK regressions, and the offline
application smoke suite. Tests use generated media and local HTTP fixtures, not
real provider accounts. A passing run does not verify live Spotify/YouTube
authentication or playlist writes.

Actions are pinned to verified commit hashes. Test, build, and verification jobs
have read-only repository permissions. Only the final publication job receives
`contents: write`.

## Release builds and package verification

[Release CI](../.github/workflows/release-ci.yml) runs on `v*` tag pushes or manual
dispatch. It rejects a tag that does not equal `v` plus the Cargo package version,
repeats the checks, and builds:

- A Debian/Ubuntu/Kali DEB.
- A Fedora RPM.
- An installable Arch `.pkg.tar.zst`.
- An AppImage with GTK, GStreamer, and FFmpeg.
- An archive of the exact committed application source.
- `SHA256SUMS` covering all five artifacts.

Release Docker builds use `git archive HEAD` as their context. Untracked files
are excluded, tracked changes must be committed, and a new output directory is
required. This keeps the source archive and packaged application at the same
revision. AppImage build tools are downloaded explicitly and checked against
pinned SHA-256 hashes before execution.

Five jobs download the exact build artifacts and test installation in fresh
containers:

| Target | Verification |
| --- | --- |
| Debian 12 Bookworm | APT dependency resolution and DEB installation |
| Kali rolling | APT dependency resolution and the same DEB |
| Fedora 44 | DNF dependency resolution and RPM installation |
| Arch rolling | Pacman installation of the Arch binary package |
| Ubuntu 24.04 | AppImage extraction without FUSE, with no host GTK 4/GStreamer/FFmpeg installed |

All jobs verify checksums and the expected version, check desktop/icon/AppStream,
license, wiki, and optional-helper files, then run the installed application as
an ordinary user through `scripts/smoke-test.py` under Xvfb and the Xfwm window
manager. This gives resizing and closing real desktop semantics across GTK
versions. That suite covers local media,
playback/seek, MPRIS, library/playlist operations, local podcast-directory/feed
fixtures, downloads, and persistence. The AppImage's application binary must
also match the DEB byte-for-byte. Fixtures and state are disposable; no host
music, accounts, display, or audio devices are mounted.

Packages and source are retained as `linux-x86_64-release` for 14 days. Installation
logs include the base-image digest and are retained for 14 days as
`package-verification-<target>`, including failures. Publication depends on every
verification job passing and rechecks the downloaded checksums. An existing
release is not overwritten. Checksums are integrity checks, not package signatures.

## Try the workflow without publishing

```sh
gh workflow run release-ci.yml --repo fossgap/open-music-player --ref main
```

A manual run from `main` builds and verifies artifacts; its publish job is skipped.

To reproduce locally with Docker:

```sh
bash scripts/ci.sh check
bash scripts/ci.sh release
for target in debian kali fedora arch appimage; do
    bash scripts/check-package-installation.sh "$target" dist/release
done
```

Set `OPEN_MUSIC_CI_OUTPUT` to a new directory for another release build; the
default is `dist/release`. The installation checker accepts that directory as its
second argument. It installs software only inside disposable Docker containers.
These commands do not publish anything.

## Publish a version

1. Update `Cargo.toml`, the package entry in `Cargo.lock`, and
   `packaging/PKGBUILD` to the intended version. RPM and binary-package versions
   are read from Cargo automatically.
2. Commit the changes and confirm Test CI and a manual release run pass.
3. Create and push the matching annotated tag, for example:

```sh
git tag -a v0.1.4 -m "Open Music Player 0.1.4"
git push origin v0.1.4
```

After all checks pass, GitHub creates a release with generated notes and the
tested artifacts. It uses the repository's provided `GITHUB_TOKEN`; no personal
token or service-account credentials are required.
