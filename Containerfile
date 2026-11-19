FROM docker.io/library/rust:1.90-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        dbus-daemon libgtk-4-dev pkg-config \
        xauth xdotool xvfb \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

WORKDIR /app
