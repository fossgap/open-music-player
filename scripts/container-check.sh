#!/usr/bin/env bash
set -euo pipefail
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 scripts/check-locales.py
bash scripts/ui-test.sh
xvfb-run -a dbus-run-session -- env GDK_BACKEND=x11 GSK_RENDERER=cairo python3 scripts/smoke-test.py
