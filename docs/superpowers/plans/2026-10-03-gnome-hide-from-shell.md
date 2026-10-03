# Hide Pasta from GNOME Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With the `clipboard@pasta.launcher` extension enabled on GNOME 50, every window of the verified `pasta-launcher` process is absent from the dock, both Alt+Tab switchers and the overview; disabling the extension brings it back; no other app is affected.

**Architecture:** A new `PastaHider` class in the existing extension tracks the verified Pasta PID (owner of `com.pasta.Launcher` whose executable basename is `pasta-launcher`) and installs two JavaScript-level overrides: a wrapper around `Shell.AppSystem.prototype.get_running` that drops apps whose windows are all Pasta's, and a `skip_taskbar` getter (plus `is_skip_taskbar()`) that reports `true` for Pasta's windows. When the PID changes or the hider is created/destroyed, it emits `notify::skip-taskbar` on the affected windows and `app-state-changed` for their apps so open docks and the overview redraw. A test-only probe extension reports what the shell's lists see, and a new nested-shell scenario checks it.

**Tech Stack:** GJS 1.88 ESM extension for GNOME Shell 50.1; POSIX `sh` + Python 3/PyGObject test harness around `dbus-run-session` and headless `gnome-shell`.

**Spec:** `docs/superpowers/specs/2026-10-03-gnome-hide-from-shell-design.md`

## Global Constraints

- Extension UUID stays exactly `clipboard@pasta.launcher`; `metadata.json` keeps `"shell-version": ["50"]`; its integer `"version"` goes from `1` to `2` (Pasta compares the loaded version with the on-disk one to tell the user a log-out is pending, which is exactly right here: new JS only loads after one).
- Identification is by verified PID only: the owner of `com.pasta.Launcher`, accepted only when `exeBasename(GLib.file_read_link('/proc/<pid>/exe'))` is `pasta-launcher`. Never by app id or window title.
- Every override returns the original function's result when anything in our code throws, and logs at most once per `PastaHider` lifetime. Hiding failing must never fail `enable()` or affect clipboard capture.
- On removal, restore a member only if its current value is still ours; otherwise leave our wrapper in place, switched off (passes through).
- The extension never blocks the compositor: no `*_sync` D-Bus calls; `GLib.file_read_link` on `/proc/<pid>/exe` is the only synchronous call allowed.
- Extension log lines use the existing prefix `pasta-clipboard:`.
- Comments justify this side's own choices; never describe Pasta's internals from the extension, or the shell's from Pasta.
- Nothing in this plan loads the extension into the live GNOME session or calls `org.gnome.Shell.Extensions` on the live session bus. Extension behaviour is exercised only through `gnome-extension/tests/nested-shell.sh`. The harness refuses to run while a live `pasta-launcher` is running: before running it, stop the live one by PID (`pgrep -x pasta-launcher`, then `kill <pid>`), and afterwards restart it with `gtk-launch com.pasta.launcher`. Never use `pkill -f`.
- Markdown files: one line per paragraph, never hard-wrapped.
- Conventional Commits. Work on branch `feat/gnome-hide-from-shell` in an isolated worktree. Never push.

## Rulings this plan makes against the spec text (for the maintainer to confirm)

1. **Redraw nudge chosen from source, proven by test.** The spec left the signal open. GNOME's dash (`ui/dash.js:380-381`), Ubuntu Dock (`dash.js:261,272`) and Dash to Dock v109 (`dash.js:271,282`) all redisplay on `Shell.AppSystem` `app-state-changed`, and Dash to Dock already emits that signal itself (`locations.js:982`); the overview's workspace removes/adds a window on `notify::skip-taskbar` (`ui/workspace.js:1208-1213`). The hider emits those two, not `installed-changed`, which also reloads the app grid. The scenario checks the emissions happen; that each dock reacts to them is read from source, not tested. Cost if wrong: a dock keeps showing Pasta until its next app change.
2. **Display name becomes "Pasta".** The spec said name and description widen; "Pasta clipboard" no longer covers it. Cost if wrong: one string, and the README line that names it.
3. **The probe's redraw check counts signals rather than watching the dash redraw on its own.** GNOME's dash defers redisplay while unmapped (the headless overview is never shown), so the probe forces `_redisplay()` to read the dash and separately counts `app-state-changed` and `notify::skip-taskbar` per PID to prove the nudge. Cost if wrong: none for correctness; the test proves emission, not each consumer's reaction.

---

## File Structure

- Create `gnome-extension/clipboard@pasta.launcher/hide.js` — `PastaHider`: PID tracking, overrides, redraw nudge, removal.
- Modify `gnome-extension/clipboard@pasta.launcher/peer.js` — export `pidOf(busName)`; `executableOf` uses it.
- Modify `gnome-extension/clipboard@pasta.launcher/extension.js` — create the hider last in `enable()` (own `try`/`catch`), destroy it first in `disable()`.
- Modify `gnome-extension/clipboard@pasta.launcher/metadata.json` — name, description, version.
- Modify `Cargo.toml` (`[package.metadata.deb] assets`) and `packaging/linux/pasta.spec` (`%install` loop) — ship `hide.js`. (`scripts/install-linux-app.sh` copies the whole directory already.)
- Create `gnome-extension/tests/probe@pasta.launcher/{metadata.json,extension.js}` — test-only probe, owns `com.pasta.TestProbe`, method `Report() -> s` (JSON).
- Create `gnome-extension/tests/probe_check.py` — reads the probe report and asserts states.
- Modify `gnome-extension/tests/clip_tool.py` — add `window` and `impostor-window` commands.
- Modify `gnome-extension/tests/nested-shell.sh` — copy the probe extension into the nest.
- Modify `gnome-extension/tests/lib.sh` — add `enable_probe`.
- Create `gnome-extension/tests/scenario-hide.sh`; modify `gnome-extension/tests/run-all.sh` to run it.
- Modify `docs/linux-platform-notes.md`, `README.md`, `SMOKE_TEST_CHECKLIST.md`.

---

### Task 1: Test probe, helpers and the hide scenario (failing)

**Files:**
- Create: `gnome-extension/tests/probe@pasta.launcher/metadata.json`
- Create: `gnome-extension/tests/probe@pasta.launcher/extension.js`
- Create: `gnome-extension/tests/probe_check.py`
- Create: `gnome-extension/tests/scenario-hide.sh`
- Modify: `gnome-extension/tests/clip_tool.py` (add two commands and register them in `COMMANDS`)
- Modify: `gnome-extension/tests/nested-shell.sh` (copy the probe)
- Modify: `gnome-extension/tests/lib.sh` (add `enable_probe`)

**Interfaces:**
- Produces: D-Bus name `com.pasta.TestProbe`, object `/com/pasta/TestProbe`, interface `com.pasta.TestProbe1`, method `Report() -> (s)` returning JSON `{"running":[{"id":str,"pids":[int]}],"dash":[str],"windows":[{"pid":int,"appId":str|null,"skip":bool,"isSkip":bool,"overview":bool}],"appStateChanges":{pid:int},"skipNotifies":{pid:int},"everListed":{pid:true}}`.
- Produces: `probe_check.py wait <hidden|shown> <pid> <timeout>`, `probe_check.py counts <pid>` (prints `"<appStateChanges> <skipNotifies>"`), `probe_check.py wait nudged <pid> <timeout> "<counts-before>"`, `probe_check.py ever-listed <pid>` (prints `yes`/`no`).
- Produces: `clip_tool.py window <seconds>` and `clip_tool.py impostor-window <seconds>`, each printing `WINDOW shown` once the window is presented; the latter also prints `IMPOSTOR owns com.pasta.Launcher`.
- Produces: `enable_probe` in `lib.sh`.

- [ ] **Step 1: Create the probe's metadata**

`gnome-extension/tests/probe@pasta.launcher/metadata.json`:

```json
{
  "uuid": "probe@pasta.launcher",
  "name": "Pasta test probe",
  "description": "Test-only. Reports what GNOME Shell's app and window lists see. Never packaged.",
  "shell-version": ["50"],
  "version": 1
}
```

- [ ] **Step 2: Create the probe extension**

`gnome-extension/tests/probe@pasta.launcher/extension.js`:

```js
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
```

- [ ] **Step 3: Create the report checker**

`gnome-extension/tests/probe_check.py`:

```python
"""Reads the test probe's report and checks it. Only ever run inside nested-shell.sh.

Usage:
  probe_check.py wait hidden <pid> <timeout>
  probe_check.py wait shown <pid> <timeout>
  probe_check.py wait nudged <pid> <timeout> "<counts printed before>"
  probe_check.py counts <pid>
  probe_check.py ever-listed <pid>
"""
import json
import sys
import time

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402


def report():
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    reply = connection.call_sync(
        "com.pasta.TestProbe", "/com/pasta/TestProbe", "com.pasta.TestProbe1", "Report",
        None, GLib.VariantType("(s)"), Gio.DBusCallFlags.NONE, 5000, None)
    return json.loads(reply.unpack()[0])


def windows_of(data, pid):
    return [w for w in data["windows"] if w["pid"] == pid]


def hidden(data, pid):
    """Returns None when every window of `pid` is hidden everywhere, else the reason it is not."""
    windows = windows_of(data, pid)
    if not windows:
        return f"no windows for pid {pid}"
    for w in windows:
        if not (w["skip"] and w["isSkip"] and not w["overview"]):
            return f"window not hidden: {w}"
    if any(pid in app["pids"] for app in data["running"]):
        return f"pid {pid} still in running apps: {data['running']}"
    app_ids = {w["appId"] for w in windows if w["appId"]}
    if app_ids & set(data["dash"]):
        return f"app {app_ids & set(data['dash'])} still in the dash: {data['dash']}"
    return None


def shown(data, pid):
    """Returns None when every window of `pid` is listed everywhere, else the reason it is not."""
    windows = windows_of(data, pid)
    if not windows:
        return f"no windows for pid {pid}"
    for w in windows:
        if w["skip"] or w["isSkip"] or not w["overview"]:
            return f"window hidden: {w}"
    if not any(pid in app["pids"] for app in data["running"]):
        return f"pid {pid} missing from running apps: {data['running']}"
    app_ids = {w["appId"] for w in windows if w["appId"]}
    if not app_ids & set(data["dash"]):
        return f"none of {app_ids} in the dash: {data['dash']}"
    return None


def counts(data, pid):
    key = str(pid)
    return data["appStateChanges"].get(key, 0), data["skipNotifies"].get(key, 0)


def nudged(before):
    app_before, skip_before = (int(n) for n in before.split())

    def check(data, pid):
        app_now, skip_now = counts(data, pid)
        if app_now > app_before and skip_now > skip_before:
            return None
        return f"no nudge for pid {pid}: app-state-changed {app_before}->{app_now}, notify::skip-taskbar {skip_before}->{skip_now}"

    return check


def wait(check, pid, timeout):
    deadline = time.monotonic() + timeout
    reason = "never checked"
    while time.monotonic() < deadline:
        try:
            reason = check(report(), pid)
        except GLib.Error as error:
            reason = f"probe unavailable: {error.message}"
        if reason is None:
            return 0
        time.sleep(0.3)
    print(f"FAIL after {timeout}s: {reason}", flush=True)
    return 1


def main(argv):
    command, *args = argv
    if command == "wait":
        state, pid, timeout, *rest = args
        check = {"hidden": hidden, "shown": shown}.get(state) or nudged(rest[0])
        return wait(check, int(pid), float(timeout))
    if command == "counts":
        print(*counts(report(), int(args[0])), flush=True)
        return 0
    if command == "ever-listed":
        print("yes" if str(int(args[0])) in report()["everListed"] else "no", flush=True)
        return 0
    print(f"unknown command {command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 4: Add the two window commands to `clip_tool.py`**

Insert before `COMMANDS = {`:

```python
def window(seconds):
    """Show a plain window for `seconds`; prints WINDOW shown once it is presented."""

    def activate(app):
        win = Gtk.ApplicationWindow(application=app)
        win.present()
        print("WINDOW shown", flush=True)
        GLib.timeout_add_seconds(int(seconds), lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id="dev.pasta.GnomeTestWindow", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def impostor_window(seconds):
    """Own com.pasta.Launcher without being pasta-launcher, and show a window."""
    Gio.bus_own_name(
        Gio.BusType.SESSION, "com.pasta.Launcher", Gio.BusNameOwnerFlags.NONE,
        None, lambda *_: print("IMPOSTOR owns com.pasta.Launcher", flush=True), None)
    window(seconds)
```

and add to `COMMANDS`:

```python
    "window": window,
    "impostor-window": impostor_window,
```

- [ ] **Step 5: Install the probe into every nest**

In `gnome-extension/tests/nested-shell.sh`, directly after the line `cp -r "$REPO_ROOT/gnome-extension/$UUID" "$NEST/data/gnome-shell/extensions/"`, add:

```sh
# Test-only probe; scenarios that need it enable it themselves.
rm -rf "$NEST/data/gnome-shell/extensions/probe@pasta.launcher"
cp -r "$HERE/probe@pasta.launcher" "$NEST/data/gnome-shell/extensions/"
```

- [ ] **Step 6: Add `enable_probe` to `lib.sh`**

Append to `gnome-extension/tests/lib.sh`:

```sh
# enable_probe: turn on the test-only probe extension and wait until it answers.
enable_probe() {
    enabled=$(busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s probe@pasta.launcher)
    [ "$enabled" = "b true" ] || { echo "FAIL enabling the probe returned: $enabled"; return 1; }
    for _ in $(seq 1 40); do
        if busctl --user call com.pasta.TestProbe /com/pasta/TestProbe com.pasta.TestProbe1 Report >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.25
    done
    echo "FAIL the probe never answered on com.pasta.TestProbe"
    return 1
}
```

- [ ] **Step 7: Write the scenario**

`gnome-extension/tests/scenario-hide.sh` (make it executable with `chmod +x`):

```sh
#!/bin/sh
# Pasta's own windows stay out of the dock, Alt+Tab and the overview while the
# extension is enabled; nothing else is hidden, and disabling restores them.
set -eu
. "$HERE/lib.sh"
CHECK="python3 $HERE/probe_check.py"
enable_probe

# An unrelated window that must stay listed throughout.
python3 "$HERE/clip_tool.py" window 120 >"$NEST/window.log" 2>&1 &
OTHER_PID=$!
wait_for_line "$NEST/window.log" "WINDOW shown" 10

start_pasta
trap 'kill "$PASTA_PID" "$OTHER_PID" 2>/dev/null || true' EXIT
"$REPO_ROOT/target/debug/pasta-launcher" --show 2>>"$NEST/pasta.log" || true

# 1 and 2: Pasta hidden everywhere, the other window untouched.
$CHECK wait hidden "$PASTA_PID" 15
$CHECK wait shown "$OTHER_PID" 5
echo "OBSERVED pasta listed in running apps before it was hidden: $($CHECK ever-listed "$PASTA_PID")"

# 4: disabling restores Pasta and nudges the lists to redraw.
before=$($CHECK counts "$PASTA_PID")
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
$CHECK wait shown "$PASTA_PID" 10
$CHECK wait nudged "$PASTA_PID" 5 "$before"

# 4: enabling with Pasta already running hides it again, with a nudge.
before=$($CHECK counts "$PASTA_PID")
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s "$UUID" >/dev/null
$CHECK wait hidden "$PASTA_PID" 15
$CHECK wait nudged "$PASTA_PID" 5 "$before"
$CHECK wait shown "$OTHER_PID" 5

# 3: a process that is not pasta-launcher gets nothing hidden, even holding Pasta's name.
kill "$PASTA_PID" 2>/dev/null || true
wait "$PASTA_PID" 2>/dev/null || true
python3 "$HERE/clip_tool.py" impostor-window 30 >"$NEST/impostor-window.log" 2>&1 &
IMPOSTOR_PID=$!
trap 'kill "$IMPOSTOR_PID" "$OTHER_PID" 2>/dev/null || true' EXIT
wait_for_line "$NEST/impostor-window.log" "IMPOSTOR owns com.pasta.Launcher" 10
wait_for_line "$NEST/impostor-window.log" "WINDOW shown" 10
wait_for_line "$SHELL_LOG" "pasta-clipboard: not hiding: com.pasta.Launcher is owned by /usr/bin/python3" 10
$CHECK wait shown "$IMPOSTOR_PID" 10
echo "PASS scenario-hide"
```

- [ ] **Step 8: Check the scripts parse**

Run: `sh -n gnome-extension/tests/scenario-hide.sh gnome-extension/tests/lib.sh gnome-extension/tests/nested-shell.sh && python3 -m py_compile gnome-extension/tests/probe_check.py gnome-extension/tests/clip_tool.py && node --check gnome-extension/tests/probe@pasta.launcher/extension.js`
Expected: no output, exit 0.

- [ ] **Step 9: Run the scenario and watch it fail for the right reason**

Stop the live Pasta by PID if one is running (see Global Constraints), then run:

```bash
cargo build --bin pasta-launcher && gnome-extension/tests/nested-shell.sh gnome-extension/tests/scenario-hide.sh
```

Expected: `FAIL after 15.0s: window not hidden: {...}` from the first `wait hidden` (no hider exists yet). Any other failure (probe never answered, no windows for the pid) is a harness bug to fix in this task before moving on. Restart the live Pasta afterwards.

- [ ] **Step 10: Commit** (the scenario is not added to `run-all.sh` yet, so the suite stays green)

```bash
git add gnome-extension/tests
git commit -m "test(gnome): add a shell-list probe and a scenario for hiding Pasta"
```

---

### Task 2: `PastaHider` in the extension, shipped

**Files:**
- Create: `gnome-extension/clipboard@pasta.launcher/hide.js`
- Modify: `gnome-extension/clipboard@pasta.launcher/peer.js:22-26`
- Modify: `gnome-extension/clipboard@pasta.launcher/extension.js`
- Modify: `gnome-extension/clipboard@pasta.launcher/metadata.json`
- Modify: `Cargo.toml:33-39` (deb assets)
- Modify: `packaging/linux/pasta.spec:69` (`%install` loop)
- Modify: `gnome-extension/tests/run-all.sh` (scenario list)

**Interfaces:**
- Consumes: Task 1's scenario and probe.
- Produces: `export function pidOf(busName): Promise<number>` in `peer.js`; `export class PastaHider { constructor(); destroy(); }` in `hide.js`.

- [ ] **Step 1: Export `pidOf` from `peer.js`**

Replace the `executableOf` function in `gnome-extension/clipboard@pasta.launcher/peer.js` with:

```js
/** Process ID of the connection that owns `busName` (unique or well-known). */
export function pidOf(busName) {
    return callBus('GetConnectionUnixProcessID', new GLib.Variant('(s)', [busName]), '(u)');
}

/** Absolute executable path of the process that owns `busName` (unique or well-known). */
export async function executableOf(busName) {
    return GLib.file_read_link(`/proc/${await pidOf(busName)}/exe`);
}
```

- [ ] **Step 2: Create `hide.js`**

`gnome-extension/clipboard@pasta.launcher/hide.js`:

```js
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
     * Lists redraw on signals, not when our answers change, so tell them: the
     * overview listens for notify::skip-taskbar and the docks for
     * app-state-changed.
     */
    _refresh(pids) {
        this._guard(() => {
            const tracker = Shell.WindowTracker.get_default();
            const apps = new Set();
            for (const actor of global.get_window_actors()) {
                const window = actor.meta_window;
                if (!pids.includes(window.get_pid()))
                    continue;
                window.notify('skip-taskbar');
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
```

- [ ] **Step 3: Wire it into `extension.js`**

In `gnome-extension/clipboard@pasta.launcher/extension.js`, add the import after the `PastaWatch` import:

```js
import {PastaHider} from './hide.js';
```

At the end of `enable()`, after `this._pastaWatch = new PastaWatch(this._selection);`, add:

```js
        // Hiding Pasta's windows is independent of the clipboard: if it cannot
        // start, capture keeps working and Pasta simply stays visible.
        try {
            this._hider = new PastaHider();
        } catch (e) {
            console.warn(`pasta-clipboard: not hiding Pasta: ${e.message}`);
            this._hider = null;
        }
```

At the start of `disable()`, before `this._pastaWatch?.destroy();`, add:

```js
        this._hider?.destroy();
        this._hider = null;
```

- [ ] **Step 4: Update `metadata.json`**

```json
{
  "uuid": "clipboard@pasta.launcher",
  "name": "Pasta",
  "description": "Lets the Pasta clipboard manager see and set the clipboard on GNOME, and keeps Pasta's windows out of the dock, Alt+Tab and the overview.",
  "shell-version": ["50"],
  "version": 2
}
```

- [ ] **Step 5: Ship `hide.js`**

In `Cargo.toml`, after the `bridge.js` asset line, add:

```toml
    ["gnome-extension/clipboard@pasta.launcher/hide.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
```

In `packaging/linux/pasta.spec`, change the loop header to:

```
for f in metadata.json extension.js peer.js fd.js offer.js pasta-watch.js bridge.js hide.js; do
```

Then confirm every `.js` file in the extension is listed in both:

Run: `for f in gnome-extension/clipboard@pasta.launcher/*; do b=$(basename "$f"); grep -q "pasta.launcher/$b\"" Cargo.toml || echo "missing from Cargo.toml: $b"; grep -q " $b" packaging/linux/pasta.spec || echo "missing from pasta.spec: $b"; done`
Expected: no output.

- [ ] **Step 6: Syntax-check the extension**

Run: `for f in gnome-extension/clipboard@pasta.launcher/*.js; do node --check "$f" || exit 1; done`
Expected: no output, exit 0.

- [ ] **Step 7: Run the hide scenario and watch it pass**

Stop the live Pasta by PID if running, then:

```bash
cargo build --bin pasta-launcher && gnome-extension/tests/nested-shell.sh gnome-extension/tests/scenario-hide.sh
```

Expected: an `OBSERVED pasta listed in running apps before it was hidden: yes|no` line (keep its value for Task 4) and `PASS scenario-hide`. If `wait nudged` fails, the redraw signals are not emitted; fix `_refresh`, do not loosen the check. Also run `grep -a "pasta-clipboard:" "$NEST/shell.log"` on the printed nest directory and confirm there is no `hiding Pasta failed` line.

- [ ] **Step 8: Add the scenario to the suite and run everything**

In `gnome-extension/tests/run-all.sh`, change the loop to:

```sh
for scenario in capture startup write refusals hide; do
```

Run: `gnome-extension/tests/run-all.sh`
Expected: `PASS` for all five scenarios and exit 0 (the existing four prove the clipboard path is unaffected). Restart the live Pasta afterwards.

- [ ] **Step 9: Commit**

```bash
git add gnome-extension Cargo.toml packaging/linux/pasta.spec
git commit -m "feat(gnome): hide Pasta's windows from the dock, Alt+Tab and the overview"
```

---

### Task 3: Dash to Dock check when it is installed locally

**Files:**
- Modify: `gnome-extension/tests/nested-shell.sh`
- Modify: `gnome-extension/tests/probe@pasta.launcher/extension.js`
- Modify: `gnome-extension/tests/probe_check.py`
- Modify: `gnome-extension/tests/scenario-hide.sh`

**Interfaces:**
- Consumes: Task 1's probe `Report()` and `probe_check.py`; Task 2's hider.
- Produces: report field `"dashToDock": null | "unmapped" | [str]` (app ids in Dash to Dock's dash); `probe_check.py dash-to-dock-lacks <pid>` printing `SKIP <reason>`, `PASS` or `FAIL <reason>`.

- [ ] **Step 1: Copy Dash to Dock into the nest when present**

In `nested-shell.sh`, after the probe copy, add:

```sh
# Third-party dock, used only if this machine has it; scenario-hide skips the check otherwise.
DTD=dash-to-dock@micxgx.gmail.com
if [ -d "$HOME/.local/share/gnome-shell/extensions/$DTD" ]; then
    rm -rf "$NEST/data/gnome-shell/extensions/$DTD"
    cp -r "$HOME/.local/share/gnome-shell/extensions/$DTD" "$NEST/data/gnome-shell/extensions/"
fi
```

This only reads from the real home directory and writes into the nest. Place it before the `export XDG_DATA_HOME=...` line, since `$HOME` is used directly.

- [ ] **Step 2: Report Dash to Dock's icons from the probe**

Dash to Dock v109 keeps its docks on a module-level export, `export let dockManager` in its `extension.js` (assigned in `enable()`), with `dockManager._allDocks` (`docking.js:1842`) and each dock's `dash`, whose `_redisplay()` returns early while unmapped (`dash.js:776`). Importing that module by the same file URI GNOME Shell used returns the same module instance, so the probe imports it once and reads the live binding at report time.

In the probe's `extension.js`, add this at the end of `enable()`:

```js
        // Dash to Dock is optional; its module is read only if it is loaded.
        this._dashToDock = null;
        const dock = Main.extensionManager.lookup('dash-to-dock@micxgx.gmail.com');
        if (dock)
            import(`${dock.dir.get_uri()}/extension.js`)
                .then(module => (this._dashToDock = module))
                .catch(e => console.log(`pasta-probe: Dash to Dock module not readable: ${e.message}`));
```

add this method to the class:

```js
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
```

and add `dashToDock: this._dashToDockApps(),` to the object `Report` serialises.

Because the probe looks Dash to Dock up in its own `enable()`, the scenario must enable Dash to Dock before the probe (Step 4 does).

- [ ] **Step 3: Add the check to `probe_check.py`**

Add to `main` before the unknown-command fallback:

```python
    if command == "dash-to-dock-lacks":
        pid = int(args[0])
        data = report()
        dock = data.get("dashToDock")
        if dock is None:
            print("SKIP Dash to Dock not installed or not running in the nest", flush=True)
            return 0
        if dock == "unmapped":
            print("SKIP Dash to Dock's dash is not mapped in the headless shell", flush=True)
            return 0
        app_ids = {w["appId"] for w in windows_of(data, pid) if w["appId"]}
        if app_ids & set(dock):
            print(f"FAIL Dash to Dock still shows {app_ids & set(dock)}: {dock}", flush=True)
            return 1
        print("PASS", flush=True)
        return 0
```

- [ ] **Step 4: Enable it in the scenario and check case 5**

In `scenario-hide.sh`, immediately before `enable_probe`, add:

```sh
if [ -d "$NEST/data/gnome-shell/extensions/dash-to-dock@micxgx.gmail.com" ]; then
    busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s dash-to-dock@micxgx.gmail.com >/dev/null || true
    sleep 2
fi
```

and after `echo "OBSERVED pasta listed ..."`, add:

```sh
# 5: the third-party dock, when this machine has it.
echo "DASH-TO-DOCK $($CHECK dash-to-dock-lacks "$PASTA_PID")"
$CHECK dash-to-dock-lacks "$PASTA_PID" >/dev/null
```

- [ ] **Step 5: Run the scenario**

Stop the live Pasta by PID, then run: `gnome-extension/tests/nested-shell.sh gnome-extension/tests/scenario-hide.sh`
Expected: a `DASH-TO-DOCK PASS` or `DASH-TO-DOCK SKIP <reason>` line, then `PASS scenario-hide`. Record which, and the reason if SKIP, for Task 4. Restart the live Pasta.

- [ ] **Step 6: Commit**

```bash
git add gnome-extension/tests
git commit -m "test(gnome): check Dash to Dock hides Pasta when it is installed locally"
```

---

### Task 4: Docs and manual checklist

**Files:**
- Modify: `docs/linux-platform-notes.md` (GNOME extension section)
- Modify: `README.md` (GNOME paragraph that names the extension)
- Modify: `SMOKE_TEST_CHECKLIST.md`

**Interfaces:**
- Consumes: the `OBSERVED ...` value from Task 2 Step 7 and the `DASH-TO-DOCK ...` line from Task 3 Step 5.

- [ ] **Step 1: Platform notes**

Add a subsection `### Staying out of the dock on GNOME Wayland` under the GNOME extension section of `docs/linux-platform-notes.md`, one paragraph per bullet, covering, with the measured values filled in:

- The split read from GNOME Shell 50.1's JS: app lists (`ui/dash.js`, Ubuntu Dock, Dash to Dock, the Alt+Tab app switcher) come from `Shell.AppSystem.get_running()`, computed in C; window lists (`ui/workspace.js`, `ui/workspaceThumbnail.js`, the Alt+Tab window switcher) read `skip_taskbar`. The extension's `hide.js` overrides both for the verified Pasta PID only.
- The GJS finding: overriding `skip_taskbar` on `Meta.Window.prototype` alone had no effect because GJS resolves the accessor onto the concrete class prototype (`MetaWindowWayland`); `hide.js` forces resolution and patches wherever it lands.
- The redraw signals (`notify::skip-taskbar`, `app-state-changed`) and that the scenario proves they are emitted, while each dock's reaction is read from source.
- Whether Pasta was ever listed before being hidden in the scenario (the `OBSERVED` value), stated as observed in the nested shell.
- The Dash to Dock result from Task 3 (PASS, or SKIP with its reason).
- That disabling the extension restores Pasta everywhere, and that the X11 path (`set_skip_taskbar_and_pager`) is unchanged.

Also update the earlier paragraph in the "Staying out of the dock" section that says nothing keeps Pasta out of the dock on Wayland, so it points at the new subsection.

- [ ] **Step 2: README**

In the GNOME paragraph that says to turn on "Pasta clipboard" in GNOME's Extensions app, change the name to "Pasta" and add one sentence: the same extension keeps Pasta's windows out of the dock, Alt+Tab and the overview; Pasta is reached with its shortcut or tray icon.

- [ ] **Step 3: Smoke checklist**

Add a `## GNOME (Wayland)` section to `SMOKE_TEST_CHECKLIST.md` (or extend the existing GNOME section if there is one) with these items:

```markdown
- With the Pasta extension enabled, open the launcher: Pasta has no dock icon (GNOME dash, Ubuntu Dock or Dash to Dock).
- Press Alt+Tab and Super+` while the launcher is open: Pasta appears in neither switcher.
- Open the Activities overview while the launcher or the About window is open: no Pasta window thumbnail.
- Copy some text: it still shows up in Pasta's history.
- Turn the extension off in GNOME's Extensions app: Pasta's dock icon comes back without restarting Pasta; turn it on again and the icon goes away.
```

- [ ] **Step 4: Check Markdown is not hard-wrapped**

Run: `git diff -U0 -- '*.md' | grep '^+' | grep -v '^+++' | awk 'length > 0' | head -50`
Expected: each added prose paragraph is a single line (lists, headings and code blocks keep their own lines).

- [ ] **Step 5: Commit**

```bash
git add docs/linux-platform-notes.md README.md SMOKE_TEST_CHECKLIST.md
git commit -m "docs: document how Pasta stays out of the GNOME dock, Alt+Tab and overview"
```
