# Hide Pasta from GNOME Shell — design

Status: approved in conversation on 2026-10-03. Extends the `clipboard@pasta.launcher` extension (`docs/superpowers/specs/2026-10-02-gnome-clipboard-extension-design.md`), whose non-goals listed the dock entry; this design takes that item on.

## Problem

On GNOME 50 Wayland, Pasta's windows show up in the dock, in Alt+Tab and in the overview. Pasta is a tray app with a pop-up launcher, so those entries are noise, and the dock entry's "Quit" item is an easy way to kill Pasta by accident — which also kills the Super+V shortcut, since the portal shortcut lives only as long as the process. On X11 Pasta hides itself with `_NET_WM_STATE_SKIP_TASKBAR` (`set_skip_taskbar_and_pager` in `src/platform/linux/mod.rs`); Wayland has no equivalent a client can set, and `Meta.Window`'s `skip-taskbar` property is read-only to extensions in GNOME 50.

## Goal

With the extension enabled, every window belonging to the verified Pasta process is absent from the dock (GNOME's dash, Ubuntu Dock, Dash to Dock), from both Alt+Tab switchers, and from the overview's window picker and workspace thumbnails. Disabling the extension brings everything back. Nothing about any other app changes.

## Settled decisions

1. **Mechanism:** two JavaScript-level overrides inside the existing extension (approach A). Patching each piece of shell UI separately (B) was rejected because it cannot reach third-party docks; making Pasta's windows something other than normal toplevels (C) was rejected because GNOME has no layer-shell and the alternative is moving the launcher UI into the extension.
2. **Identification:** by the verified process ID — the PID of the owner of `com.pasta.Launcher`, accepted only when that process's executable basename is `pasta-launcher` (the check `offer.js` already applies). Not by app id, which any client can claim.
3. **Surfaces:** dock, Alt+Tab (app and window switchers) and the overview (window picker and workspace thumbnails).
4. **Which windows:** all windows of the Pasta process, including the About window. Accepted cost: after clicking away from About, it can only be reached again from the tray.
5. **Packaging:** same extension, same UUID; its name and description widen from clipboard-only to cover this. `shell-version` stays `["50"]`.

## Evidence (throwaway spike, 2026-10-03)

Read from GNOME Shell 50.1's bundled JS (`/usr/lib/gnome-shell/libshell-18.so` resources) and Dash to Dock v109:

- **App lists** come from `Shell.AppSystem.get_running()`, whose notion of "running" is computed in C and ignores anything an extension can set: GNOME's dash (`ui/dash.js` `_redisplay`), Dash to Dock (`dash.js` `_redisplay`), and the Alt+Tab app switcher (`ui/altTab.js`, `AppSwitcherPopup`).
- **Window lists** filter on the JS-visible `skip_taskbar` property: the overview window picker (`ui/workspace.js` `_isOverviewWindow`), workspace thumbnails (`ui/workspaceThumbnail.js`), and the window switcher (`ui/altTab.js` `getWindows`).

Measured in an isolated nested headless GNOME Shell 50.1 (GJS 1.88) with the real `pasta-launcher` (two windows) and an unrelated window present:

- Wrapping `Shell.AppSystem.prototype.get_running` removed Pasta from the result and from a forced `Main.overview.dash._redisplay()`; the unrelated app stayed. Restoring the original brought Pasta back.
- Redefining `skip_taskbar` only on `Meta.Window.prototype` had **no effect**: GJS had resolved the accessor onto the concrete class prototype (`MetaWindowWayland`), which shadows it. Redefining it on the prototype that actually owns the accessor made Pasta's windows report `true` and `Workspace.prototype._isOverviewWindow` return `false` for them, `true` for the unrelated window. `is_skip_taskbar()` overridden on `Meta.Window.prototype` took effect directly. Restoring the saved descriptors reverted all of it.

Not measured: Dash to Dock in the nested shell (inferred from its calling the same `get_running()`), the Alt+Tab app switcher (same call), and whether open docks redraw when the overrides change (see Refresh).

## Architecture

```
clipboard@pasta.launcher (GNOME Shell process)
 extension.js ── enable(): bridge, owner-changed, PastaWatch, then PastaHider (own try/catch)
 hide.js      ── PastaHider
                  watches com.pasta.Launcher ─► peer.js: owner PID + exe check ─► pastaPid | null
                  installs overrides on construction, removes them on destroy()
                  nudges docks to redraw when pastaPid changes, on install and on removal
 peer.js      ── gains pidOf(busName) (the PID lookup executableOf already does, exposed)
```

### Units

- **`hide.js` — `PastaHider`.** Owns `pastaPid` (a number or `null`), the override installation and removal, and the redraw nudge. `isPasta(window)` is `pastaPid !== null && window.get_pid() === pastaPid`.
- **`peer.js`.** Exposes the PID lookup so the hider and the clipboard path share one verification routine; `exeBasename` is reused unchanged.
- **`extension.js`.** Creates the hider last in `enable()` and destroys it first in `disable()`.

## Overrides

1. **App list.** `Shell.AppSystem.prototype.get_running` is replaced by a wrapper that calls the original and drops each app whose window list is non-empty and consists only of Pasta's windows.
2. **Window flag, property.** For `skip_taskbar`, walk the prototype chain of a live `Meta.Window` (and `Meta.Window.prototype` itself) and redefine the accessor on every prototype that owns one, with a getter returning `isPasta(this) || original.get.call(this)`. The owning prototype is discovered at run time, not hard-coded, because it depends on GJS's lazy property resolution. Each original descriptor is saved for restoration. If no window exists yet when the hider installs, it installs on `Meta.Window.prototype` and repeats the discovery on the first `window-created`.
3. **Window flag, method.** `Meta.Window.prototype.is_skip_taskbar` is wrapped the same way, for callers such as `ui/windowAttentionHandler.js` and `ui/appMenu.js`.

## Refresh

Docks rebuild their app lists on signals such as `app-state-changed`, not on a timer, so a change in what the overrides report is invisible until something else changes. Pasta starting or exiting after the hider is installed is covered by GNOME's own `app-state-changed`. Three cases are not: enabling the extension while Pasta is already running, disabling it while Pasta runs, and `pastaPid` being verified after Pasta's windows already exist. In those cases the hider emits a redraw nudge. Which signal reliably makes GNOME's dash, Ubuntu Dock and Dash to Dock redisplay (candidates: `installed-changed` on `Shell.AppSystem`, `app-state-changed` for Pasta's app) is decided by test during implementation; the scenario below fails if none works.

## Error handling

1. **The overrides never break the shell for other apps.** Every wrapper runs its own logic in `try`/`catch`; on any exception it returns the original function's result unchanged and logs once per hider lifetime.
2. **Hiding failing never stops clipboard capture.** A missing patch target (no `get_running`, no `skip_taskbar` accessor found) skips that override with one `console.warn`. The hider is constructed inside its own `try`/`catch` after the clipboard pieces, so it cannot fail `enable()`.
3. **GNOME upgrades.** `shell-version: ["50"]` disables the whole extension on another major version until it is tested; within 50.x, rule 2 applies.
4. **Other extensions patching the same members.** On removal, a member is restored only if its current value is still ours. Otherwise the wrapper is left in place and switched off (it passes everything through), so another extension's wrapper layered on top survives.
5. **Identity.** No owner, a lookup error, or an executable other than `pasta-launcher` sets `pastaPid = null`, which hides nothing. The name vanishing sets it to `null` before anything else, so a later process reusing the PID is not hidden.
6. **Start-up gap.** Pasta owns `com.pasta.Launcher` only once its GNOME clipboard bridge starts, so a window mapped before that can appear briefly and then vanish on the nudge. Accepted; implementation observes whether it occurs and records the result in `docs/linux-platform-notes.md`.

## Testing

- **Probe extension** (`gnome-extension/tests/probe@pasta.launcher/`, installed only into the nested shell, never packaged) logs, on request, the running-app ids with their window PIDs, the apps in GNOME's dash after a forced redisplay, and for each window its PID, `skip_taskbar`, `is_skip_taskbar()` and `Workspace.prototype._isOverviewWindow`'s verdict. Requests arrive over a test-only D-Bus method on a name the probe owns in the private session bus.
- **`scenario-hide.sh`**, added to `run-all.sh`:
  1. Real Pasta running: absent from running apps and the dash; all its windows report skip-taskbar and are excluded from the overview.
  2. An unrelated window in the same session is unaffected.
  3. An impostor (a non-`pasta-launcher` process that owns `com.pasta.Launcher` and maps a window) has nothing hidden.
  4. Disabling the extension restores Pasta everywhere; re-enabling it with Pasta already running hides it again, with the dash reflecting both without any other change (this is where the redraw nudge is proven).
  5. If Dash to Dock is installed under `~/.local/share/gnome-shell/extensions`, it is copied into the nest and its icons are checked for case 1; otherwise this case prints a SKIP line.
- **Manual (`SMOKE_TEST_CHECKLIST.md`, GNOME section):** Pasta is absent from the dock, both Alt+Tab switchers and the overview; clipboard capture still works; turning the extension off brings Pasta back.

## Docs

- `docs/linux-platform-notes.md`: the app-list/window-list split, the GJS prototype finding, the redraw signal that worked, and whether the start-up gap was observed.
- README GNOME section: the extension also keeps Pasta out of the dock, Alt+Tab and the overview.
- `gnome-extension/clipboard@pasta.launcher/metadata.json`: name and description widened.

## Non-goals

- Hiding Pasta on non-GNOME Wayland desktops (KDE, Hyprland and others have their own mechanisms and are separate work).
- Distinguishing the About window from other Pasta windows.
- Moving the Super+V shortcut into the extension (raised in conversation as a possible follow-up).
- Any change to the X11 skip-taskbar path.
