// Accepts clipboard writes from Pasta over D-Bus and applies them with St.Clipboard.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {executableOf, exeBasename} from './peer.js';

const BRIDGE_NAME = 'com.pasta.Launcher.ShellBridge';
const BRIDGE_PATH = '/com/pasta/Launcher/ShellBridge';
const ALLOWED_CALLER_EXE = 'pasta-launcher';
const BRIDGE_XML = `<node>
  <interface name="com.pasta.Launcher.ShellBridge1">
    <method name="SetClipboard">
      <arg type="s" name="mimetype" direction="in"/>
      <arg type="h" name="data" direction="in"/>
    </method>
  </interface>
</node>`;

function readAll(fd) {
    return new Promise((resolve, reject) => {
        const input = GioUnix.InputStream.new(fd, true);
        const sink = Gio.MemoryOutputStream.new_resizable();
        sink.splice_async(input,
            Gio.OutputStreamSpliceFlags.CLOSE_SOURCE | Gio.OutputStreamSpliceFlags.CLOSE_TARGET,
            GLib.PRIORITY_DEFAULT, null,
            (stream, result) => {
                try {
                    stream.splice_finish(result);
                    resolve(stream.steal_as_bytes());
                } catch (e) {
                    reject(e);
                }
            });
    });
}

export class ShellBridge {
    constructor(log) {
        this._log = log;
        this._exported = Gio.DBusExportedObject.wrapJSObject(BRIDGE_XML, this);
        this._exported.export(Gio.DBus.session, BRIDGE_PATH);
        this._nameId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BRIDGE_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    async SetClipboardAsync([mimetype, handle], invocation, fdList) {
        // Take ownership of the incoming fds first so they are closed whatever happens next.
        const fds = fdList ? fdList.steal_fds() : [];
        const exe = await executableOf(invocation.get_sender());
        if (exeBasename(exe) !== ALLOWED_CALLER_EXE) {
            fds.forEach(fd => GLib.close(fd));
            this._log(`rejected SetClipboard from ${exe}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `${exe} is not ${ALLOWED_CALLER_EXE}`);
            return;
        }
        const bytes = await readAll(fds[handle]);
        St.Clipboard.get_default().set_content(St.ClipboardType.CLIPBOARD, mimetype, bytes);
        const sha = GLib.compute_checksum_for_bytes(GLib.ChecksumType.SHA256, bytes).slice(0, 12);
        this._log(`SetClipboard ${mimetype} ${bytes.get_size()} bytes sha256=${sha}`);
        invocation.return_value(null);
    }

    destroy() {
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
    }
}
