#!/bin/sh
# KWin as a Wayland compositor on a virtual output, then the check, under one
# session bus. Run as an ordinary user.
XDG_RUNTIME_DIR="/tmp/xdg-$(id -u)"
export XDG_RUNTIME_DIR
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
export QT_QPA_PLATFORM=offscreen KWIN_COMPOSE=Q LIBGL_ALWAYS_SOFTWARE=1 XDG_SESSION_TYPE=wayland
kwin_wayland --virtual --no-lockscreen --no-global-shortcuts --socket wayland-0 --width 1280 --height 800 >/shots/kwin.log 2>&1 &
for _ in $(seq 1 60); do [ -S "$XDG_RUNTIME_DIR/wayland-0" ] && break; sleep 0.5; done
for _ in $(seq 1 40); do
  gdbus call --session -d org.kde.KWin -o /KWin -m org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1 && break; sleep 0.5
done
export WAYLAND_DISPLAY=wayland-0
unset QT_QPA_PLATFORM
python3 "$(dirname "$0")/kde_close_check.py"
