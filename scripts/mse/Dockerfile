# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# A libtorrent peer for dm's encryption tests (scripts/mse-interop.sh):
# libtorrent is the library most BitTorrent clients are built on, with its
# own implementation of encrypted connections (MSE/PE).
FROM debian:12-slim
RUN apt-get update && apt-get install -y --no-install-recommends python3 python3-libtorrent && rm -rf /var/lib/apt/lists/*
COPY peer.py /peer.py
