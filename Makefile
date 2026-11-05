PREFIX ?= /usr
DESTDIR ?=
APP_ID = io.github.openaudio.Player

.PHONY: build run test check smoke install uninstall deb rpm arch appimage
build:
	cargo build --release --locked
run:
	cargo run --locked
test:
	cargo test --locked
check:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	python3 scripts/check-locales.py
smoke:
	cargo build --locked
	xvfb-run -a dbus-run-session -- env GDK_BACKEND=x11 GSK_RENDERER=cairo python3 scripts/smoke-test.py
install:
	install -Dm755 target/release/open-audio-player $(DESTDIR)$(PREFIX)/bin/open-audio-player
	install -Dm644 packaging/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 resources/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 packaging/$(APP_ID).metainfo.xml $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/open-audio-player $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
deb rpm arch appimage: build
	bash scripts/package.sh $@
