// Offers the clipboard as it stands whenever Pasta appears on the bus, so a
// copy made before Pasta started is not lost.
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';

import {offerToPasta} from './offer.js';

export class PastaWatch {
    constructor(selection) {
        // The appeared callback also fires right away when the name is
        // already owned, which covers enabling while Pasta is running.
        this._watchId = Gio.bus_watch_name_on_connection(
            Gio.DBus.session, 'com.pasta.Launcher', Gio.BusNameWatcherFlags.NONE,
            () => this._offerCurrent(selection), null);
    }

    _offerCurrent(selection) {
        const mimetypes = selection.get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD);
        if (mimetypes.length === 0)
            return;
        offerToPasta(selection, mimetypes)
            .catch(e => console.warn(`pasta-clipboard: start-up offer failed: ${e.message}`));
    }

    destroy() {
        Gio.bus_unwatch_name(this._watchId);
    }
}
