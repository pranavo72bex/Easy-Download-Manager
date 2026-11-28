# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# EDM with no window, for a home server (a NAS, a small computer with no screen): gaps row C9.
# Built from the repository's root:
#   docker build -f tooling/docker/Dockerfile -t EDM .
# Run, keeping EDM's lists, settings and downloads in a volume:
#   docker run -d --name EDM -v edm-data:/data EDM
# Driven from the host through the container (the control port stays on the container's own
# loopback, as on a computer):
#   docker exec EDM edm-gpui https://example.com/file.zip   # add a download
#   docker exec EDM edm-gpui list                           # what it holds
#   docker exec EDM edm-gpui pause 3 | resume 3 | cancel 3
#   docker exec -i EDM edm-gpui mcp                          # an AI client, over MCP
# Finished files are in the volume, sorted by kind under /data/EDM (Documents, Videos…).

FROM rust:1-slim-bookworm AS build
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config \
      libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libxcb1-dev libx11-xcb-dev libvulkan-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
# Fewer jobs where the builder's memory is small (Docker Desktop's VM ran out at its default):
#   docker build --build-arg JOBS=2 …
ARG JOBS=
# Linked with thin LTO in many units: the release profile's one-unit LTO needed more than the
# 7.6 GB Docker Desktop gives (killed at the last link). A little larger, as fast to run.
ENV CARGO_PROFILE_RELEASE_LTO=thin \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
COPY . .
RUN cargo build --release --locked -p edm-gpui ${JOBS:+--jobs $JOBS} \
    && cp target/release/edm-gpui /edm-gpui

FROM debian:bookworm-slim
ENV DEBIAN_FRONTEND=noninteractive
# The shared libraries the program links (the window's, never opened with --headless), and the
# certificate store reqwest's verifier reads at start; EDM's own TLS trusts its built-in roots.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates libfontconfig1 libfreetype6 libxkbcommon0 libxkbcommon-x11-0 \
      libwayland-client0 libxcb1 libx11-xcb1 libvulkan1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --home-dir /data --uid 1000 EDM
COPY --from=build /edm-gpui /usr/local/bin/edm-gpui
USER EDM
ENV HOME=/data
WORKDIR /data
VOLUME /data
# The control port on the container's own loopback, for `docker exec EDM edm-gpui list` and MCP.
ENTRYPOINT ["edm-gpui", "--headless", "--control", "0"]
