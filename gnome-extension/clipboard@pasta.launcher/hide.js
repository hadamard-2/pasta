// Keeps the verified Pasta process's windows out of the dock, both Alt+Tab
// switchers and the overview. GNOME 50 gives a Wayland client no way to mark
// its window skip-taskbar, so this adjusts what the shell's JavaScript reads:
// the running-apps list (docks, app switcher) and each window's skip-taskbar
// flag (overview, workspace thumbnails, window switcher).
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import {exeBasename, pidOf} from './peer.js';

const PASTA_NAME = 'com.pasta.Launcher';
const PASTA_EXE = 'pasta-launcher';

export class PastaHider {
    constructor() {
        this._pastaPid = null;
        this._active = true;
        this._generation = 0;
        this._warned = false;
        this._restorers = [];
        this._patchedPrototypes = new Set();
        this._windowCreatedId = 0;
        this._watchId = 0;
        try {
            this._install();
        } catch (e) {
            this.destroy();
            throw e;
        }
    }

    _install() {
        this._wrapMethod(Shell.AppSystem.prototype, 'get_running',
            apps => apps.filter(app => !this._isPastaApp(app)));
        this._wrapMethod(Meta.Window.prototype, 'is_skip_taskbar',
            (skip, window) => skip || this._isPasta(window));
        this._patchSkipTaskbar(Meta.Window.prototype);
        for (const actor of global.get_window_actors())
            this._patchPrototypesOf(actor.meta_window);
        this._windowCreatedId = global.display.connect('window-created',
            (_display, window) => this._guard(() => this._patchPrototypesOf(window), undefined));
        this._watchId = Gio.bus_watch_name_on_connection(
            Gio.DBus.session, PASTA_NAME, Gio.BusNameWatcherFlags.NONE,
            (_connection, _name, owner) => this._verify(owner),
            () => this._forget());
    }

    _isPasta(window) {
        return this._pastaPid !== null && window.get_pid() === this._pastaPid;
    }

    _isPastaApp(app) {
        const windows = app.get_windows();
        return windows.length > 0 && windows.every(window => this._isPasta(window));
    }

    /** Runs `fn`; on any exception logs once and returns `fallback`, so the shell keeps its own answer. */
    _guard(fn, fallback) {
        try {
            return fn();
        } catch (e) {
            if (!this._warned) {
                this._warned = true;
                console.warn(`pasta-clipboard: hiding Pasta failed, leaving it visible: ${e.message}`);
            }
            return fallback;
        }
    }

    /** Replaces `prototype[name]` with a wrapper that passes the original result through `adjust`. */
    _wrapMethod(prototype, name, adjust) {
        const original = prototype[name];
        if (typeof original !== 'function') {
            console.warn(`pasta-clipboard: ${name} not found; Pasta stays visible where it is used`);
            return;
        }
        const hider = this;
        const wrapper = function (...args) {
            const result = original.apply(this, args);
            return hider._active ? hider._guard(() => adjust(result, this), result) : result;
        };
        prototype[name] = wrapper;
        this._restorers.push(() => {
            // Another extension may have wrapped ours since; removing ours
            // would drop theirs, so leave it in place, switched off.
            if (prototype[name] === wrapper)
                prototype[name] = original;
        });
    }

    /**
     * GJS defines the skip_taskbar accessor lazily, on the prototype of the
     * class the lookup started from (MetaWindowWayland, say), where it shadows
     * one on Meta.Window.prototype. So read it once to make it resolve, then
     * override it wherever along the chain it now lives.
     */
    _patchPrototypesOf(window) {
        void window.skip_taskbar;
        for (let proto = Object.getPrototypeOf(window); proto; proto = Object.getPrototypeOf(proto))
            this._patchSkipTaskbar(proto);
    }

    _patchSkipTaskbar(prototype) {
        if (this._patchedPrototypes.has(prototype))
            return;
        void ('skip_taskbar' in prototype);
        const original = Object.getOwnPropertyDescriptor(prototype, 'skip_taskbar');
        if (!original?.get)
            return;
        this._patchedPrototypes.add(prototype);
        const hider = this;
        const get = function () {
            const skip = original.get.call(this);
            return hider._active ? skip || hider._guard(() => hider._isPasta(this), false) : skip;
        };
        try {
            Object.defineProperty(prototype, 'skip_taskbar', {...original, get});
        } catch (e) {
            console.warn(`pasta-clipboard: cannot override skip_taskbar: ${e.message}`);
            return;
        }
        this._restorers.push(() => {
            if (Object.getOwnPropertyDescriptor(prototype, 'skip_taskbar')?.get === get)
                Object.defineProperty(prototype, 'skip_taskbar', original);
        });
    }

    async _verify(owner) {
        const generation = ++this._generation;
        let pid = null;
        try {
            const candidate = await pidOf(owner);
            const exe = GLib.file_read_link(`/proc/${candidate}/exe`);
            if (exeBasename(exe) === PASTA_EXE)
                pid = candidate;
            else
                console.warn(`pasta-clipboard: not hiding: ${PASTA_NAME} is owned by ${exe}`);
        } catch (e) {
            console.warn(`pasta-clipboard: not hiding: could not identify the owner of ${PASTA_NAME}: ${e.message}`);
        }
        // A newer owner change (or destroy) supersedes this answer.
        if (generation === this._generation)
            this._setPid(pid);
    }

    _forget() {
        this._generation++;
        this._setPid(null);
    }

    _setPid(pid) {
        const previous = this._pastaPid;
        if (previous === pid)
            return;
        this._pastaPid = pid;
        this._refresh([previous, pid]);
    }

    /**
     * Lists redraw on signals, not when our answers change, so tell the docks
     * with app-state-changed. Not notify::skip-taskbar: the shell treats that
     * as a real change of the window's own flag, and emitting it spuriously
     * corrupted its state and crashed it once Pasta exited. The overview reads
     * skip_taskbar afresh each time it opens.
     */
    _refresh(pids) {
        this._guard(() => {
            const tracker = Shell.WindowTracker.get_default();
            const apps = new Set();
            for (const actor of global.get_window_actors()) {
                const window = actor.meta_window;
                if (!pids.includes(window.get_pid()))
                    continue;
                const app = tracker.get_window_app(window);
                if (app)
                    apps.add(app);
            }
            const appSystem = Shell.AppSystem.get_default();
            for (const app of apps)
                appSystem.emit('app-state-changed', app);
        }, undefined);
    }

    destroy() {
        this._generation++;
        if (this._watchId) {
            Gio.bus_unwatch_name(this._watchId);
            this._watchId = 0;
        }
        if (this._windowCreatedId) {
            global.display.disconnect(this._windowCreatedId);
            this._windowCreatedId = 0;
        }
        this._active = false;
        for (const restore of this._restorers.reverse())
            restore();
        this._restorers = [];
        const pid = this._pastaPid;
        this._pastaPid = null;
        if (pid !== null)
            this._refresh([pid]);
    }
}
