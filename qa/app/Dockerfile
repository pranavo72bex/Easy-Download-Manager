# dm built from this checkout, run headless by the QA suite; Chromium with the
# unpacked extension for the hand-off checks. Built from the repository root:
#   docker build -f qa/app/Dockerfile .
FROM rust:1-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config python3 zip curl ca-certificates \
      libxkbcommon-x11-0 libx11-dev libxcursor-dev libxi-dev libxkbcommon-dev libwayland-dev \
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
    cd desktop && cargo build -p dm-desktop --locked && cp /target/debug/dm-desktop /usr/local/bin/dm-desktop
# The extension, as scripts/package.sh builds it (WebAssembly and both zips).
# package.sh reads the build from browser/target, so not the shared target dir.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    env -u CARGO_TARGET_DIR sh scripts/package.sh && mkdir -p /out && cp -R dist /out/dist

FROM debian:bookworm-slim
ARG YTDLP=2026.10.26
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 procps lsof ca-certificates curl unzip xvfb xauth dbus chromium \
      libxkbcommon-x11-0 libegl1 libgl1 libgl1-mesa-dri libx11-6 libxcursor1 libx11-xcb1 libxi6 \
      libxkbcommon0 libwayland-client0 libwayland-egl1 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
# yt-dlp pinned: the release zipapp, run by the system's Python.
RUN curl -fsSL -o /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP}/yt-dlp \
    && chmod +x /usr/local/bin/yt-dlp && yt-dlp --version
COPY --from=build /usr/local/bin/dm-desktop /usr/local/bin/dm-desktop
COPY --from=build /out/dist /repo/dist
COPY scripts /repo/scripts
COPY qa /repo/qa
# Not root: a folder made read-only must be read-only to dm.
RUN useradd -m -u 1000 qa && mkdir -p /downloads /reports && chown qa /downloads /reports
USER qa
ENV DM_BINARY=/usr/local/bin/dm-desktop DM_YTDLP=/usr/local/bin/yt-dlp CHROME_PATH=/usr/bin/chromium
WORKDIR /repo
CMD ["python3", "-u", "qa/tests/run.py"]
