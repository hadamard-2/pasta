import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {offerToPasta} from './offer.js';

function log(message) {
    console.log(`PASTA-SPIKE ${message}`);
}

export default class PastaClipboardSpike extends Extension {
    enable() {
        this._selection = global.display.get_selection();
        this._ownerChangedId = this._selection.connect('owner-changed', (selection, type, source) => {
            if (type !== Meta.SelectionType.SELECTION_CLIPBOARD)
                return;
            const mimetypes = selection.get_mimetypes(type);
            // String(source) names the concrete source type; recorded so the
            // findings can say which events are replays by the clipboard manager.
            log(`owner-changed source=${String(source)} mimetypes=${JSON.stringify(mimetypes)}`);
            if (mimetypes.length === 0)
                return;
            offerToPasta(selection, mimetypes, log).catch(e => log(`offer failed: ${e.message}`));
        });
        log('enabled');
    }

    disable() {
        this._selection.disconnect(this._ownerChangedId);
        this._selection = null;
        log('disabled');
    }
}
