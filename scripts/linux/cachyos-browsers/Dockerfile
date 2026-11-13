# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# CachyOS with Brave, Google Chrome, Microsoft Edge and Firefox, and dm and
# its extension built from this checkout, for the extension hand-off checks
# (scripts/chrome-check.py, scripts/firefox-check.py native) in each browser.
# Built from the repository root by run.sh. x86-64 only (Chrome and Edge have
# no Linux arm64 build); on an Apple Silicon Mac it runs under emulation.
#   docker build --platform linux/amd64 -f scripts/linux/cachyos-browsers/Dockerfile .
FROM --platform=linux/amd64 rust:1-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config python3 zip curl ca-certificates \
      # What the window (GPUI) builds against.
      libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libxcb1-dev libx11-xcb-dev libvulkan-dev \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_TARGET_DIR=/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
COPY browser/Cargo.lock /tmp/browser.lock
RUN rustup target add wasm32-unknown-unknown \
    && v=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' /tmp/browser.lock) \
    && cargo install wasm-bindgen-cli --version "$v" --locked && rm -rf /usr/local/cargo/registry/src
WORKDIR /src
COPY . .
RUN --mount=type=cache,id=dm-amd64-registry,target=/usr/local/cargo/registry --mount=type=cache,id=dm-amd64-target,target=/target \
    cd desktop && cargo build -p dm-gpui --locked && cp /target/debug/dm-gpui /usr/local/bin/dm-gpui
RUN --mount=type=cache,id=dm-amd64-registry,target=/usr/local/cargo/registry \
    env -u CARGO_TARGET_DIR sh scripts/package.sh && mkdir -p /out && cp -R dist /out/dist

FROM --platform=linux/amd64 cachyos/cachyos:latest
ARG BRAVE=1.98.62
# pacman's own sandbox and free-space check do not work under emulation.
RUN sed -i '/^CheckSpace/d; s/^\[options\]/[options]\nDisableSandbox/' /etc/pacman.conf \
 && pacman -Sy --noconfirm --needed python xorg-server-xvfb xorg-xauth mesa gtk3 libxkbcommon-x11 libxcb \
      xcb-util-cursor fontconfig freetype2 vulkan-icd-loader ttf-dejavu libx11 libxcursor libxrandr libxi libxinerama libglvnd wayland \
      curl unzip libarchive nss alsa-lib at-spi2-core libcups libdrm pango cairo libxcomposite \
      libxdamage libxfixes libxext libxshmfence dbus libsecret procps-ng ffmpeg firefox \
 && pacman -Scc --noconfirm
# Brave as Brave publishes it; Chrome and Edge from their vendors' .deb (what
# the AUR's google-chrome and microsoft-edge-stable-bin repackage).
RUN curl -fsSL -o /tmp/brave.zip https://github.com/brave/brave-browser/releases/download/v${BRAVE}/brave-browser-${BRAVE}-linux-amd64.zip \
 && mkdir -p /opt/brave.com/brave && unzip -q /tmp/brave.zip -d /opt/brave.com/brave && rm /tmp/brave.zip \
 && chmod -R a+rX /opt/brave.com/brave && ln -s /opt/brave.com/brave/brave /usr/bin/brave-browser
RUN curl -fsSL -o /tmp/c.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
 && cd /tmp && bsdtar -xf c.deb data.tar.xz && bsdtar -xf data.tar.xz -C / && rm -f c.deb data.tar.xz
RUN f=$(curl -fsSL https://packages.microsoft.com/repos/edge/dists/stable/main/binary-amd64/Packages \
        | awk '/^Package: microsoft-edge-stable$/{p=1} p&&/^Filename:/{print $2; p=0}' | sort -V | tail -1) \
 && curl -fsSL -o /tmp/e.deb "https://packages.microsoft.com/repos/edge/$f" \
 && cd /tmp && bsdtar -xf e.deb data.tar.xz && bsdtar -xf data.tar.xz -C / && rm -f e.deb data.tar.xz
COPY --from=build /usr/local/bin/dm-gpui /usr/local/bin/dm-gpui
COPY --from=build /out/dist /repo/dist
COPY scripts /repo/scripts
ENV DM_BINARY=/usr/local/bin/dm-gpui DM_FIREFOX_CACHE=/opt/ff
WORKDIR /repo
