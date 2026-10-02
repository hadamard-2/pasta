# GNOME clipboard extension spike — findings

History document: records what a 2026-10 spike measured about feeding Pasta's clipboard history from a GNOME Shell extension. It sees both sides of the D-Bus boundary on purpose; do not copy its cross-boundary reasoning into code comments.

## Questions

1. Can large payloads (multi-MB screenshots) move from the shell to Pasta quickly and intact?
2. Can each side verify the other's executable, and do impostors get refused in both directions?
3. What does installing and enabling a bundled extension look like for a user?

## Environment

Ubuntu 26.04.1, GNOME Shell 50.1, gjs 1.88.0, rustc 1.95.0, session bus `dbus-daemon`. All scenarios ran in a headless nested shell (`spike/gnome-clipboard/nested-shell.sh`) with a private bus, display, XDG dirs and in-memory GSettings.

## Results

### Install and enable (question 3)

GNOME Shell discovers extensions only at start-up. An extension copied into a running shell's user extension directory gave an empty dict from `GetExtensionInfo` and `false` from `EnableExtension`. The same extension present before start-up was listed, `EnableExtension` returned `true`, the state was 1 (enabled) and `enable()` ran. Consequence (inferred from this nested start-up-discovery measurement; not tried in a live session): after installing or upgrading the extension, the user must log out and back in once before enabling can work.

### Event shape

Output of `scenario-watch.sh` (one client copies text, holds the clipboard 3 seconds, then exits):

```
PASTA-SPIKE owner-changed source=[object instance wrapper GType:MetaSelectionSourceWayland ...] mimetypes=["text/plain","text/plain;charset=utf-8"]
PASTA-SPIKE owner-changed source=null mimetypes=[]
PASTA-SPIKE owner-changed source=[object instance wrapper GIName:Meta.SelectionSourceMemory ...] mimetypes=["text/plain;charset=utf-8"]
```

The first event is the real copy, with a Wayland source and both text mimetypes. The second has a null source and an empty mimetype list, which appears right after the source app exits (the event order is measured; that the exit causes it is inferred). The third has a memory source with a single mimetype, which re-presents the same content from shell memory after the owner went away (origin unknown; not traced).

### Large payloads (question 1)

Measured by `scenario-offer.sh` in the nested shell; the extension offers on each `owner-changed` with a non-empty mimetype list, Pasta reads each pipe to EOF. All payloads arrived with the expected byte count and SHA-256 prefix, no truncation, no `steal_fds` problems.

| Payload | Bytes | Pasta `Offer` to `RECEIVED` | Extension `offered` | `RECEIVED` lines |
|---|---|---|---|---|
| text/plain;charset=utf-8 | 10 | 1 ms | 1.83 ms | 2 |
| image/png (about 3 MB) | 3173432 | 21 ms | 22.4 ms | 2 |
| image/png (about 15 MB, incompressible) | 15556313 | 50 ms | 50.7 ms | 1 |

The Xwayland baseline (carried over from the planning conversation, measured with `xclip` through Xwayland and not by any spike scenario) was 3.2 MB in about 40 ms and 15.6 MB in about 145 ms, so the pipe path looked faster (about 2x to 3x) and the shell side added under 2 ms on top of Pasta's own read time. This comparison is indicative only: the two paths differ and each figure is from a single run. The table rows are from the `scenario-offer.sh` run from an earlier `scenario-offer.sh` run. The second 3 MB delivery in a set is the memory-source replay below, and it was faster than the first (14 ms at Pasta and 14.9 ms in the extension in that run; 9 ms at Pasta and 10.6 ms in the extension in the final regression run).

Each text and 3 MB set produced two `owner-changed` events and so two `RECEIVED` lines: first a `MetaSelectionSourceWayland` source (the real owner), then a `Meta.SelectionSourceMemory` source with the same mimetypes, with a `source=null` event with an empty mimetype list between them (measured order: Wayland, null, memory). The 15 MB set showed only the Wayland-source event, and one Offer, before the scenario ended; why no replay was logged is not established (it may not have arrived yet, inferred).

### Peer verification (question 2)

Both directions show executable-based gating, but with different scopes and enforcement levels.

**Pasta checks its caller (`Offer`)**: Pasta holds `com.pasta.Launcher` and serves `Offer` calls only from a caller whose executable path is exactly `/usr/bin/gnome-shell` (verified via `/proc/<pid>/exe`). Reject-caller scenario (`scenario-reject-caller.sh`): when busctl attempts the call, D-Bus returns `org.freedesktop.DBus.Error.AccessDenied` and Pasta logs `rejected Offer from /usr/bin/busctl`. Only busctl and python callers were tried, so "tight" is untested beyond those.

**Extension checks Pasta before offering**: The extension calls `Offer` on whoever holds `com.pasta.Launcher`, and will refuse to send content if the name owner's executable basename is not `pasta-launcher`. Impostor scenario (`scenario-impostor-pasta.sh`): when a Python process claims the name, the extension logs `PASTA-SPIKE refusing to offer: com.pasta.Launcher is owned by /usr/bin/python3.14` (checked in a rerun of the scenario; the scenario's regex matches only the prefix `/usr/bin/python3`, so it would also accept this) and sends nothing. The check is basename-only (not full path or file ownership), so any native executable named `pasta-launcher` running as the user passes it—including a copied binary, a hard link, or any program compiled under that name.

**Bridge refusals** (`scenario-write.sh`): (1) Non-Pasta caller to the bridge: `clip_tool.py poke-bridge` from python gets `GDBus.Error:org.freedesktop.DBus.Error.AccessDenied: /usr/bin/python3.14 is not pasta-launcher` and the shell logs `rejected SetClipboard from /usr/bin/python3.14` (the exe path is the resolved interpreter, python3.14, not the `python3` symlink). (2) Impostor bridge: with the extension disabled and a Python process owning `com.pasta.Launcher.ShellBridge`, `pasta-launcher write` logs `refusing to write: com.pasta.Launcher.ShellBridge is owned by /usr/bin/python3.14`, exits non-zero, and the impostor logs no `IMPOSTOR got` call, so no content reached it.

`SHELL_EXE` is hard-coded to `/usr/bin/gnome-shell`, which was measured only on Ubuntu 26.04.1. It is unverified for Fedora and Arch packaging, and it will not match layouts such as NixOS store paths (inferred).

### Write-back

`scenario-write.sh` writes text (16 bytes) and a 3173374-byte PNG through `SetClipboard`: Pasta logs `WROTE` and the shell logs `SetClipboard` with the same 12-hex hash as the source file, and the resulting `owner-changed` makes the extension offer the bytes straight back, so Pasta logs `RECEIVED` with the identical hash (text 0 ms, PNG 14 ms for the echo read).

The log lines carry no timestamps for the write path, so no end-to-end write latency was measured; the only timings are the echo reads above.

The echo means a write by Pasta is indistinguishable from a user copy at the Offer layer; the real implementation needs its own loop suppression (inference: for example by remembering the hash it just wrote).

## Implications for the real implementation

Each item below is tagged: measured (a scenario showed it), read from code (seen in the spike source, not exercised), or inferred/hypothesis/suggestion (not shown by anything). Untagged text should not be read as confirmed. The go/no-go decision is the maintainer's.

- **Replays and empty events.** One copy produced two `RECEIVED` lines for text and 3 MB payloads (a Wayland-source event, then a `source=null`, empty-mimetype event, then a `Meta.SelectionSourceMemory` replay). Hypothesis: events with empty mimetypes or a memory source are not new user copies. Option A: drop empty-mimetype events in the extension and rely on Pasta's existing content dedup for the replay (read from code: `upsert_clipboard_item_with_hint` in `src/storage.rs` looks up by `content_hash` and bumps recency on a duplicate); costs a duplicate payload transfer per copy. Option B: also filter by source type (skip memory sources) in the extension; saves the transfer, but it assumes a memory source never carries a genuine user copy, which was not tested (for example a clipboard manager re-owning content, or a copy made by the shell itself).
- **Payload reader deadline.** Measured: in the final run no `Offer` went without a `RECEIVED` (five Offers, five `RECEIVED` lines in `scenario-offer.sh`); the 15 MB set had one Offer and one `RECEIVED`, and the fewer lines there come from the replay not being logged, not from a missing read (the reason for the missing replay is unestablished). No stalled writer was tested, so whether a deadline is needed is unmeasured; a source app that never closes its pipe would hold the read open (inferred).
- **Install and upgrade.** After installing or upgrading the extension the user must log out and back in before enabling works (inferred from the nested start-up-discovery measurement; not tried in a live session). While the shell has not discovered it, `GetExtensionInfo` returns an empty dict and `EnableExtension` returns `false`, so (suggestion) an "Enable" button could say "Log out and back in to finish installing" in that state rather than reporting a failure.
- **Limits of executable-name verification.** Pasta accepts only a caller whose `/proc/<pid>/exe` is exactly `/usr/bin/gnome-shell`; measured against busctl and python only; the hard-coded path does not fit layouts such as NixOS store paths. The shell side accepts any process named `pasta-launcher`, so a same-user copy of any binary under that name passes; (inferred) this stops accidental callers, not a hostile same-user process.
- **Formats.** The plan-level change: Pasta chooses which formats it wants (it returns writers only for the mimetypes it wants from `Offer`), not the extension. Pasta must still receive the mimetype list for each event.
- **Selection read timing (read from code, not tested).** `transfer_async` reads the selection when the transfer runs, not as of the `owner-changed` event. The async gap (name and pid lookup plus the `Offer` call) can therefore stream newer content under an earlier event's mimetype list.
- **Write-back echo (measured).** Content Pasta writes comes back through `Offer`, so the real implementation needs loop suppression (for example remembering the hash it just wrote; inferred).
- **Extension fd hygiene (read from code, not tested).** Close stolen fds that are unused, validate handle indexes, and cap the size of the bridge's `readAll`; the spike does none of these.
- **Extension lifecycle (read from code, not tested).** `extension.js` should construct the bridge before connecting `owner-changed`, or guard `disable()` against a missing bridge.

## Open items

- Behaviour in the live session with real apps (Nautilus, KeePassXC, a screenshot tool) is untested, because the spike never loads the extension into the live shell. Source types and event counts there may differ from the nested shell's single test client.
- Secrets handling: whether password managers mark their copies with a hint mimetype and whether the extension sees it was not examined.
- Payloads above 15 MB, a source that stalls mid-transfer, and many rapid copies were not tested.
- Packaging and the real extension UUID are undecided; the spike UUID is deliberately temporary.
- Non-GNOME-50 shells, non-Debian executable layouts and Xwayland-only source apps were not tested.
