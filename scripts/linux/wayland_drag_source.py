#!/usr/bin/env python3
"""A window that offers a link to drag, as a browser offers one, and takes
files dropped on it, as a file manager does: run under Wayland by
wayland_drag_check.py. GTK sends the link as text/uri-list and as text (the
targets Firefox also offers); what is dropped on it is written to
/shots/received.txt."""
import sys
import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Gdk  # noqa: E402

LINK = sys.argv[1]
window = Gtk.Window(title="drag source")
window.set_default_size(400, 300)
button = Gtk.Button(label="Drag this link onto dm")
button.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY)
button.drag_source_add_uri_targets()
button.drag_source_add_text_targets()


def give(_widget, _context, data, _info, _time):
    # Each format as it is asked for: the address as a URI list, or as text.
    if "uri" in data.get_target().name():
        data.set_uris([LINK])
    else:
        data.set_text(LINK, -1)


button.connect("drag-data-get", give)

# Files dropped here, as a file manager takes them.
window.drag_dest_set(Gtk.DestDefaults.ALL, [], Gdk.DragAction.COPY)
window.drag_dest_add_uri_targets()


def received(_widget, _context, _x, _y, data, _info, _time):
    with open("/shots/received.txt", "a") as out:
        for uri in data.get_uris() or []:
            out.write(uri + "\n")


window.connect("drag-data-received", received)
window.add(button)
window.connect("destroy", Gtk.main_quit)
window.show_all()
Gtk.main()
