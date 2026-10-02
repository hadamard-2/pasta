// Accepts clipboard writes from Pasta over D-Bus and applies them with St.Clipboard.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {closeQuietly, isUsableIndex} from './fd.js';
import {executableOf, exeBasename} from './peer.js';

const BRIDGE_NAME = 'com.pasta.Launcher.ShellBridge';
const BRIDGE_PATH = '/com/pasta/Launcher/ShellBridge';
const ALLOWED_CALLER_EXE = 'pasta-launcher';
const MAX_BYTES = 32 * 1024 * 1024;
const BRIDGE_XML = `<node>
  <interface name="com.pasta.Launcher.ShellBridge1">
    <method name="SetClipboard">
      <arg type="s" name="mimetype" direction="in"/>
      <arg type="h" name="data" direction="in"/>
    </method>
  </interface>
</node>`;

function joinChunks(chunks, total) {
    const out = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
        out.set(chunk, offset);
        offset += chunk.length;
    }
    return new GLib.Bytes(out);
}

/** Reads `fd` to EOF, refusing payloads over MAX_BYTES. */
function readAll(fd) {
    const input = GioUnix.InputStream.new(fd, true);
    const chunks = [];
    let total = 0;
    return new Promise((resolve, reject) => {
        const next = () => input.read_bytes_async(64 * 1024, GLib.PRIORITY_DEFAULT, null, (stream, result) => {
            let bytes;
            try {
                bytes = stream.read_bytes_finish(result);
            } catch (e) {
                input.close(null);
                reject(e);
                return;
            }
            const size = bytes.get_size();
            if (size === 0) {
                input.close(null);
                resolve(joinChunks(chunks, total));
                return;
            }
            total += size;
            if (total > MAX_BYTES) {
                input.close(null);
                reject(new Error(`payload exceeds ${MAX_BYTES} bytes`));
                return;
            }
            chunks.push(bytes.toArray());
            next();
        });
        next();
    });
}

export class ShellBridge {
    constructor() {
        this._exported = Gio.DBusExportedObject.wrapJSObject(BRIDGE_XML, this);
        this._exported.export(Gio.DBus.session, BRIDGE_PATH);
        this._nameId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BRIDGE_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    async SetClipboardAsync([mimetype, handle], invocation, fdList) {
        // Take ownership of every incoming fd first so none outlives this call.
        const fds = fdList ? fdList.steal_fds() : [];
        const closeAllExcept = keep => fds.forEach((fd, index) => {
            if (index !== keep)
                closeQuietly(fd);
        });

        let exe;
        try {
            exe = await executableOf(invocation.get_sender());
        } catch (e) {
            closeAllExcept(-1);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `could not identify caller: ${e.message}`);
            return;
        }
        if (exeBasename(exe) !== ALLOWED_CALLER_EXE) {
            closeAllExcept(-1);
            console.warn(`pasta-clipboard: rejected SetClipboard from ${exe}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `${exe} is not ${ALLOWED_CALLER_EXE}`);
            return;
        }
        if (!isUsableIndex(handle, fds, new Set())) {
            closeAllExcept(-1);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.InvalidArgs',
                `no file descriptor at index ${handle}`);
            return;
        }
        closeAllExcept(handle);

        let bytes;
        try {
            bytes = await readAll(fds[handle]);
        } catch (e) {
            console.warn(`pasta-clipboard: SetClipboard ${mimetype} read failed: ${e.message}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', e.message);
            return;
        }
        St.Clipboard.get_default().set_content(St.ClipboardType.CLIPBOARD, mimetype, bytes);
        invocation.return_value(null);
    }

    destroy() {
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
    }
}
