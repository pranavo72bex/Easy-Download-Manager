# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.
# shellcheck shell=sh disable=SC2034  # `binary` is for the script that sources this

# Sourced by the Linux checks (scripts/linux-*.sh) from the repository's top:
# the window they build and drive. Sets `binary` to edm-gpui, EDM's window. The
# older egui window (EDM_WINDOW=egui) is gone from the tree (docs/decisions.md,
# 2026-11-12), so EDM_WINDOW, if set, can only name this one.
case "${EDM_WINDOW:-gpui}" in
  gpui) binary=edm-gpui ;;
  *) echo "EDM_WINDOW is gpui (the egui window was removed), not $EDM_WINDOW"; exit 1 ;;
esac
