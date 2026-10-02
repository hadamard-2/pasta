// Pushes a clipboard change to Pasta and streams the payloads it asks for into
// the pipe write ends its reply carries.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {closeQuietly, isUsableIndex} from './fd.js';
import {exeBasename, ownerOf} from './peer.js';

const PASTA_NAME = 'com.pasta.Launcher';
const PASTA_PATH = '/com/pasta/Launcher/Clipboard';
const PASTA_IFACE = 'com.pasta.Launcher.Clipboard1';
const PASTA_EXE = 'pasta-launcher';

function transferInto(selection, mimetype, fd) {
    return new Promise(resolve => {
        const stream = GioUnix.OutputStream.new(fd, true);
        selection.transfer_async(Meta.SelectionType.SELECTION_CLIPBOARD, mimetype, -1, stream, null,
            (source, result) => {
                try {
                    source.transfer_finish(result);
                } catch (e) {
                    console.warn(`pasta-clipboard: transfer of ${mimetype} failed: ${e.message}`);
                }
                try {
                    // Closing is what tells the reader the payload is complete.
                    stream.close(null);
                } catch (e) {
                    console.warn(`pasta-clipboard: closing ${mimetype} stream failed: ${e.message}`);
                }
                resolve();
            });
    });
}

function callOffer(destination, mimetypes) {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call_with_unix_fd_list(
            destination, PASTA_PATH, PASTA_IFACE, 'Offer',
            new GLib.Variant('(as)', [mimetypes]), new GLib.VariantType('(a{sh})'),
            Gio.DBusCallFlags.NONE, 2000, null, null,
            (connection, result) => {
                try {
                    resolve(connection.call_with_unix_fd_list_finish(result));
                } catch (e) {
                    reject(e);
                }
            });
    });
}

/** Offers the current clipboard (listing `mimetypes`) to Pasta, if Pasta is running and genuine. */
export async function offerToPasta(selection, mimetypes) {
    let owner;
    try {
        owner = await ownerOf(PASTA_NAME);
    } catch {
        return; // Pasta is not running; nothing to offer to.
    }
    if (exeBasename(owner.exe) !== PASTA_EXE) {
        console.warn(`pasta-clipboard: not offering: ${PASTA_NAME} is owned by ${owner.exe}`);
        return;
    }

    // Address the verified unique name, not the well-known one, so an owner
    // change between the check and the call cannot redirect the payload.
    const [reply, fdList] = await callOffer(owner.unique, mimetypes);
    // steal_fds() hands ownership to us; with get() the list would keep a
    // duplicate of every write end open and the reader would never see EOF.
    const fds = fdList ? fdList.steal_fds() : [];
    const wanted = reply.deepUnpack()[0];
    const used = new Set();
    const transfers = [];
    for (const [mimetype, index] of Object.entries(wanted)) {
        if (!isUsableIndex(index, fds, used)) {
            console.warn(`pasta-clipboard: ignoring bad handle ${index} for ${mimetype}`);
            continue;
        }
        used.add(index);
        transfers.push(transferInto(selection, mimetype, fds[index]));
    }
    fds.forEach((fd, index) => {
        if (!used.has(index))
            closeQuietly(fd);
    });
    await Promise.all(transfers);
}
