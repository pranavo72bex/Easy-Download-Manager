"""A tray host, as a panel is one: it owns a StatusNotifierHost name and
registers it with the StatusNotifierWatcher, again whenever the watcher
comes back. dm hides into the tray only where a host is registered (a
watcher with no panel drawing its items shows nothing), so the checks that
stand in for a panel stand in for this part of one too."""
import os

from gi.repository import Gio, GLib  # noqa: E402

WATCHER = "org.kde.StatusNotifierWatcher"
NAME = f"org.kde.StatusNotifierHost-{os.getpid()}"


def register(connection, *_):
    try:
        connection.call_sync(WATCHER, "/StatusNotifierWatcher", WATCHER,
                             "RegisterStatusNotifierHost", GLib.Variant("(s)", (NAME,)),
                             None, Gio.DBusCallFlags.NONE, 2000, None)
    except GLib.Error:
        pass  # no watcher yet: registered when one appears


def owned(connection, *_):
    Gio.bus_watch_name_on_connection(connection, WATCHER, Gio.BusNameWatcherFlags.NONE,
                                     register, None)


Gio.bus_own_name(Gio.BusType.SESSION, NAME, Gio.BusNameOwnerFlags.NONE, None, owned, None)
GLib.MainLoop().run()
