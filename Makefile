PREFIX ?= /usr
DESTDIR ?=
APP_ID = io.github.openmusic.Player
BINARY ?= $(or $(CARGO_TARGET_DIR),target)/release/open-music-player

.PHONY: build run test check smoke ui-test install uninstall deb rpm arch arch-source appimage
build:
	cargo build --release --locked
run:
	cargo run --locked
test:
	cargo test --locked
	python3 scripts/test-youtube-music.py
check:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	python3 scripts/check-locales.py
smoke:
	cargo build --locked
	xvfb-run -a dbus-run-session -- env GDK_BACKEND=x11 GSK_RENDERER=cairo python3 scripts/smoke-test.py
ui-test:
	bash scripts/ui-test.sh
install:
	install -Dm755 $(BINARY) $(DESTDIR)$(PREFIX)/bin/open-music-player
	install -Dm755 scripts/setup-youtube-music.sh $(DESTDIR)$(PREFIX)/bin/open-music-setup-youtube
	install -Dm644 resources/youtube-requirements.txt $(DESTDIR)$(PREFIX)/share/open-music-player/youtube-requirements.txt
	install -Dm644 packaging/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 resources/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 packaging/$(APP_ID).metainfo.xml $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/open-music-player/LICENSE
	install -Dm644 README.md $(DESTDIR)$(PREFIX)/share/doc/open-music-player/README.md
	install -Dm644 packaging/copyright $(DESTDIR)$(PREFIX)/share/doc/open-music-player/copyright
	cp -r docs/wiki $(DESTDIR)$(PREFIX)/share/doc/open-music-player/
uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/open-music-player $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
	rm -f $(DESTDIR)$(PREFIX)/bin/open-music-setup-youtube $(DESTDIR)$(PREFIX)/share/open-music-player/youtube-requirements.txt
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/open-music-player $(DESTDIR)$(PREFIX)/share/doc/open-music-player
deb rpm arch appimage: build
	bash scripts/package.sh $@
arch-source:
	bash scripts/package.sh $@
