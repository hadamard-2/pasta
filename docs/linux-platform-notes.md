# Linux platform notes

Hard-won, non-obvious knowledge about running Pasta on Linux — especially the X11/GPUI interactions that are easy to misdiagnose. If you're touching windowing, the global hotkey, blur, single-instance behavior, or debugging any of it, read this first so you don't re-derive it from scratch. Symbols are referenced by name (not line number) because line numbers drift.

## What "Linux" actually means here

The behavior differs sharply between X11 and Wayland, and `is_wayland_session()` (in `src/platform/linux/mod.rs`) decides which — and it decides purely by `std::env::var_os("WAYLAND_DISPLAY").is_some()`. Nothing else. On a GNOME-on-Xorg session (`XDG_SESSION_TYPE=x11`, no `WAYLAND_DISPLAY`) every Wayland-only code path is skipped. Several features that look "broken" are actually X11-vs-Wayland or GNOME/Mutter-specific, not bugs in our code. When diagnosing, always establish the session type and window manager first.

## GPUI 0.2.2 X11 backend landmines

GPUI's Linux/X11 support is much younger than its macOS path, and several things that "just work" elsewhere do not on X11. These are the biggest time sinks:

- **`Bounds::centered(...)` is not honored by Mutter.** GPUI computes a centered origin and sets the window's x/y, but never sets the ICCCM `USPosition` flag in `WM_NORMAL_HINTS`. Per ICCCM, a window manager is then free to ignore the position and run its own placement — and GNOME's Mutter does exactly that, dropping the window near the top-left. It presents as a **race**: GPUI also re-asserts the position via a post-map reconfigure, so sometimes GPUI wins (centered) and sometimes Mutter wins (top-left). That intermittency is the tell.
- **No per-monitor awareness.** GPUI models a "display" as an entire X screen: origin `(0,0)`, size = the *union bounding box of all monitors*. So on multi-monitor, `Bounds::centered` centers on the middle of the combined desktop (often the seam between monitors), not on one screen. See `X11Display::new` and `primary_display()` in the GPUI source if confirming.
- **`HasWindowHandle` is `unimplemented!()` on X11.** It only works on Wayland. Calling `HasWindowHandle::window_handle(window)` on an X11 GPUI `Window` **panics** (`not implemented` from `gpui/.../x11/window.rs`). This is why the KDE-blur path — which uses the raw handle — is safe only because it's Wayland-gated. To get the X11 window id you must query the X server yourself (see next section).
- **The Linux launcher window is destroyed and recreated on every show.** On hide, `spawn_launcher_transition_loop` calls `window.remove_window()` and sets `LauncherState.window = None`; `show_launcher` recreates it via `create_launcher_window`. Consequence: any window-creation-time behavior (placement, properties) re-runs on **every** open, and Mutter's placement race happens every time — not just at startup.

## Window centering fix (how and why)

`center_launcher_window_on_primary` in `src/platform/linux/mod.rs`, called from inside `create_launcher_window`'s `open_window` closure while the window is still unmapped. Because GPUI won't give us the window id on X11, we find it on the X server:

1. GPUI stamps `_NET_WM_PID` on the windows it creates. We walk the root window's children and match the one whose `_NET_WM_PID` equals our own PID **and** whose device size matches the launcher (`LAUNCHER_WIDTH*scale` × `LAUNCHER_HEIGHT*scale`). The size check is what distinguishes the launcher from the 1×1 background-anchor window, which shares our PID.
2. We compute the center from the **primary RandR monitor** (`primary_monitor_rect`, via `x11rb` with the `randr` feature), falling back to the whole screen if RandR has no primary output.
3. We `ConfigureWindow` the still-unmapped window to that position and set `WM_NORMAL_HINTS` with `WmSizeHintsSpecification::UserSpecified` (the `USPosition` flag), so Mutter honors the position instead of re-placing it.

Everything is best-effort: any failure returns and leaves GPUI's default behavior. It's a no-op on Wayland (the compositor owns placement there). The `x11rb` dependency was already in the tree via GPUI, so adding it as a direct dep pulls nothing new.

**The lookup is a race, and it is now retried.** The window lookup in step 1 runs on our own X11 connection, separate from the one GPUI used to create the window. There's no ordering guarantee between the two — occasionally our `query_tree` runs before GPUI's `CreateWindow`/resize has reached the server, so the launcher either isn't in the tree yet or hasn't reached its final size (the size match tolerates only a couple of pixels). This used to miss roughly 1 in 5 launches, and on a miss the lookup silently returned and Mutter's top-left placement won. `find_launcher_x_window` is now retried (12 attempts, 3ms apart) rather than giving up on the first miss, which matters more than it used to because the same lookup also applies the skip-taskbar state below — losing it costs both a centred window and a stray dock entry. If you want centering guaranteed rather than best-effort, see the GNOME workaround below.

## Staying out of the dock

`NSApplicationActivationPolicyAccessory` keeps Pasta out of the Dock on macOS, and there is no Linux equivalent — nothing about being a tray app stops a window from being listed. GPUI creates the launcher as a plain window with no `_NET_WM_WINDOW_TYPE`, so window managers treat it as `_NET_WM_WINDOW_TYPE_NORMAL` and list it as a running app; on GNOME that means a dock entry, with a *generic* icon because the launcher also carries no `WM_CLASS`. `set_skip_taskbar_and_pager` fixes this by setting `_NET_WM_STATE_SKIP_TASKBAR` and `_NET_WM_STATE_SKIP_PAGER`, piggybacking on the same window lookup as the centering fix.

Two things worth knowing before you touch it:

- **It sets the state twice, deliberately.** Writing the `_NET_WM_STATE` property directly is what the window manager reads at map time, but the write is ignored once the window is already mapped — and the retrying lookup can easily land past that point. The mapped case needs a `_NET_WM_STATE` client message to the root window instead. Doing both covers either side of the map; the redundant one is a no-op. Setting only the property looks like it works and then silently doesn't, which is exactly how this was first mis-fixed.
- **Don't "simplify" it to `WindowKind::PopUp`.** GPUI's X11 backend maps `PopUp` to `_NET_WM_WINDOW_TYPE_NOTIFICATION` (see `params.kind == WindowKind::PopUp` in its `x11/window.rs`), which does keep the window out of the dock — that's why GPUI's own 2x2 notification window is already skip-taskbar. But notification windows are forced always-on-top and window managers may refuse them keyboard focus, which breaks a launcher whose entire purpose is being typed into.

## Blur is KDE-Wayland-only

`try_apply_kde_wayland_blur` uses the KWin protocol `org_kde_kwin_blur`. It early-returns unless the session is Wayland **and** the compositor is KWin (the blur-manager bind fails otherwise). So blur only appears on **KDE Plasma + Wayland**. On X11 (any DE) or GNOME/wlroots Wayland it silently does nothing. This is a compositor limitation, not a missing setting — worth stating plainly since the README advertises blur without that caveat.

## Global hotkey (Meta+Space) needs /dev/input access

`setup_hotkey` is a stub; the real listener is `spawn_hotkey_listener` in `src/app/runtime.rs`, which reads `/dev/input/event*` directly via evdev (it does **not** go through the compositor). That requires read access to those device nodes, normally via membership in the `input` group (`sudo usermod -aG input $USER`, then re-login). If it can't open any keyboard it prints one warning to stderr and silently does nothing. The permission-free alternative is `pasta-launcher --show` bound to a desktop shortcut (see below). Trade-off to note: `input`-group membership gives any process you run keylogger-grade access to all input devices.

## Single-instance guard + `--show` trigger

Two cooperating pieces in `src/platform/linux/mod.rs`, wired from the Linux `main()`:

- **`acquire_single_instance_lock`** takes an exclusive non-blocking `flock` on `$XDG_RUNTIME_DIR/pasta-launcher.lock`, held for the process lifetime (the `File` is parked in a static so it's never dropped). The kernel releases it on exit/crash, so there's no stale-lock wedge. Fails open if the lock file can't be created.
- **Trigger socket** (`spawn_trigger_listener` / `send_show_trigger`) on `$XDG_RUNTIME_DIR/pasta-launcher.sock`. The lock holder binds it and forwards any incoming connection to `MENU_COMMAND_TX` as `ShowLauncher`; a second invocation connects and exits.

`main()` flow: try the lock. If we got it, we're the instance — run normally. If not, another instance is running, so signal it to show its launcher over the socket and exit. This covers both a plain re-launch and `pasta-launcher --show`, so re-launching always surfaces the existing window instead of starting a second copy. `--show` starts a fresh instance if none is running. The listener clears a stale socket file before binding.

## Pasta Brain (neural search)

"Pasta Brain" is the semantic/neural search feature: on-device embeddings via `fastembed` + `ort` (ONNX Runtime), model all-MiniLM-L6-v2. It is **disabled by default** (`default_pasta_brain_enabled`, and the fresh-install defaults in `default_ui_style_state` on both platforms). Two important details: the startup init (`spawn_neural_init`) is **gated on the `pasta_brain_enabled` flag** so a disabled brain never downloads the ~90 MB model or spins up the CPU-heavy ONNX session (this was the cause of fans spinning on launch); and turning it on from the tray now triggers `spawn_neural_init` itself (the `SetPastaBrain(true)` handler in `runtime.rs`), so enabling doesn't need a separate "Download model" step.

## Debugging recipes and gotchas

Things that will waste your time if you don't know them:

- **`pkill -f pasta-launcher` kills the shell running the command itself.** The `-f` pattern matches the full command line, which includes your own `bash -c '...pasta-launcher...'` process, so the script gets SIGKILLed mid-run (symptom: exit code, no output). Use **`pkill -x pasta-launcher`** (exact process-name match) instead.
- **The launcher auto-hides on focus loss and is destroyed on hide.** Running `xwininfo`/`xprop` from a terminal moves focus, so the window can vanish before you inspect it. Launch and measure in a single fast script.
- **Finding the launcher window with no `wmctrl`/`xdotool`.** Only `xwininfo` and `xprop` are available. The launcher has **no `WM_CLASS` and no name**, so match it by size: `xwininfo -root -tree | grep 'has no name.*1720x896'` (1720×896 = `LAUNCHER_WIDTH`×`LAUNCHER_HEIGHT` = 860×448 at 2× scale — derive it from the constants in `main.rs` rather than trusting the number here, it has gone stale before). Confirm position with `xwininfo -id <id>` → `Absolute upper-left X/Y`; centered on a 2560×1600 primary is `420,352`. `xprop -id <id> _NET_WM_STATE` is the quickest check that the skip-taskbar fix is live.
- **Persisted config overrides code defaults.** UI settings live at `~/.config/PastaClipboard/ui-style.json`. A value saved there (e.g. `pasta_brain_enabled`) wins over any changed code default, so flipping a default only affects fresh installs — existing installs need the JSON edited or the tray toggle used.
- **Binary vs package name.** The binary is `pasta-launcher` (dev: `target/debug/pasta-launcher`; installed: `~/.local/bin/pasta-launcher`). The `.deb` *package* is named `pasta` (`[package.metadata.deb]`), but the binary inside is still `pasta-launcher`.
- **GNOME workaround for placement.** `gsettings set org.gnome.mutter center-new-windows true` makes Mutter center all new windows itself — a zero-code way to confirm the placement diagnosis, and, because the in-app fix has a known intermittent miss (see "Known gap" above), currently the more reliable fix if a user still sees top-left placement. The trade-off: it's global to every app on the desktop, not scoped to Pasta, so we deliberately don't set it programmatically — it's a setting for the user to opt into, not something Pasta should change on their behalf.

## Reference: a representative dev environment

One machine this was all characterized on (useful for reproducing the multi-monitor/HiDPI specifics): Ubuntu, GNOME on **X11**, `Xft.dpi=192` (2× scale), dual monitor — `eDP-1` primary 2560×1600 at `+0+0` and `HDMI-1` 3840×2160 at `+2560+0`, combined X screen 6400×2160. Note Ubuntu itself is incidental; the same behavior reproduces on any GNOME-on-X11 distro.
