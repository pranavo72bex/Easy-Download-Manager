# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Pages and media the video-page checks read, made once at build
# time with ffmpeg (no network when the suite runs).
FROM python:3.12-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg && rm -rf /var/lib/apt/lists/*
COPY mockvideo/make.sh /make.sh
RUN sh /make.sh /site
COPY mockvideo/serve.py /serve.py
EXPOSE 8090
CMD ["python3", "-u", "/serve.py"]
