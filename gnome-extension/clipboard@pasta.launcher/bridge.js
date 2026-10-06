// Accepts clipboard writes and paste requests from Pasta over D-Bus. Writes go through St.Clipboard; pastes press the shortcut through a virtual keyboard.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {closeQuietly, isUsableIndex} from './fd.js';
import {executableOf, exeBasename, pidOf} from './peer.js';

const BRIDGE_NAME = 'com.pasta.Launcher.ShellBridge';
const BRIDGE_PATH = '/com/pasta/Launcher/ShellBridge';
const ALLOWED_CALLER_EXE = 'pasta-launcher';
const MAX_BYTES = 32 * 1024 * 1024;
const FOCUS_TIMEOUT_MS = 1000;
// How long a window must have held focus before it gets the keys. Measured:
// a terminal handed the shortcut within a few ms of taking focus often pasted
// nothing; 50 ms and 150 ms always pasted.
const FOCUS_SETTLE_MS = 100;
const BRIDGE_XML = `<node>
  <interface name="com.pasta.Launcher.ShellBridge1">
    <method name="SetClipboard">
      <arg type="s" name="mimetype" direction="in"/>
      <arg type="h" name="data" direction="in"/>
    </method>
    <method name="Paste">
      <arg type="as" name="terminal_app_ids" direction="in"/>
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
        this._keyboard = null;
        this._focusWaits = new Set();
        this._settleTimers = new Map();
        this._destroyed = false;
        this._focusChangedAt = GLib.get_monotonic_time();
        this._focusChangedId = global.display.connect('notify::focus-window', () => {
            this._focusChangedAt = GLib.get_monotonic_time();
        });
        this._exported = Gio.DBusExportedObject.wrapJSObject(BRIDGE_XML, this);
        this._exported.export(Gio.DBus.session, BRIDGE_PATH);
        this._nameId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BRIDGE_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    async SetClipboardAsync(params, invocation, fdList) {
        // Every path inside answers the invocation; this catch makes sure an
        // unexpected throw still does, so the caller is never left waiting.
        try {
            await this._setClipboard(params, invocation, fdList);
        } catch (e) {
            console.warn(`pasta-clipboard: SetClipboard failed: ${e.message}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', e.message);
        }
    }

    async _setClipboard([mimetype, handle], invocation, fdList) {
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

    async PasteAsync(params, invocation) {
        // As with SetClipboardAsync: every path answers, and this catch makes
        // sure an unexpected throw does too.
        try {
            await this._paste(params, invocation);
        } catch (e) {
            console.warn(`pasta-clipboard: Paste failed: ${e.message}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', e.message);
        }
    }

    async _paste([terminalAppIds], invocation) {
        let callerPid, exe;
        try {
            callerPid = await pidOf(invocation.get_sender());
            exe = GLib.file_read_link(`/proc/${callerPid}/exe`);
        } catch (e) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `could not identify caller: ${e.message}`);
            return;
        }
        if (exeBasename(exe) !== ALLOWED_CALLER_EXE) {
            console.warn(`pasta-clipboard: rejected Paste from ${exe}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `${exe} is not ${ALLOWED_CALLER_EXE}`);
            return;
        }
        if (this._destroyed) {
            this._failShuttingDown(invocation);
            return;
        }
        const window = await this._focusAwayFrom(callerPid);
        if (this._destroyed) {
            this._failShuttingDown(invocation);
            return;
        }
        if (!window) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed',
                `no window other than the caller's took focus within ${FOCUS_TIMEOUT_MS} ms`);
            return;
        }
        const heldMs = (GLib.get_monotonic_time() - this._focusChangedAt) / 1000;
        if (heldMs < FOCUS_SETTLE_MS)
            await this._settle(Math.ceil(FOCUS_SETTLE_MS - heldMs));
        if (this._destroyed) {
            this._failShuttingDown(invocation);
            return;
        }
        if (global.display.focus_window !== window) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed',
                `focus moved within ${FOCUS_SETTLE_MS} ms of reaching the window to paste into`);
            return;
        }
        const appId = (window.get_wm_class() ?? '').toLowerCase();
        const terminal = terminalAppIds.some(id => id.toLowerCase() === appId);
        this._press(terminal
            ? [Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_v]
            : [Clutter.KEY_Control_L, Clutter.KEY_v]);
        invocation.return_value(null);
    }

    _failShuttingDown(invocation) {
        invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', 'bridge is shutting down');
    }

    /** Resolves with the focused window once it is not `pid`'s, or with null after FOCUS_TIMEOUT_MS. */
    _focusAwayFrom(pid) {
        const display = global.display;
        const usable = () => {
            const window = display.focus_window;
            return window && window.get_pid() !== pid ? window : null;
        };
        if (this._destroyed)
            return Promise.resolve(null);
        const now = usable();
        if (now)
            return Promise.resolve(now);
        return new Promise(resolve => {
            let focusId = 0;
            let timeoutId = 0;
            const finish = window => {
                if (focusId)
                    display.disconnect(focusId);
                if (timeoutId)
                    GLib.source_remove(timeoutId);
                focusId = timeoutId = 0;
                this._focusWaits.delete(finish);
                resolve(window);
            };
            focusId = display.connect('notify::focus-window', () => {
                const window = usable();
                if (window)
                    finish(window);
            });
            timeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, FOCUS_TIMEOUT_MS, () => {
                timeoutId = 0;
                finish(null);
                return GLib.SOURCE_REMOVE;
            });
            this._focusWaits.add(finish);
        });
    }

    /** Resolves after `ms`, or as soon as the bridge is destroyed. */
    _settle(ms) {
        return new Promise(resolve => {
            const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
                this._settleTimers.delete(id);
                resolve();
                return GLib.SOURCE_REMOVE;
            });
            this._settleTimers.set(id, resolve);
        });
    }

    /** Presses `keyvals` in order and releases them in reverse, through a virtual keyboard. */
    _press(keyvals) {
        if (this._destroyed)
            return;
        if (!this._keyboard) {
            const seat = global.stage.context.get_backend().get_default_seat();
            this._keyboard = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        }
        let time = GLib.get_monotonic_time();
        for (const keyval of keyvals)
            this._keyboard.notify_keyval(time++, keyval, Clutter.KeyState.PRESSED);
        for (const keyval of [...keyvals].reverse())
            this._keyboard.notify_keyval(time++, keyval, Clutter.KeyState.RELEASED);
    }

    destroy() {
        this._destroyed = true;
        for (const finish of [...this._focusWaits])
            finish(null);
        for (const [id, resolve] of [...this._settleTimers]) {
            GLib.source_remove(id);
            resolve();
        }
        this._settleTimers.clear();
        global.display.disconnect(this._focusChangedId);
        this._keyboard?.run_dispose();
        this._keyboard = null;
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
    }
}
