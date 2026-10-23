#!/bin/sh
# A headless sway session for scripts/linux/wayland_close_check.py, run as an
# ordinary user (sway will not run as root).
XDG_RUNTIME_DIR="/tmp/xdg-$(id -u)"
export XDG_RUNTIME_DIR
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
printf 'output HEADLESS-1 resolution 1280x800\nseat seat0 fallback true\n' > "$XDG_RUNTIME_DIR/sway.conf"
sway -c "$XDG_RUNTIME_DIR/sway.conf" >/shots/sway-close.log 2>&1 &
socket=""
for _ in $(seq 1 50); do
  for candidate in "$XDG_RUNTIME_DIR"/wayland-*; do
    case "$candidate" in
      *.lock|*'*') ;;
      *) socket="$candidate" ;;
    esac
  done
  [ -n "$socket" ] && break
  sleep 0.2
done
WAYLAND_DISPLAY=$(basename "$socket")
SWAYSOCK=""
for candidate in "$XDG_RUNTIME_DIR"/sway-ipc.*; do
  [ -e "$candidate" ] && SWAYSOCK="$candidate"
done
export WAYLAND_DISPLAY SWAYSOCK
python3 "$(dirname "$0")/wayland_close_check.py"
