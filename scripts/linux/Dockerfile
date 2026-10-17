# A Linux with everything the client needs to build and run. Tray integration
# uses the session's StatusNotifier host and is tested in a desktop session.
FROM rust:1-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl file \
      xvfb xauth x11-utils dbus procps python3 yt-dlp ca-certificates \
      libxkbcommon-x11-0 libegl1 libgl1 libgl1-mesa-dri \
      # What winit opens at run time, as the .deb declares: GTK brought
      # these along until the tray moved to D-Bus.
      libx11-6 libxcursor1 libx11-xcb1 libxi6 libxkbcommon0 \
      libwayland-client0 libwayland-egl1 \
    && rm -rf /var/lib/apt/lists/*

# No web view: the window is egui on GL, drawn by Mesa under Xvfb.
ENV CARGO_TARGET_DIR=/target \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_INCREMENTAL=0

WORKDIR /work
