Name: open-music-player
Version: 0.1.0
Release: 1%{?dist}
Summary: A native home for music, podcasts, and radio
License: Unspecified
Requires: gtk4 >= 4.8
Requires: dbus-libs
Requires: gstreamer1-plugins-base, gstreamer1-plugins-good, ffmpeg, ca-certificates
Recommends: gstreamer1-plugin-libav, python3, python3-pip, libsecret

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

%changelog
* Wed Nov 04 2026 Open Music Player contributors - 0.1.0-1
- Initial package.
