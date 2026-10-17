#!/usr/bin/env python3
"""A window titled "drag me" that a real X11 drag starts from, carrying the
file given on the command line as a text/uri-list, as a file manager does."""
import pathlib
import sys

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gdk, Gtk  # noqa: E402

uri = pathlib.Path(sys.argv[1]).resolve().as_uri()

window = Gtk.Window(title="drag me")
window.set_default_size(160, 120)
window.move(20, 20)
box = Gtk.EventBox()
box.add(Gtk.Label(label="drag me"))
box.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY)
box.drag_source_add_uri_targets()
box.connect("drag-data-get", lambda _w, _c, data, _i, _t: data.set_uris([uri]))
window.add(box)
window.connect("destroy", Gtk.main_quit)
window.show_all()
Gtk.main()
