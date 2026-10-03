"""Test clients for the GNOME extension harness. Only ever run inside nested-shell.sh."""
import os
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk  # noqa: E402


def hold_clipboard(fill, hold_seconds=3.0):
    """Open a window (so the app has a seat serial), call fill(clipboard), keep serving, quit."""

    def activate(app):
        window = Gtk.ApplicationWindow(application=app)
        clipboard = window.get_display().get_clipboard()

        window.present()
        GLib.timeout_add(500, lambda: (fill(clipboard), False)[1])
        GLib.timeout_add(int(hold_seconds * 1000), lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id="dev.pasta.GnomeTestClip", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def provider(mimetype, data):
    return Gdk.ContentProvider.new_for_bytes(mimetype, GLib.Bytes.new(data))


def set_text(text):
    hold_clipboard(lambda clipboard: clipboard.set(text))


def get_text():
    """Print the clipboard's text, or nothing if it holds none."""

    def activate(app):
        window = Gtk.ApplicationWindow(application=app)
        window.present()

        def done(clipboard, result):
            try:
                print(clipboard.read_text_finish(result) or "", flush=True)
            except GLib.Error:
                pass
            app.quit()

        GLib.timeout_add(500, lambda: (window.get_display().get_clipboard().read_text_async(None, done), False)[1])
        GLib.timeout_add_seconds(5, lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id="dev.pasta.GnomeTestClip", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def set_file(mimetype, path):
    data = open(path, "rb").read()
    hold_clipboard(lambda clipboard: clipboard.set_content(provider(mimetype, data)))


def set_file_reference(path):
    """Copy a file the way Nautilus does: a reference, not the bytes."""
    uri = "file://" + os.path.abspath(path)
    hold_clipboard(lambda clipboard: clipboard.set_content(Gdk.ContentProvider.new_union([
        provider("x-special/gnome-copied-files", ("copy\n" + uri).encode()),
        provider("text/uri-list", (uri + "\r\n").encode()),
        provider("text/plain;charset=utf-8", uri.encode()),
    ])))


def make_png(path, width, height):
    """Write a PNG with some noise rows so its size is non-trivial."""
    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf

    width, height = int(width), int(height)
    rowstride = width * 3
    pixels = bytearray(rowstride * height)
    for y in range(height):
        row = os.urandom(rowstride) if y % 5 == 0 else bytes([y % 256, 90, 200]) * width
        pixels[y * rowstride:(y + 1) * rowstride] = row
    GdkPixbuf.Pixbuf.new_from_bytes(
        GLib.Bytes.new(bytes(pixels)), GdkPixbuf.Colorspace.RGB, False, 8, width, height, rowstride
    ).savev(path, "png", [], [])


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


def window(seconds):
    """Show a plain window for `seconds`; prints WINDOW shown once it is presented."""

    def activate(app):
        win = Gtk.ApplicationWindow(application=app)
        win.present()
        print("WINDOW shown", flush=True)
        GLib.timeout_add_seconds(int(seconds), lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id="dev.pasta.GnomeTestWindow", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def impostor_window(seconds):
    """Own com.pasta.Launcher without being pasta-launcher, and show a window."""
    Gio.bus_own_name(
        Gio.BusType.SESSION, "com.pasta.Launcher", Gio.BusNameOwnerFlags.NONE,
        None, lambda *_: print("IMPOSTOR owns com.pasta.Launcher", flush=True), None)
    window(seconds)


COMMANDS = {
    "set-text": set_text,
    "get-text": get_text,
    "set-file": set_file,
    "set-file-reference": set_file_reference,
    "make-png": make_png,
    "impostor": impostor,
    "poke-bridge": poke_bridge,
    "window": window,
    "impostor-window": impostor_window,
}

if __name__ == "__main__":
    command, *args = sys.argv[1:]
    COMMANDS[command](*args)
