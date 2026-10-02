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


IMPOSTORS = {
    "pasta": (
        "com.pasta.Launcher",
        "/com/pasta/Launcher/Clipboard",
        '<node><interface name="com.pasta.Launcher.Clipboard1">'
        '<method name="Offer"><arg type="as" direction="in"/><arg type="a{sh}" direction="out"/></method>'
        "</interface></node>",
    ),
    "bridge": (
        "com.pasta.Launcher.ShellBridge",
        "/com/pasta/Launcher/ShellBridge",
        '<node><interface name="com.pasta.Launcher.ShellBridge1">'
        '<method name="SetClipboard"><arg type="s" direction="in"/><arg type="h" direction="in"/></method>'
        "</interface></node>",
    ),
}


def impostor(kind, seconds):
    """Own a name the real peers trust, answer every call, and report it."""
    name, path, xml = IMPOSTORS[kind]
    interface = Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0]
    loop = GLib.MainLoop()

    def on_call(_conn, _sender, _path, _iface, method, _params, invocation):
        print(f"IMPOSTOR got {method}", flush=True)
        if method == "Offer":
            invocation.return_value(GLib.Variant("(a{sh})", ({},)))
        else:
            invocation.return_value(None)

    def on_bus(connection, _name):
        connection.register_object(path, interface, on_call, None, None)

    Gio.bus_own_name(
        Gio.BusType.SESSION, name, Gio.BusNameOwnerFlags.NONE,
        on_bus, lambda *_: print(f"IMPOSTOR owns {name}", flush=True), None)
    GLib.timeout_add_seconds(int(seconds), loop.quit)
    loop.run()


def poke_bridge():
    """Call SetClipboard as a process that is not pasta-launcher."""
    import os

    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    read_end, write_end = os.pipe()
    os.write(write_end, b"poke")
    os.close(write_end)
    fds = Gio.UnixFDList.new_from_array([read_end])
    try:
        connection.call_with_unix_fd_list_sync(
            "com.pasta.Launcher.ShellBridge", "/com/pasta/Launcher/ShellBridge",
            "com.pasta.Launcher.ShellBridge1", "SetClipboard",
            GLib.Variant("(sh)", ("text/plain;charset=utf-8", 0)), None,
            Gio.DBusCallFlags.NONE, 5000, fds, None)
        print("POKE accepted", flush=True)
    except GLib.Error as error:
        print(f"POKE rejected: {error.message}", flush=True)


COMMANDS = {
    "set-text": set_text,
    "make-png": make_png,
    "set-file": set_file,
    "impostor": impostor,
    "poke-bridge": poke_bridge,
}

if __name__ == "__main__":
    command, *args = sys.argv[1:]
    COMMANDS[command](*args)
