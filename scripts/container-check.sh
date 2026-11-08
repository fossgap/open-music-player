#!/usr/bin/env bash
set -euo pipefail
make check
make test
cargo build --locked
shellcheck scripts/package.sh scripts/ci.sh scripts/ci-inside.sh scripts/fetch-appimage-tools.sh scripts/check-package-installation.sh tests/package-install/*.sh
bash scripts/ui-test.sh
xvfb-run -a dbus-run-session -- env GDK_BACKEND=x11 GSK_RENDERER=cairo python3 scripts/smoke-test.py
