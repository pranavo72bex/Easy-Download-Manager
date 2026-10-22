#!/bin/sh
# A headless sway session for scripts/linux/wayland_drag_check.py, run as an
# ordinary user (sway will not run as root).
export XDG_RUNTIME_DIR=/tmp/xdg-$(id -u)
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
printf 'output HEADLESS-1 resolution 1280x800\nseat seat0 fallback true\n' > "$XDG_RUNTIME_DIR/sway.conf"
sway -c "$XDG_RUNTIME_DIR/sway.conf" >/shots/sway.log 2>&1 &
for _ in $(seq 1 50); do
  ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null 2>&1 && break
  sleep 0.2
done
WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -1)")
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.* | head -1)
export WAYLAND_DISPLAY SWAYSOCK
python3 "$(dirname "$0")/wayland_drag_check.py"
