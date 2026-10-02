// Pushes a clipboard change to Pasta and streams the payloads it asks for into
// the pipe write ends its reply carries.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {exeBasename, ownerOf} from './peer.js';

const PASTA_NAME = 'com.pasta.Launcher';
const PASTA_PATH = '/com/pasta/Launcher/Clipboard';
const PASTA_IFACE = 'com.pasta.Launcher.Clipboard1';
const PASTA_EXE = 'pasta-launcher';

function transferInto(selection, mimetype, fd) {
    return new Promise((resolve, reject) => {
        const stream = GioUnix.OutputStream.new(fd, true);
        selection.transfer_async(Meta.SelectionType.SELECTION_CLIPBOARD, mimetype, -1, stream, null,
            (source, result) => {
                try {
                    source.transfer_finish(result);
                    resolve();
                } catch (e) {
                    reject(e);
                } finally {
                    // Closing is what tells the reader the payload is complete.
                    stream.close(null);
                }
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

export async function offerToPasta(selection, mimetypes, log) {
    let owner;
    try {
        owner = await ownerOf(PASTA_NAME);
    } catch (e) {
        log(`no ${PASTA_NAME} on the bus; not offering (${e.message})`);
        return;
    }
    if (exeBasename(owner.exe) !== PASTA_EXE) {
        log(`refusing to offer: ${PASTA_NAME} is owned by ${owner.exe}`);
        return;
    }

    const started = GLib.get_monotonic_time();
    // Address the verified unique name, not the well-known one, so a name
    // change between the check and the call cannot redirect the payload.
    const [reply, fdList] = await callOffer(owner.unique, mimetypes);
    const wanted = reply.deepUnpack()[0];
    // steal_fds() hands ownership to us; with get() the list would keep a
    // duplicate of every write end open and the reader would never see EOF.
    const fds = fdList ? fdList.steal_fds() : [];
    await Promise.all(Object.entries(wanted).map(
        ([mimetype, index]) => transferInto(selection, mimetype, fds[index])));
    log(`offered ${Object.keys(wanted).length} payload(s) in ${(GLib.get_monotonic_time() - started) / 1000} ms`);
}
