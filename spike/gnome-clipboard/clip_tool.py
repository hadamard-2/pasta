"""Test clients for the GNOME clipboard spike. Only ever run inside nested-shell.sh."""
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk  # noqa: E402


def hold_clipboard(fill, hold_seconds=3.0):
    """Open a window (so the app has a seat serial), call fill(clipboard), keep serving, quit."""

    def activate(app):
        window = Gtk.ApplicationWindow(application=app)
        window.present()
        clipboard = window.get_display().get_clipboard()
        GLib.timeout_add(500, lambda: (fill(clipboard), False)[1])
        GLib.timeout_add(int(hold_seconds * 1000), lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id="dev.pasta.SpikeClip", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def set_text(text):
    hold_clipboard(lambda clipboard: clipboard.set(text))


COMMANDS = {
    "set-text": set_text,
}

if __name__ == "__main__":
    command, *args = sys.argv[1:]
    COMMANDS[command](*args)
