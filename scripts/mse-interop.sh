#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# dm against another implementation of encrypted peer connections (gaps row
# 150): builds a libtorrent peer's image (scripts/mse/), then runs the tests
# that download from it and seed to it, each side taking only encrypted
# connections (desktop/crates/dm-core/tests/mse_interop.rs). Needs Docker.
# Everything stays on this computer: a container's port is published on
# loopback only, and no session has DHT, local discovery or port mapping.
set -e
cd "$(dirname "$0")/.."
docker build -q -t dm-libtorrent scripts/mse >/dev/null
cd desktop
cargo test -p dm-core --test mse_interop -- --ignored --nocapture
