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


def make_png(path, width, height, noise_every_n_rows):
    """Write a PNG whose size is controlled by how many rows are random noise."""
    import os

    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf

    width, height, every = int(width), int(height), int(noise_every_n_rows)
    rowstride = width * 3
    pixels = bytearray(rowstride * height)
    for y in range(height):
        row = os.urandom(rowstride) if y % every == 0 else bytes([y % 256, 90, 200]) * width
        pixels[y * rowstride:(y + 1) * rowstride] = row
    pixbuf = GdkPixbuf.Pixbuf.new_from_bytes(
        GLib.Bytes.new(bytes(pixels)), GdkPixbuf.Colorspace.RGB, False, 8, width, height, rowstride)
    pixbuf.savev(path, "png", [], [])


def set_file(mimetype, path):
    data = open(path, "rb").read()
    hold_clipboard(lambda clipboard: clipboard.set_content(
        Gdk.ContentProvider.new_for_bytes(mimetype, GLib.Bytes.new(data))))


COMMANDS = {
    "set-text": set_text,
    "make-png": make_png,
    "set-file": set_file,
}

if __name__ == "__main__":
    command, *args = sys.argv[1:]
    COMMANDS[command](*args)
