# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# A Linux with everything the client needs to build and run. Tray integration
# uses the session's StatusNotifier host and is tested in a desktop session.
FROM rust:1-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl file \
      xvfb xauth x11-utils dbus procps lsof git fonts-noto-core fonts-noto-cjk python3 ca-certificates \
      # xclip: the stress check's clipboard phase copies from another program, on Xvfb's clipboard.
      xclip \
      # What the window opens at run time, as the .deb declares: Vulkan where
      # there is a driver, else GL through EGL (Mesa), and Wayland's client.
      libxkbcommon-x11-0 libxkbcommon0 libvulkan1 libegl1 libgl1-mesa-dri \
      libwayland-client0 libwayland-egl1 \
      # WebKitGTK's JavaScriptCore, which dm's .deb recommends: YouTube's checks
      # and a proxy script run in it, nothing fetched (gaps row 164).
      libjavascriptcoregtk-4.1-0 \
      # What the window (dm-gpui, GPUI) builds against.
      libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libxcb1-dev libx11-xcb-dev libvulkan-dev \
    && rm -rf /var/lib/apt/lists/*

# No web view: the window is GPUI, drawn by Mesa under Xvfb.
ENV CARGO_TARGET_DIR=/target \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_INCREMENTAL=0

WORKDIR /work
