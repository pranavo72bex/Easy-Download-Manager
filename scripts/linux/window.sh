# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.
# shellcheck shell=sh disable=SC2034  # `binary` is for the script that sources this

# Sourced by the Linux checks (scripts/linux-*.sh) from the repository's top:
# which of dm's windows they build and drive, as DM_WINDOW names it. Sets
# `binary`: dm-gpui, the GPUI window (DM_WINDOW=gpui, the default), or
# dm-desktop, the egui window (DM_WINDOW=egui).
case "${DM_WINDOW:-gpui}" in
  gpui) binary=dm-gpui ;;
  egui) binary=dm-desktop ;;
  *) echo "DM_WINDOW is gpui or egui, not $DM_WINDOW"; exit 1 ;;
esac
