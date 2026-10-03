import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {ShellBridge} from './bridge.js';
import {offerToPasta} from './offer.js';
import {PastaWatch} from './pasta-watch.js';
import {PastaHider} from './hide.js';

export default class PastaClipboardExtension extends Extension {
    enable() {
        this._selection = global.display.get_selection();
        // The bridge comes first: if it throws, nothing else has been
        // connected yet for disable() to miss.
        this._bridge = new ShellBridge();
        this._ownerChangedId = this._selection.connect('owner-changed', (selection, type) => {
            if (type !== Meta.SelectionType.SELECTION_CLIPBOARD)
                return;
            const mimetypes = selection.get_mimetypes(type);
            // A change with no formats is the clipboard being cleared between
            // owners; there is nothing to offer.
            if (mimetypes.length === 0)
                return;
            offerToPasta(selection, mimetypes)
                .catch(e => console.warn(`pasta-clipboard: offer failed: ${e.message}`));
        });
        this._pastaWatch = new PastaWatch(this._selection);
        // Hiding Pasta's windows is independent of the clipboard: if it cannot
        // start, capture keeps working and Pasta simply stays visible.
        try {
            this._hider = new PastaHider();
        } catch (e) {
            console.warn(`pasta-clipboard: not hiding Pasta: ${e.message}`);
            this._hider = null;
        }
    }

    disable() {
        this._hider?.destroy();
        this._hider = null;
        this._pastaWatch?.destroy();
        this._pastaWatch = null;
        if (this._ownerChangedId) {
            this._selection.disconnect(this._ownerChangedId);
            this._ownerChangedId = 0;
        }
        this._bridge?.destroy();
        this._bridge = null;
        this._selection = null;
    }
}
