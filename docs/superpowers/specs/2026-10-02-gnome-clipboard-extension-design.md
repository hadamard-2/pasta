# GNOME clipboard extension — design

Status: approved in conversation on 2026-10-02. Supersedes the throwaway spike (`spike/gnome-clipboard/`, `examples/gnome_bridge_spike.rs`), whose measured results live in `docs/gnome-extension-spike-findings.md`.

## Problem

GNOME 50 (Ubuntu 26.04 and later) has no X11 session and its compositor implements neither `ext-data-control-v1` nor `wlr-data-control-v1`, so Pasta's Wayland clipboard monitor fails with "missing ext-data-control / wlr-data-control protocol" and Pasta records no clipboard history on GNOME. Secret auto-clear and copying an image back out of history are broken there for the same reason. GNOME deliberately keeps background clients away from the clipboard; code running inside GNOME Shell is not subject to that restriction.

## Goal

On GNOME, Pasta captures clipboard history, writes to the clipboard (text, images, secret auto-clear) and captures what was on the clipboard when it started, through a bundled GNOME Shell extension. Every other desktop keeps its current clipboard path unchanged.

## Settled decisions

1. **Channel:** session D-Bus, initiated by the extension on every clipboard change ("push"). Pasta replies to each push with one pipe write end per payload it wants and the extension streams the payloads into them (GJS cannot create pipes; verified). No interface lets any process ask for the clipboard. Each side verifies the other's executable before exchanging data.
2. **Scope:** clipboard only. The anchor window, dock entry and missing app id are separate work.
3. **When the extension is not usable:** an in-launcher banner with an "Enable" button where that can help (see the state table). No automatic Xwayland fallback.
4. **Delivery:** bundled with Pasta's packages and install script; not published on extensions.gnome.org.
5. **Replays:** the extension forwards every event that lists formats, including GNOME's in-memory replay after a source app exits; Pasta's existing content-hash dedup absorbs repeats. Only events with no formats are dropped.
6. **Start-up capture:** the extension offers the current clipboard once whenever Pasta's D-Bus name appears (including when it is already present as the extension enables).
7. **UUID:** `clipboard@pasta.launcher`. Effectively permanent: it is stored in each user's enabled-extensions setting.
8. **Supported shells:** `metadata.json` declares `"shell-version": ["50"]` only; versions are added as they are tested.
9. **Integration approach:** follow the existing per-path pattern — a change counter plus `read_clipboard_*` functions — rather than an event-driven rewrite of the shared watcher.
10. **Echoes of Pasta's own writes:** the existing self-write suppression stays active for its whole 5-second window instead of clearing after its first match (all Linux paths).
11. **Where the unavailable-capture message shows:** a slim banner above the results whenever capture is unavailable, in addition to the existing empty-history notice.
12. **Testing:** the spike's isolated nested-shell harness is promoted to permanent local tests against the real extension and the real `pasta-launcher`; the write path is tested end to end by a test-only example that compiles Pasta's real bridge client source file. The spike code is deleted; its findings document stays.

## Architecture

```
GNOME Shell process                                   pasta-launcher process
┌──────────────────────────────────────┐             ┌──────────────────────────────────────────┐
│ clipboard@pasta.launcher             │   Offer(as) │ gnome_bridge (service)                   │
│  extension.js  owner-changed ───────►├────────────►│  verify caller = /usr/bin/gnome-shell    │
│  pasta-watch.js Pasta appeared ─────►│  ◄─ a{sh} ──┤  pick formats, return pipe write ends    │
│  offer.js  stream payloads into fds ─┼═══ pipes ══►│  read pipes (cap, deadline) → snapshot   │
│  peer.js   identity checks           │             │  bump counter                            │
│  bridge.js SetClipboard(s,h) ◄───────┼─────────────┤ gnome_bridge (write client)              │
│            St.Clipboard.set_content  │             │  verify bridge owner = /usr/bin/gnome-shell
└──────────────────────────────────────┘             │ gnome_extension_status → banner/Enable   │
                                                     └──────────────────────────────────────────┘
```

### Units

| Unit | Location | Responsibility |
|---|---|---|
| Extension entry | `gnome-extension/clipboard@pasta.launcher/extension.js` | Lifecycle: construct the bridge, then connect `owner-changed` and the Pasta name watcher; undo all of it in `disable()` (guarded against partial `enable()`). |
| Identity | `…/peer.js` | Async lookup of a bus name's unique owner and executable via `org.freedesktop.DBus`; `exeBasename` strips ` (deleted)`. |
| Push | `…/offer.js` | Given a selection and its formats: verify `com.pasta.Launcher`'s owner basename is `pasta-launcher`, call `Offer` on the verified unique name, take ownership of all returned fds (`steal_fds`), stream each wanted payload with `Meta.Selection.transfer_async`, close every fd it received whether used or not, reject out-of-range handles. |
| Start-up offer | `…/pasta-watch.js` | `Gio.bus_watch_name` on `com.pasta.Launcher`; on appearance, push the current clipboard once through `offer.js`. |
| Write bridge | `…/bridge.js` | Owns `com.pasta.Launcher.ShellBridge`, exports `SetClipboard(s mimetype, h data)`; accepts only callers whose executable basename is `pasta-launcher`; reads the fd to EOF with a 32 MiB cap; sets the clipboard with `St.Clipboard.set_content`. |
| Pasta service | `src/platform/linux/gnome_bridge.rs` | Owns `com.pasta.Launcher` and serves `Offer`; caller check; format selection; pipe readers with cap and deadline; publishes the latest snapshot and bumps a counter. |
| Pasta write client | `src/platform/linux/gnome_bridge_client.rs` | `SetClipboard` client with the bridge-owner check. Depends only on `zbus` and `std` (no `crate::` imports) so a test example can include the same file. |
| Extension status | `src/platform/linux/gnome_extension_status.rs` | Query `org.gnome.Shell.Extensions` for the extension, follow `ExtensionStateChanged`, map state to a banner message and optional Enable action, call `EnableExtension` on click. |
| Path selection | `src/platform/linux/mod.rs` | Choose the clipboard path once at start-up and route `clipboard_change_count`, `read_clipboard_*` and `write_clipboard_*` by it. |
| Banner | `src/app/view.rs` | Render the unavailable-capture banner above results, with the Enable button when the status provides one. |

## D-Bus contract

The only surface shared by the two halves. Each side's comments justify its own choices only.

- Pasta owns `com.pasta.Launcher`, object `/com/pasta/Launcher/Clipboard`, interface `com.pasta.Launcher.Clipboard1`, method `Offer(as mimetypes) -> (a{sh} writers)`. Served only to a caller whose `/proc/<pid>/exe` is `/usr/bin/gnome-shell` (with any ` (deleted)` suffix stripped); others get `org.freedesktop.DBus.Error.AccessDenied`. The caller writes the payload for each returned mimetype into its fd and closes it. An empty map means "nothing wanted".
- The extension owns `com.pasta.Launcher.ShellBridge`, object `/com/pasta/Launcher/ShellBridge`, interface `com.pasta.Launcher.ShellBridge1`, method `SetClipboard(s mimetype, h data)`. Served only to a caller whose executable basename (` (deleted)` stripped) is `pasta-launcher`. The callee reads `data` to EOF (at most 32 MiB) and sets the clipboard to it under `mimetype`.
- A contract change bumps the interface version suffix on both sides together.
- Known limit, accepted: the extension's check is a basename match, so any native executable named `pasta-launcher` running as the user passes it. Pasta's check pins the full path `/usr/bin/gnome-shell`, verified on Ubuntu 26.04.1 only.

## Data flow

### Capture

1. `owner-changed` fires for `SELECTION_CLIPBOARD`. If `get_mimetypes` returns an empty list, stop.
2. `offer.js` verifies Pasta's owner and calls `Offer(mimetypes)`.
3. Pasta verifies the caller, then selects wanted formats from the list: the first `image/*`; `text/plain;charset=utf-8`, else `text/plain`; `text/uri-list` and `x-special/gnome-copied-files` when present. For each it creates a pipe, starts a reader thread on the read end and returns the write end.
4. The extension streams each payload into its fd and closes it.
5. Each reader reads to EOF with a 32 MiB cap and a 5-second deadline. A payload that exceeds the cap or misses the deadline is discarded and logged (mimetype, byte count, reason — never contents).
6. When every reader for that `Offer` has finished, Pasta publishes one snapshot: all offered mimetype names, the text (if any), the image bytes and mimetype (if any), and the file-list bytes (if any). Then it increments the GNOME change counter. A later `Offer` replaces the snapshot.
7. The existing watcher sees the counter change and calls the existing functions, which on this path read the snapshot: `read_clipboard_snapshot` (text plus concealed/transient detection from the mimetype names), image capture from the snapshot's image, and `read_clipboard_file_image` from the snapshot's file list. Storage, dedup, secret classification and self-write suppression run unchanged.

Known limit (from the spike, read from code): `transfer_async` reads the selection as it is when the transfer runs, so a second copy landing during the async gap can be streamed under the first event's mimetype list. The next event delivers the newer content again; the effect is at most one mislabelled transfer.

### Start-up

`pasta-watch.js` watches `com.pasta.Launcher`. When it appears, the extension runs the capture flow once for the current clipboard, from step 1. On the GNOME path, Pasta's own start-up clipboard read in `main()` is skipped.

### Writes

On the GNOME path, `write_clipboard_text` and `write_clipboard_image_bytes` call `gnome_bridge_client`: resolve `com.pasta.Launcher.ShellBridge`'s unique owner, verify its executable is `/usr/bin/gnome-shell`, send `SetClipboard` with a pipe read end, and feed the bytes from a thread (dropping the read end before joining so a refused call cannot block the feeder). If the bridge is absent or refused, log and return; the GPUI write that callers already perform remains the text fallback while the launcher has focus. Secret auto-clear's "is my secret still on the clipboard" check reads the snapshot's text, and its clear is an empty-string write through the bridge.

### Self-write suppression

`should_ignore_self_clipboard_write` (Linux) no longer clears the pending entry on a match; the entry expires only at its `due_at` (5 seconds after the write). Every echo of a write within that window is ignored, on all Linux paths. A genuine re-copy of identical content inside the window is also ignored, which only skips a recency bump of the item already at the top.

## Path selection and extension status

At start-up, Linux picks exactly one clipboard path:

1. The Wayland data-control monitor binds → existing Wayland path.
2. Otherwise, if `WAYLAND_DISPLAY` is set and `XDG_CURRENT_DESKTOP` contains `GNOME` → GNOME extension path. Pasta requests `com.pasta.Launcher`; failure to own it becomes the unavailable-capture reason.
3. Otherwise, without `WAYLAND_DISPLAY` → existing X11 path.
4. Otherwise (Wayland, no data-control, not GNOME) → existing missing-protocol reason.

On the GNOME path, Pasta reads `org.gnome.Shell.Extensions` (`UserExtensionsEnabled`, `GetExtensionInfo("clipboard@pasta.launcher")`, `GetExtensionErrors`) at start-up and again on every `ExtensionStateChanged` signal for this UUID. GNOME 50 state values (from the shell's `misc/extensionUtils.js`): `ACTIVE 1, INACTIVE 2, ERROR 3, OUT_OF_DATE 4, DOWNLOADING 5, INITIALIZED 6, DEACTIVATING 7, ACTIVATING 8`.

| Condition (checked in this order) | Banner text | Action |
|---|---|---|
| `UserExtensionsEnabled` is false | "Extensions are turned off in GNOME. Turn them on in the Extensions app so Pasta can see your clipboard." | none — a desktop-wide setting Pasta never changes |
| `GetExtensionInfo` empty and the extension directory exists in `/usr/share/gnome-shell/extensions/` or `~/.local/share/gnome-shell/extensions/` | "Log out and back in to finish installing Pasta's GNOME extension." | none |
| `GetExtensionInfo` empty and no directory | "Pasta's GNOME extension isn't installed. Reinstall Pasta to add it." | none |
| Loaded `version` differs from the on-disk `metadata.json` `version` | "Log out and back in to finish updating Pasta's GNOME extension." | none |
| state `ERROR` | "Pasta's GNOME extension failed to load: <first entry of GetExtensionErrors>." | none |
| state `OUT_OF_DATE` | "This GNOME version isn't supported by Pasta's extension yet." | none |
| state `INACTIVE` or `INITIALIZED` | "Pasta needs its GNOME extension to see your clipboard." | **Enable** → `EnableExtension("clipboard@pasta.launcher")` |
| state `ACTIVE` (or `ACTIVATING`/`DEACTIVATING`/`DOWNLOADING`) | no banner | — |

Unverified detail for the plan to confirm first: that `GetExtensionInfo` includes `metadata.json`'s `version` key (on GNOME 50 it returned the metadata's `uuid`, `name`, `description` and `shell-version`, so other metadata keys are expected but were not checked). If it does not, the update-pending row is dropped rather than replaced with a guess.

The unavailable-capture reason store becomes updatable for the GNOME path (set, replace, clear as the status changes). The X11 and Wayland reasons keep first-reason-wins behaviour.

### Banner

When `clipboard_capture_unavailable_reason()` is `Some`, the launcher shows a one-line banner above the results (muted text, the reason, and an Enable button when the GNOME status provides one), whether or not history is empty. The empty-history notice stays as it is. Clicking Enable suppresses blur auto-hide for the duration of the D-Bus call, like the existing file-picker and auth flows.

## Packaging and install

- `gnome-extension/clipboard@pasta.launcher/{metadata.json,extension.js,peer.js,offer.js,pasta-watch.js,bridge.js}`. `metadata.json`: `uuid` `clipboard@pasta.launcher`, `name` "Pasta clipboard", `shell-version` `["50"]`, integer `version` starting at 1.
- `.deb` (`[package.metadata.deb] assets`) and `.rpm` (`packaging/linux/pasta.spec`) install the directory to `/usr/share/gnome-shell/extensions/clipboard@pasta.launcher/`.
- `scripts/install-linux-app.sh` copies it to `~/.local/share/gnome-shell/extensions/clipboard@pasta.launcher/` and prints: "Log out and back in, then click Enable in Pasta (or enable it in GNOME's Extensions app)." `scripts/uninstall-linux-app.sh` removes that directory. Neither script enables, disables or otherwise changes GNOME settings.

## Error handling

- Every refusal, cap or deadline hit, failed `transfer_async`, failed name lookup and failed `SetClipboard` is logged with mimetype names, sizes, executable paths and reasons only — never clipboard contents. Pasta logs via `eprintln!` like the rest of the Linux platform code; the extension via `console.log`/`console.warn` prefixed `pasta-clipboard:`.
- The extension never blocks the compositor: no `*_sync` D-Bus calls, no blocking reads or writes; reading a `/proc/<pid>/exe` symlink is the only synchronous call.
- A failure in one `Offer` (lookup, refusal, transfer) affects only that event; the extension keeps watching.
- If Pasta's service thread dies, the GNOME path sets an unavailable-capture reason rather than failing silently.

## Testing

- **Rust unit tests** (inline `#[cfg(test)]`, per repo convention): path selection from (data-control result, `WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP`); status-to-banner mapping for every row of the table; wanted-format selection; executable checks including ` (deleted)`; capped and deadline-bounded reads; snapshot publication only after all payloads finish; the self-write window change (a second matching echo inside 5 s is ignored; after `due_at` it is not).
- **Isolated harness** at `gnome-extension/tests/`, promoted from the spike: private `dbus-run-session`, private XDG config/data/cache/state, `GSETTINGS_BACKEND=memory`, headless `gnome-shell --wayland --no-x11 --virtual-monitor`, the extension installed before shell start. It refuses to run while a live `pasta-launcher` process exists (it shares `XDG_RUNTIME_DIR`, so a live instance's single-instance lock would capture the test launch). Verified before this spec: the real `pasta-launcher` starts in this environment, uses the nested display, falls back to ephemeral encryption, and keeps its data inside the scratch directory. Scenarios: text, image and file-manager-reference capture through the real binary; start-up offer (clipboard set before Pasta starts); Pasta refuses a non-shell `Offer` caller; the extension refuses an impostor holding `com.pasta.Launcher`; write path via the test example below; the extension refuses a non-Pasta `SetClipboard` caller; the write client refuses an impostor bridge.
- **Write-path test example:** `examples/gnome_bridge_write.rs` includes `src/platform/linux/gnome_bridge_client.rs` with `#[path]` and exposes a `<mimetype> <file>` command; the harness runs it under the name `pasta-launcher`. Linux-only behind `#[cfg(target_os = "linux")]` with an empty `main` elsewhere, so CI on macOS still builds it.
- **Manual (live session), added to `SMOKE_TEST_CHECKLIST.md`:** copy from Nautilus (file reference becomes an image item), KeePassXC (concealed hint), a screenshot tool, a terminal; copy an item and an image back out of history; secret auto-clear; the Enable-button flow from a fresh install through log-out to active.
- CI keeps running `cargo fmt`, `clippy` (no new warnings), `cargo test` and the release build. The harness is local-only: CI runners do not have GNOME 50.

## Docs

- `docs/linux-platform-notes.md`: a GNOME extension section — why it exists, the contract, the state table, the log-out requirement, the harness and its live-Pasta precondition.
- `README.md`: replace the claim that Pasta is tested on GNOME and needs data-control with the GNOME extension requirement and the Enable flow.
- `AGENTS.md`: add the new modules and `gnome-extension/` to the module layout.
- Delete `spike/gnome-clipboard/`, `examples/gnome_bridge_spike.rs` and its `[[example]]` entry. Keep `docs/gnome-extension-spike-findings.md` and the spike plan as history.

## Non-goals

- Window behaviour on Wayland (anchor window, dock entry, app id, placement).
- GNOME versions other than 50; X11 sessions; publishing on extensions.gnome.org.
- An Xwayland clipboard fallback on GNOME.
- Primary-selection history.
