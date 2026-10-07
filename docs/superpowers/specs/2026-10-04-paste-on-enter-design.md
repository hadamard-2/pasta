# Paste on Enter — design

Status: approved in conversation on 2026-10-04.

## Problem

Choosing an item in Pasta (Enter or double-click) only puts it on the clipboard and hides the launcher; the user then has to press Ctrl+V (Cmd+V) in the app they came from. Every mainstream clipboard manager (Maccy, Raycast, Alfred, Windows' Win+V) finishes the job by pasting into the previously focused field.

## Goal

On GNOME (Wayland, through the bundled extension), on any X11 session, and on macOS, choosing an item puts it on the clipboard, hides Pasta, and pastes it into the window that regains focus. A copy-only variant stays one key away. Other desktops keep today's copy-only behaviour unchanged.

## Settled decisions

1. **Paste key per app.** Terminals get Ctrl+Shift+V, everything else Ctrl+V. Terminals are recognised by the focused window's app ID (Wayland `app_id` / X11 `WM_CLASS`). macOS always uses Cmd+V, which terminals there also accept.
2. **No clipboard restore.** After a paste the clipboard keeps the pasted item, exactly as after today's copy. Restoring the previous content was rejected: the receiving app reads the clipboard asynchronously, so restoring early risks pasting the old content, and re-writing it would disturb history. The previous content remains one keystroke away in Pasta's history.
3. **Secrets paste directly.** Enter on a secret (after the usual OS authentication) hides and pastes it like any item; the existing 30-second auto-clear still applies. The copy-only variant keeps today's secret behaviour (launcher stays open, 12-second reveal).
4. **On by default, no setting.** Enter and double-click paste wherever pasting is supported; Ctrl+Enter (Cmd+Enter on macOS) copies only. There is no toggle; Ctrl+Enter is the escape hatch.
5. **Scope.** GNOME Wayland via the extension, X11 via XTest, macOS via CGEvent. KDE, Sway, Hyprland and other Wayland desktops stay copy-only.
6. **Every copy-then-hide flow pastes.** History items (text, image, secret), the emoji picker, and committing a parameter-template fill. Transforms are unaffected — they write the clipboard but keep the launcher open, so there is no target window to paste into.
7. **One terminal list, in Rust.** The list of terminal app IDs lives in Pasta and is sent to the extension with each paste request, so X11 and GNOME share it and adding a terminal never requires an extension update or re-login.

## Evidence

- GNOME Shell 50.1's own on-screen keyboard (`ui/keyboard.js` in `/usr/lib/gnome-shell/libshell-18.so` resources) creates a virtual keyboard with `seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE)` and types with `notify_keyval(time_us, keyval, state)`. The extension runs in the same process and can use the same API.
- `x11rb` 0.13.2 (already a dependency) has an `xtest` feature providing `xtest_fake_input`.
- The same file shows the seat is reached with `global.stage.context.get_backend().get_default_seat()`, timestamps are microseconds (`Clutter.get_current_event_time() * 1000`), and the device is released with `run_dispose()`.
- `/dev/uinput` is `crw------- root root` on the reference machine, which is why uinput is not the cross-desktop answer.
- **`xclip` takes the CLIPBOARD selection after its parent process has exited.** Traced with `strace -f` against Xwayland: the parent's last X request precedes the `fork`, and the child's first request is `CreateWindow` (opcode 1). A selection owner must be a window, so `SetSelectionOwner` comes later, in the child. Pasta's `write_via_command` returning therefore does not mean the clipboard holds the new content.
- **GPUI 0.2.2's X11 `write_to_clipboard` synchronously makes GPUI's own window the CLIPBOARD owner** (`set_selection_owner` + `flush` in `platform/linux/x11/clipboard.rs`), and for an image item it publishes the item's text, i.e. an empty string. Pasta then runs `xclip`, which takes ownership over from GPUI. So on X11 the owner goes previous → GPUI → `xclip`, and only the last one holds an image correctly.

Not yet verified (the plan verifies each before relying on it): the exact app IDs of installed terminals; that the virtual keyboard's key events reach a focused client in the nested headless shell; macOS behaviour (no macOS machine available — compile-checked by CI only); X11 end to end (no X11 session or Xvfb on the reference machine).

## User-visible behaviour

| Action | Paste-capable platform | Copy-only platform |
| --- | --- | --- |
| Enter / double-click on an item | copy, hide, paste | copy, hide (today) |
| Ctrl+Enter (Cmd+Enter) on an item | copy, hide (today) | copy, hide (today) |
| Enter on an emoji | copy, hide, paste | copy, hide |
| Ctrl+Enter on an emoji | copy, hide | copy, hide |
| Enter committing a parameter fill | copy, hide, paste | copy, hide |
| Ctrl+Enter committing a parameter fill | copy, hide | copy, hide |
| Enter on a secret | authenticate if still masked, then copy, hide, paste, auto-clear | today's authenticate-reveal-copy-and-stay |
| Ctrl+Enter on a secret | today's authenticate-reveal-copy-and-stay | today's authenticate-reveal-copy-and-stay |
| Ctrl+R (reveal and copy) | unchanged: copy-only | unchanged |

The command palette shows a "Paste" row (Enter) and a "Copy" row (Ctrl+Enter / ⌘↩) where pasting is supported, and only "Copy" (Enter) elsewhere.

When a paste cannot be performed (extension outdated or disabled, Accessibility not granted, no non-Pasta window regains focus within the timeout) the item is still on the clipboard; Pasta logs the reason and does nothing else. Pasta never types into its own windows.

## Architecture

```
app/actions.rs  Delivery::{Paste, CopyOnly} chosen by key/click
                ─► write_text_for_delivery / write_image_for_delivery:
                     GPUI write ─► (Paste) platform::begin_paste() -> Option<PasteTicket> ─► platform write
                ─► view.pending_paste = ticket ─► begin_close_transition(Hide)
app/runtime.rs  transition loop, on Hide: hide window (unchanged) ─► if pending_paste: platform::request_paste(ticket, cx)

platform/linux/paste.rs
   paste_supported()   GnomeExtension or X11 clipboard path
   begin_paste()       GnomeExtension ─► ticket; X11 ─► ticket holding the current CLIPBOARD owner; Wayland ─► None
   request_paste()     GnomeExtension ─► bridge worker queue: Paste(TERMINAL_APP_IDS)
                       X11            ─► background thread: wait for owner change + focus, WM_CLASS, XTest
   TERMINAL_APP_IDS, is_terminal(ids)
platform/linux/mod.rs  GNOME bridge writes move from thread-per-write to one serial worker

platform/macos/paste.rs
   paste_supported()   true
   begin_paste()       Accessibility trusted ─► ticket; otherwise prompt once ─► None
   request_paste()     wait for another app frontmost ─► CGEvent Cmd+V

gnome-extension bridge.js  new method Paste(as terminal_app_ids) on ShellBridge1
```

### Units

- **`Delivery` (in `app/actions.rs`).** `Paste` or `CopyOnly`. Enter and double-click map to `Paste` when `paste_supported()`, otherwise `CopyOnly`; Ctrl+Enter / Cmd+Enter always map to `CopyOnly`. `copy_selected_to_clipboard` becomes `deliver_selected(delivery, cx)`; `copy_index_to_clipboard`, `copy_selected_emoji`, `commit_parameter_fill_prompt` and `reveal_secret`'s follow-up copy take a `Delivery` the same way. A `CopyOnly` delivery behaves exactly as today.
- **Write helpers (in `app/actions.rs`).** `write_text_for_delivery` and `write_image_for_delivery` replace the three copies of "mark self-write, GPUI write, platform write". They fix one order on every path — GPUI write, then `begin_paste` (for `Paste`), then the platform write — because the X11 ticket must record the owner GPUI just set, so the paste waits for the platform write's owner instead (see Evidence). The parameter-fill path currently writes in the opposite order; it adopts this one.
- **`PasteTicket` (per platform).** Proof that a paste was prepared before the platform write, carrying whatever the platform needs to wait for it (X11: the CLIPBOARD owner at that moment). `begin_paste()` returning `None` means "this delivery falls back to copy-only", including today's secret behaviour. The view stores it in `pending_paste`; `reset_for_show` clears it.
- **Transition loop (`app/runtime.rs`).** After it hides the window (macOS `cx.hide()`, Linux `remove_window()`), it takes `pending_paste` and, if set, calls `request_paste`. Requesting at hide time rather than at keypress keeps the focus wait short on every platform.
- **`platform/linux/paste.rs`.** Owns the terminal list and classifier, `paste_supported`, `begin_paste`, `request_paste`, and the X11 injector. `TERMINAL_APP_IDS` is matched case-insensitively against each identifier the window offers (Wayland `app_id`; both strings of X11 `WM_CLASS`).
- **GNOME bridge worker (`platform/linux/mod.rs`).** Today each `SetClipboard` runs on its own freshly spawned thread, so nothing orders a later call after it. Writes and paste requests go instead through one long-lived worker thread fed by a channel, processed in order. That is what guarantees the extension has set the clipboard before it is asked to paste.
- **`platform/macos/paste.rs`.** `paste_supported`, `begin_paste`, `request_paste`.
- **Extension `bridge.js`.** Gains `Paste`. The virtual keyboard device is created on first use and released with `run_dispose()` in `destroy()`.

## Platform mechanisms

### GNOME (Wayland, extension)

`Paste(as terminal_app_ids)`:

1. Identify the caller exactly as `SetClipboard` does (executable basename `pasta-launcher`); refuse otherwise. Keep the caller's PID.
2. Wait until `global.display.focus_window` is non-null and its `get_pid()` is not the caller's PID, using `notify::focus-window` with a 1000 ms timeout. On timeout, return a D-Bus error (`no window to paste into`).
3. Read the window's `get_wm_class()` (the Wayland `app_id`, or the class for Xwayland windows) and match it case-insensitively against `terminal_app_ids`.
4. Through the virtual keyboard, press Control_L, (Shift_L for terminals), `v`, then release in reverse order, using `notify_keyval` with increasing `GLib.get_monotonic_time()` timestamps (microseconds, the unit `keyboard.js` passes). Keyvals rather than keycodes keep it independent of where V sits on the layout; whether it works while a non-Latin layout is active is not verified.
5. Return success.

`metadata.json` `version` goes from 2 to 3. Pasta's existing status code already reports a newer on-disk version as a pending update; until re-login the running extension lacks `Paste`, the call fails with an unknown-method error, and Pasta falls back to copy-only.

### X11

`begin_paste` (on the UI thread, between the GPUI write and the `xclip` write) records the current CLIPBOARD owner — GPUI's window, just set — in the ticket. If that query fails, it returns `None` and the delivery is copy-only.

Then, on a background thread with its own `x11rb` connection:

1. Confirm the XTEST extension is present; otherwise log and stop.
2. Poll every 10 ms, for up to 1000 ms in total, until both hold: the CLIPBOARD owner is neither none nor the ticket's owner (i.e. `xclip` has taken it), and `_NET_ACTIVE_WINDOW` names a window whose `_NET_WM_PID` is not Pasta's own PID. A window manager without `_NET_ACTIVE_WINDOW` gets no paste.
3. Read that window's `WM_CLASS` and classify it.
4. Resolve keycodes for `Control_L`, `Shift_L` and `v` from `GetKeyboardMapping`, send press/release events with `xtest_fake_input`, and sync.

Requires enabling `x11rb`'s `xtest` feature. Residual risk: `begin_paste` reads the owner on a different connection from GPUI's, so in principle it could read the owner from before GPUI's flushed request has been processed; the paste would then fire on GPUI's ownership. For text that still pastes the right content (GPUI serves the same string); for an image it would paste an empty string. Not observed; recorded so a report of it is recognisable.

### macOS

1. `begin_paste`: if `AXIsProcessTrusted()` is false, call `AXIsProcessTrustedWithOptions` with the prompt option (the system shows its Accessibility dialog) the first time in each run, log, and return `None` — the delivery is copy-only.
2. `request_paste`, on a foreground timer: poll every 16 ms for up to 1000 ms until `NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier` is not Pasta's PID.
3. Post Cmd+V: `CGEventCreateKeyboardEvent` for key code 9 (`kVK_ANSI_V`) down and up, with `kCGEventFlagMaskCommand` set, posted to `kCGHIDEventTap`.

Known limitation: key code 9 is the physical ANSI "V" position, so on layouts that move V (e.g. Dvorak without the "QWERTY ⌘" variant) the shortcut may be wrong. Layout-aware lookup is a non-goal for now.

## Terminal list

Initial entries, matched case-insensitively against app IDs and `WM_CLASS` strings: `org.gnome.Ptyxis`, `org.gnome.Console`, `org.gnome.Terminal`, `gnome-terminal-server`, `com.mitchellh.ghostty`, `kitty`, `Alacritty`, `org.wezfurlong.wezterm`, `foot`, `footclient`, `org.kde.konsole`, `konsole`, `xfce4-terminal`, `com.gexperts.Tilix`, `terminator`. The plan verifies the IDs of the terminals installed on the reference machine (Ptyxis, Ghostty, GNOME Terminal); the rest are taken from each project's published desktop ID or `WM_CLASS` and may need correction.

Not detectable: terminals embedded in other apps (e.g. an editor's integrated terminal) are classified by their host window and get Ctrl+V. `xterm` and `urxvt` have no default clipboard-paste shortcut and are left out.

## Error handling

Every failure leaves the item on the clipboard and is logged with what Pasta observed (e.g. `paste skipped: no non-Pasta window focused within 1000 ms`, `paste skipped: extension has no Paste method`), never a diagnosis of the other side. No failure blocks the UI thread: the GNOME call runs on the bridge worker, X11 on its own thread, macOS on a timer.

## Security note

The extension accepts `Paste` from any process whose executable is named `pasta-launcher`, the same check `SetClipboard` already uses. Such a process can now cause Ctrl+V / Ctrl+Shift+V in the focused window; it can press nothing else. A same-user process could do worse without the extension, so this is not a new privilege boundary, but it is recorded here as an increase in what the bridge allows.

## Testing

- **Rust unit tests:** terminal classification (case-insensitivity, both `WM_CLASS` strings, non-terminals); `Delivery` mapping from key and modifiers per `paste_supported`; `paste_supported` per clipboard path; command-palette rows and labels per platform support; GNOME bridge worker preserves order (write before paste).
- **Extension scenario** (`gnome-extension/tests/scenario-paste.sh`, nested shell, local only): a non-Pasta caller is refused; with a test window focused, `Paste` produces the expected key events (Ctrl+V for an ordinary window, Ctrl+Shift+V when its app ID is in the list); with only the caller's window present it times out with an error.
- **Manual, reference machine (GNOME 50 Wayland):** paste text, an image and a secret into a GTK text field; paste into Ptyxis and Ghostty; Ctrl+Enter copies without pasting; emoji and parameter-fill paste; with the extension disabled, Enter still copies.
- **Not testable here:** macOS (CI compile only — needs a manual pass on a Mac), X11 (needs an X11 session; the `xclip` ordering check lives there).

## Docs

- `SMOKE_TEST_CHECKLIST.md`: the manual cases above.
- `docs/linux-platform-notes.md`: how pasting works per Linux session type and why non-GNOME Wayland is copy-only.
- README feature list, if it describes Enter's behaviour.

## Non-goals

- Pasting on KDE, Sway, Hyprland or other non-GNOME Wayland desktops.
- Restoring the previous clipboard content.
- A setting to turn pasting off.
- User-configurable terminal list or paste shortcut.
- Layout-aware Cmd+V on macOS.
- Pasting transform results.
