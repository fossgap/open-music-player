FROM rust:1.89-bookworm

RUN rustup component add rustfmt clippy

RUN apt-get update && apt-get install -y --no-install-recommends \
    libdbus-1-dev libgtk-4-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-pulseaudio \
    ffmpeg pkg-config ca-certificates dbus-x11 xvfb xauth xdotool \
    python3 imagemagick make rpm desktop-file-utils dpkg-dev zstd \
    curl patchelf file adwaita-icon-theme glib-networking fonts-dejavu-core \
    librsvg2-dev libgirepository1.0-dev gstreamer1.0-tools shellcheck \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY . .
CMD ["bash", "scripts/container-check.sh"]
