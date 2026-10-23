"""A StatusNotifierWatcher, as a desktop's panel provides one: tray items
register with it, hosts register with it, and it says what is listed and
whether any host is there. A stand-in for `status-notifier-watcher` where
that cannot reach the session bus."""
from gi.repository import Gio, GLib  # noqa: E402

NAME = "org.kde.StatusNotifierWatcher"
XML = """<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
</interface></node>"""
items, hosts = [], []


def call(connection, sender, path, iface, method, params, invocation):
    arg = params.unpack()[0]
    if method == "RegisterStatusNotifierItem":
        # An item is named by its bus name, or a path on the sender's.
        entry = arg if arg.startswith(":") or "." in arg else sender + arg
        if entry not in items:
            items.append(entry)
            connection.emit_signal(None, path, iface, "StatusNotifierItemRegistered",
                                   GLib.Variant("(s)", (entry,)))
    else:
        hosts.append(arg)
        connection.emit_signal(None, path, iface, "StatusNotifierHostRegistered", None)
    invocation.return_value(None)


def prop(connection, sender, path, iface, name):
    if name == "RegisteredStatusNotifierItems":
        return GLib.Variant("as", items)
    if name == "IsStatusNotifierHostRegistered":
        return GLib.Variant("b", bool(hosts))
    return GLib.Variant("i", 0)


def acquired(connection, name):
    info = Gio.DBusNodeInfo.new_for_xml(XML)
    connection.register_object("/StatusNotifierWatcher", info.interfaces[0], call, prop, None)


Gio.bus_own_name(Gio.BusType.SESSION, NAME, Gio.BusNameOwnerFlags.NONE, acquired, None, None)
GLib.MainLoop().run()
