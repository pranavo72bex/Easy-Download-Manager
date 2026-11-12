# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# dm built from this checkout, run headless by the QA suite; Chromium with the
# unpacked extension for the hand-off checks. Built from the repository root:
#   docker build -f qa/app/Dockerfile .
FROM rust:1-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config python3 zip curl ca-certificates \
      # What the window (GPUI) builds against.
      libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libxcb1-dev libx11-xcb-dev libvulkan-dev \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_TARGET_DIR=/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
# The extension's tools, before the source: kept by the build cache while
# only the source changes. wasm-bindgen's version is the one the lock pins.
COPY browser/Cargo.lock /tmp/browser.lock
RUN rustup target add wasm32-unknown-unknown \
    && v=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' /tmp/browser.lock) \
    && cargo install wasm-bindgen-cli --version "$v" --locked && rm -rf /usr/local/cargo/registry/src
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/target \
    cd desktop && cargo build -p dm-gpui --locked && cp /target/debug/dm-gpui /usr/local/bin/dm-gpui
# The extension, as scripts/package.sh builds it (WebAssembly and both zips).
# package.sh reads the build from browser/target, so not the shared target dir.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    env -u CARGO_TARGET_DIR sh scripts/package.sh && mkdir -p /out && cp -R dist /out/dist

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 procps lsof ca-certificates curl unzip xvfb xauth dbus chromium \
      # What the window links and opens, as the .deb names it (dm-gpui's Cargo.toml).
      libxcb1 libxkbcommon0 libxkbcommon-x11-0 libfreetype6 libfontconfig1 \
      libvulkan1 libegl1 libgl1-mesa-dri libwayland-client0 libwayland-egl1 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/dm-gpui /usr/local/bin/dm-gpui
COPY --from=build /out/dist /repo/dist
COPY scripts /repo/scripts
COPY qa /repo/qa
# Not root: a folder made read-only must be read-only to dm.
RUN useradd -m -u 1000 qa && mkdir -p /downloads /reports && chown qa /downloads /reports
USER qa
ENV DM_BINARY=/usr/local/bin/dm-gpui CHROME_PATH=/usr/bin/chromium
WORKDIR /repo
CMD ["python3", "-u", "qa/tests/run.py"]
