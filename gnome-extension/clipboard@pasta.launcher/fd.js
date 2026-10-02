// File descriptors received over D-Bus belong to this process once stolen
// from their list; anything not handed to a stream must be closed here.
import GLib from 'gi://GLib';

export function closeQuietly(fd) {
    try {
        GLib.close(fd);
    } catch (e) {
        console.warn(`pasta-clipboard: closing fd ${fd} failed: ${e.message}`);
    }
}

/** Whether `index` names an entry of `fds` that has not been used yet. */
export function isUsableIndex(index, fds, used) {
    return Number.isInteger(index) && index >= 0 && index < fds.length && !used.has(index);
}
