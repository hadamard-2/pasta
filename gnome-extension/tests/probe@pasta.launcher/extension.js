// Test-only: reports what GNOME Shell's own app and window lists see, so the
// nested-shell scenarios can check them. Installed only into the test nest.
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Workspace} from 'resource:///org/gnome/shell/ui/workspace.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const PROBE_XML = `<node>
  <interface name="com.pasta.TestProbe1">
    <method name="Report"><arg type="s" name="json" direction="out"/></method>
  </interface>
</node>`;

const bump = (counts, pid) => {
    counts[pid] = (counts[pid] ?? 0) + 1;
};

export default class PastaTestProbe extends Extension {
    enable() {
        this._appStateChanges = {};
        this._skipNotifies = {};
        this._everListed = {};
        this._windowSignals = new Map();

        const appSystem = Shell.AppSystem.get_default();
        this._appStateId = appSystem.connect('app-state-changed', (_system, app) => {
            const listed = appSystem.get_running().some(a => a.get_id() === app.get_id());
            for (const pid of new Set(app.get_windows().map(w => w.get_pid()))) {
                bump(this._appStateChanges, pid);
                if (listed)
                    this._everListed[pid] = true;
            }
        });
        for (const actor of global.get_window_actors())
            this._track(actor.meta_window);
        this._windowCreatedId = global.display.connect('window-created',
            (_display, window) => this._track(window));

        this._exported = Gio.DBusExportedObject.wrapJSObject(PROBE_XML, this);
        this._exported.export(Gio.DBus.session, '/com/pasta/TestProbe');
        this._ownerId = Gio.bus_own_name_on_connection(Gio.DBus.session, 'com.pasta.TestProbe',
            Gio.BusNameOwnerFlags.NONE, null, null);
    }

    _track(window) {
        if (this._windowSignals.has(window))
            return;
        this._windowSignals.set(window, window.connect('notify::skip-taskbar',
            () => bump(this._skipNotifies, window.get_pid())));
    }

    Report() {
        const dash = Main.overview.dash;
        // The dash defers redisplay while unmapped, and a headless overview is
        // never shown, so read it after an explicit redisplay.
        dash._redisplay();
        const tracker = Shell.WindowTracker.get_default();
        const windows = global.get_window_actors().map(actor => actor.meta_window);
        return JSON.stringify({
            running: Shell.AppSystem.get_default().get_running().map(app => ({
                id: app.get_id(),
                pids: app.get_windows().map(w => w.get_pid()),
            })),
            dash: dash._box.get_children()
                .map(child => child.child?._delegate?.app?.get_id())
                .filter(Boolean),
            windows: windows.map(w => ({
                pid: w.get_pid(),
                appId: tracker.get_window_app(w)?.get_id() ?? null,
                skip: w.skip_taskbar,
                isSkip: w.is_skip_taskbar(),
                overview: Workspace.prototype._isOverviewWindow.call({}, w),
            })),
            appStateChanges: this._appStateChanges,
            skipNotifies: this._skipNotifies,
            everListed: this._everListed,
        });
    }

    disable() {
        Gio.bus_unown_name(this._ownerId);
        this._exported.unexport();
        global.display.disconnect(this._windowCreatedId);
        Shell.AppSystem.get_default().disconnect(this._appStateId);
        for (const [window, id] of this._windowSignals) {
            try {
                window.disconnect(id);
            } catch {
                // The window is already gone.
            }
        }
        this._windowSignals.clear();
    }
}
