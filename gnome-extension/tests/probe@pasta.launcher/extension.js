// Test-only: reports what GNOME Shell's own app and window lists see, so the
// nested-shell scenarios can check them, and gives the headless seat a
// keyboard. Installed only into the test nest.
import Clutter from 'gi://Clutter';
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
        // A headless shell has no input devices, so its seat offers clients no
        // keyboard until one appears, and keys pressed before a client has
        // bound the new keyboard never reach it. A real session always has a
        // keyboard; stand one in so clients started from now on bind it.
        this._keyboard = global.stage.context.get_backend().get_default_seat()
            .create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);

        this._appStateChanges = {};
        this._everListed = {};

        const appSystem = Shell.AppSystem.get_default();
        this._appStateId = appSystem.connect('app-state-changed', (_system, app) => {
            const listed = appSystem.get_running().some(a => a.get_id() === app.get_id());
            for (const pid of new Set(app.get_windows().map(w => w.get_pid()))) {
                bump(this._appStateChanges, pid);
                if (listed)
                    this._everListed[pid] = true;
            }
        });

        this._exported = Gio.DBusExportedObject.wrapJSObject(PROBE_XML, this);
        this._exported.export(Gio.DBus.session, '/com/pasta/TestProbe');
        this._ownerId = Gio.bus_own_name_on_connection(Gio.DBus.session, 'com.pasta.TestProbe',
            Gio.BusNameOwnerFlags.NONE, null, null);

        // Dash to Dock is optional; its module is read only if it is loaded.
        this._dashToDock = null;
        const dock = Main.extensionManager.lookup('dash-to-dock@micxgx.gmail.com');
        if (dock)
            import(`${dock.dir.get_uri()}/extension.js`)
                .then(module => (this._dashToDock = module))
                .catch(e => console.log(`pasta-probe: Dash to Dock module not readable: ${e.message}`));
    }

    _dashToDockApps() {
        const dash = this._dashToDock?.dockManager?._allDocks?.[0]?.dash ?? null;
        if (!dash)
            return null;
        if (!dash.mapped)
            return 'unmapped';
        dash._redisplay();
        return dash._box.get_children()
            .map(child => child.child?._delegate?.app?.get_id())
            .filter(Boolean);
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
            everListed: this._everListed,
            dashToDock: this._dashToDockApps(),
        });
    }

    disable() {
        Gio.bus_unown_name(this._ownerId);
        this._exported.unexport();
        Shell.AppSystem.get_default().disconnect(this._appStateId);
        this._keyboard.run_dispose();
        this._keyboard = null;
    }
}
