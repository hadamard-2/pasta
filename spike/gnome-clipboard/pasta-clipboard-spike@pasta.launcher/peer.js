// Identifies the process behind a D-Bus name by its executable path, without
// blocking the compositor: every bus call here is asynchronous.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

function callBus(method, args, replyType) {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call(
            'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
            method, args, new GLib.VariantType(replyType),
            Gio.DBusCallFlags.NONE, 2000, null,
            (connection, result) => {
                try {
                    resolve(connection.call_finish(result).deepUnpack()[0]);
                } catch (e) {
                    reject(e);
                }
            });
    });
}

/** Absolute executable path of the process that owns `busName` (unique or well-known). */
export async function executableOf(busName) {
    const pid = await callBus('GetConnectionUnixProcessID', new GLib.Variant('(s)', [busName]), '(u)');
    return GLib.file_read_link(`/proc/${pid}/exe`);
}

/** Current unique owner of a well-known name and that owner's executable. */
export async function ownerOf(wellKnownName) {
    const unique = await callBus('GetNameOwner', new GLib.Variant('(s)', [wellKnownName]), '(s)');
    return {unique, exe: await executableOf(unique)};
}

/** Basename of an executable path; a binary replaced on disk while running reads back with " (deleted)". */
export function exeBasename(path) {
    return GLib.path_get_basename(path.replace(/ \(deleted\)$/, ''));
}
