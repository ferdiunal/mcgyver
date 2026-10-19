"""A fullscreen GTK window offering one file URI as a drag source: pressing anywhere starts a drag."""
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, Gtk  # noqa: E402

uri = sys.argv[1]
# A busy, colourful backdrop so the overlay's glass and blur can be judged.
css = b"""
window, button {
    background-image: linear-gradient(135deg, #ff5f6d 0%, #ffc371 30%, #47cacc 62%, #5b4bdb 100%);
    border: none;
    box-shadow: none;
}
button label { font-size: 84px; font-weight: 900; color: rgba(255, 255, 255, 0.92); }
"""
provider = Gtk.CssProvider()
provider.load_from_data(css)
Gtk.StyleContext.add_provider_for_screen(
    Gdk.Screen.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER
)
win = Gtk.Window(title="drag source")
win.move(60, 60)
button = Gtk.Button(label="mcgyver\nliquid glass\nDRAG ME")
button.set_size_request(220, 120)
button.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY)
button.drag_source_add_uri_targets()
button.connect("drag-data-get", lambda w, ctx, data, info, t: data.set_uris([uri]))
win.add(button)
win.connect("destroy", Gtk.main_quit)
win.fullscreen()
win.show_all()
Gtk.main()
