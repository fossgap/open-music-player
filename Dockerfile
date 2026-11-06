FROM rust:1.85-bookworm

RUN rustup component add rustfmt clippy

RUN apt-get update && apt-get install -y --no-install-recommends \
    libdbus-1-dev libgtk-4-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-pulseaudio \
    ffmpeg pkg-config ca-certificates dbus-x11 xvfb xauth xdotool \
    python3 imagemagick make rpm desktop-file-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY . .
CMD ["bash", "scripts/container-check.sh"]
