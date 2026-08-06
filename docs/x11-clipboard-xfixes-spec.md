# Spec: replace X11 clipboard polling with XFIXES event-driven monitoring

Handoff spec for `~/Documents/source-code/pasta`. Read `CLAUDE.md` and `docs/linux-platform-notes.md` first. Symbols are referenced **by name, not line number**, per that doc's convention.

## Problem

On X11, Pasta's clipboard watcher can wedge permanently. Observed 2026-08-06: the app kept its tray icon, ignored the global hotkey, ignored repeated quit attempts, and recorded no clipboard history for 15 minutes. It burned 6 min 33 s of CPU while wedged. Only `kill` recovered it. Journal shows the same fingerprint on 2026-07-20 (25 launcher invocations in 2 seconds — a held-down hotkey against an unresponsive app).

Mechanism, verified rather than inferred:

1. `spawn_clipboard_watcher` (`src/app/runtime.rs`, the non-macOS variant) loops every 350 ms inside `cx.spawn(...)` — the **GPUI foreground executor**.
2. Each tick calls `clipboard_change_count()` → `polling_clipboard_change_count()` → `current_clipboard_signature()` (`src/platform/linux/mod.rs`).
3. That makes up to three blocking `xclip` calls (`TARGETS`, text, bytes), each preceded by `command_exists()` which spawns `sh -lc`. Roughly 4 processes per tick, ~11 processes/second, continuously.
4. All go through `read_via_command` / `read_via_command_bytes`, which use `std::process::Command::output()` — **no timeout**.
5. X11 selection transfer has no protocol-level timeout. If the selection owner changes or exits between request and reply, `xclip -o` waits forever, `.output()` waits forever, and the foreground executor never runs again — which is why the UI, hotkey, and quit all die together.

Evidence captured: pasta's only child was one `xclip -selection clipboard -t TARGETS -o` stuck 15 min in `poll_schedule_timeout`; 10 s of sampling caught zero new spawns where ~28 polls were due; the same command run by hand returned instantly. It wedges on the **first** hang and does not accumulate processes.

## Goal

X11 clipboard monitoring becomes event-driven and non-blocking-on-the-foreground-executor, mirroring the design the Wayland path in this same file already uses.

**The target architecture is already in the codebase.** `clipboard_change_count()` branches on `is_wayland_session()`. The Wayland side calls `ensure_wayland_clipboard_monitor()` — a dedicated named thread (`run_wayland_clipboard_monitor`) that sits on the `ext-data-control` / `wlr-data-control` protocol and bumps `WAYLAND_CLIPBOARD_CHANGE_COUNT` — then returns an atomic load. The X11 side falls through to blocking subprocess polling. **This work makes X11 do what Wayland already does.** Follow that structure closely; do not invent a new one.

## Non-goals

- Do not touch the Wayland path.
- Do not touch macOS.
- Do not change `spawn_clipboard_watcher`'s structure or its 350 ms tick. It should keep working unchanged, just against a now-cheap `clipboard_change_count()` — exactly as it does under Wayland today.
- Do not change storage, dedup, secret classification, or the `should_ignore_self_clipboard_write` logic.

## Design

### Phase A — event-driven change detection

Add an X11 monitor thread paralleling the Wayland one.

**`Cargo.toml`:** `x11rb` is currently `{ version = "0.13.2", features = ["randr"] }`. Add `"xfixes"`. Per `docs/linux-platform-notes.md`, x11rb is already in the tree via GPUI, so this pulls in no new crate.

**`src/platform/linux/mod.rs`:**

- Add statics mirroring the Wayland ones: `X11_CLIPBOARD_CHANGE_COUNT: AtomicI64` and `X11_CLIPBOARD_MONITOR_START: OnceLock<()>`.
- Add `ensure_x11_clipboard_monitor()`, copying the shape of `ensure_wayland_clipboard_monitor()` — `OnceLock::get_or_init`, `std::thread::Builder::new().name("pasta-x11-clipboard-monitor")`, warn to stderr on failure.
- Add `run_x11_clipboard_monitor() -> Result<(), String>`:
  1. Own X connection via `x11rb::connect(None)`. Do **not** share GPUI's connection or the one used by the window helpers.
  2. `xfixes::query_version(&conn, 5, 0)?` — x11rb requires the version handshake before any XFIXES request; requests fail without it.
  3. Intern the `CLIPBOARD` atom (`intern_atom(false, b"CLIPBOARD")`).
  4. `xfixes::select_selection_input(&conn, root, clipboard_atom, SELECTION_EVENT_MASK)` where the mask is `SET_SELECTION_OWNER | SELECTION_WINDOW_DESTROY | SELECTION_CLIENT_CLOSE`. The latter two matter: they are what fires when a selection owner dies, which is precisely the case that used to strand a request forever.
  5. Loop on `conn.wait_for_event()`; on `Event::XfixesSelectionNotify`, `fetch_add(1, Ordering::Release)` on the counter.
- Change `clipboard_change_count()`'s X11 branch to `ensure_x11_clipboard_monitor(); X11_CLIPBOARD_CHANGE_COUNT.load(Ordering::Acquire)`.
- Delete `polling_clipboard_change_count()` and `CLIPBOARD_CHANGE_STATE`, plus `current_clipboard_signature()` and its helpers (`primary_content_mime_type`, `clipboard_text_hash`, `clipboard_bytes_hash`, `read_clipboard_mime_types`) **if** nothing else references them — check first; `read_clipboard_snapshot` and `read_clipboard_file_image` share some.

**Fallback:** if XFIXES is unavailable or the connection fails, log one warning and fall back to the existing polling path rather than leaving clipboard monitoring dead. Keep `polling_clipboard_change_count` alive for this if so — decide and state which, don't leave it ambiguous.

### Phase B — bound the content reads (required; Phase A alone is not sufficient)

Phase A stops the *idle* polling, so on a quiet clipboard nothing blocks at all. But when a change **is** detected, `spawn_clipboard_watcher` still calls `read_clipboard_snapshot()` / `read_clipboard_text()` / `read_clipboard_file_image()` on the foreground executor, and those still shell out to `xclip` with no timeout. A hang during a content read still freezes the app. Phase A cuts the exposure window enormously; it does not close it.

Pick one:

- **B1 (preferred).** Have the monitor thread read the content itself right after the XFIXES notify and publish a ready-made snapshot (e.g. `Mutex<Option<ClipboardSnapshot>>` or an mpsc channel). The watcher then only consumes an already-materialized value. Fully removes clipboard I/O from the foreground executor.
- **B2 (smaller).** Keep the reads where they are but (a) give `read_via_command`/`read_via_command_bytes` a bounded wait — spawn, wait with a deadline (~200 ms), kill and return `None` on expiry — and (b) move the calls onto `cx.background_executor()`, the way the SQLite writes in `spawn_clipboard_watcher` already are.

Either way, also fix `command_exists()`: it spawns `sh -lc` on every call. Cache per-program in a `OnceLock`/`HashMap`. Purely wasteful as-is.

## Edge cases

- **We are the selection owner.** Pasta's own copies go out via `xclip -selection clipboard` (`write_via_command`), which forks a resident process to serve the selection. That fires XFIXES notifies too. Existing `should_ignore_self_clipboard_write` should still cover it — verify it does, since the notification timing changes.
- **Clipboard managers / rapid churn.** Multiple `SetSelectionOwner` events can arrive back-to-back. Counter increments are cheap; the watcher coalesces via `!=` comparison. Don't add debouncing unless a problem shows up.
- **Connection loss.** If `wait_for_event()` errors, log once and exit the thread cleanly. Don't panic — `ensure_wayland_clipboard_monitor` panics only on thread-spawn failure, and match that narrow policy.
- **Secrets.** `is_concealed` / transient detection must behave identically. If Phase B1 moves the read, make sure the concealed-hint path moves intact.
- **Wayland unaffected.** `is_wayland_session()` keys purely off `WAYLAND_DISPLAY`. Verify the new code is unreachable when that is set.

## Testing

- `cargo fmt --all -- --check`, `cargo clippy --all-targets --no-deps` (no new warnings), `cargo test`. CI uses `RUSTFLAGS="-D warnings"` for test/build.
- **Manual, on GNOME-on-X11** (the failing config):
  1. Start Pasta. Confirm **zero** `xclip` children while the clipboard is idle: `PP=$(pgrep -x pasta-launcher); for i in $(seq 1 40); do ps --ppid $PP -o cmd= ; sleep 0.05; done | sort | uniq -c`. Pre-fix this shows constant `xclip`/`sh`; post-fix idle should be empty.
  2. Copy text, an image, and a file from Nautilus — each must appear in history.
  3. **Hang regression test.** Reproduce the original failure: make a selection owner vanish mid-request (e.g. `sleep 5 | xclip -selection clipboard` then kill it while Pasta reads). Pre-fix this wedges the app; post-fix the UI, hotkey, and quit must all stay responsive.
  4. Confirm the hotkey and quit still work after several minutes of heavy clipboard churn.
- Run the relevant parts of `SMOKE_TEST_CHECKLIST.md`.

## Acceptance criteria

1. Idle Pasta on X11 spawns no subprocesses for clipboard monitoring.
2. Clipboard changes are still captured — text, image, file-reference image, concealed/secret items.
3. A stalled or vanished selection owner cannot block the foreground executor. The app stays responsive to hotkey and quit; worst case is a missed history entry.
4. Wayland and macOS behavior unchanged.
5. `cargo fmt`/`clippy`/`test` clean.

## Gotchas

- **`pkill -f pasta-launcher` kills the invoking shell** (the pattern matches its own command line). Use `pkill -x pasta-launcher`.
- The launcher window is destroyed and recreated on every show; auto-hides on focus loss. Running `xwininfo`/`xprop` from a terminal steals focus and the window vanishes — launch and measure in one script.
- `~/.config/PastaClipboard/ui-style.json` overrides code defaults, so changing a default constant won't affect this install.
- Global hotkey needs `input`-group membership; if it prints `no readable keyboards in /dev/input`, that's a permissions issue, not a regression from this work.
