Name: open-audio-player
Version: 0.1.0
Release: 1%{?dist}
Summary: A native home for music, podcasts, and radio
License: Unspecified
Requires: gtk4 >= 4.8
Requires: gstreamer1-plugins-base, gstreamer1-plugins-good, ffmpeg, ca-certificates
Recommends: gstreamer1-plugin-libav, yt-dlp

%description
A Rust and GTK music player with local libraries, podcasts, radio,
playlists, MPRIS controls, and a mini player.

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/root/. %{buildroot}/

%files
/usr/bin/open-audio-player
/usr/share/applications/io.github.openaudio.Player.desktop
/usr/share/icons/hicolor/scalable/apps/io.github.openaudio.Player.svg
/usr/share/metainfo/io.github.openaudio.Player.metainfo.xml

%changelog
* Wed Nov 04 2026 Open Audio Player contributors - 0.1.0-1
- Initial package.
