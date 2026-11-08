Name: open-music-player
Version: %{app_version}
Release: 1%{?dist}
Summary: A native home for music, podcasts, and radio
License: GPL-3.0-only
URL: https://github.com/fossgap/open-music-player
Requires: gtk4 >= 4.8
Requires: dbus-libs
Requires: gstreamer1-plugins-base, gstreamer1-plugins-good, ffmpeg-free, ca-certificates
Requires: adwaita-icon-theme, glib-networking
Recommends: gstreamer1-plugins-bad-free, python3, python3-pip, libsecret

%description
A Rust and GTK music player with local libraries, podcasts, radio,
playlists, MPRIS controls, and a mini player.

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/root/. %{buildroot}/

%files
/usr/bin/open-music-player
/usr/bin/open-music-setup-youtube
/usr/share/open-music-player/youtube-requirements.txt
/usr/share/applications/io.github.openmusic.Player.desktop
/usr/share/icons/hicolor/scalable/apps/io.github.openmusic.Player.svg
/usr/share/metainfo/io.github.openmusic.Player.metainfo.xml
%license /usr/share/licenses/open-music-player/LICENSE
%doc /usr/share/doc/open-music-player

%changelog
* Wed Nov 04 2026 Open Music Player contributors - 0.1.0-1
- Initial package.
