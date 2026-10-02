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

GNOME Shell discovers extensions only at start-up. An extension copied into a running shell's user extension directory gave an empty dict from `GetExtensionInfo` and `false` from `EnableExtension`. The same extension present before start-up was listed, `EnableExtension` returned `true`, the state was 1 (enabled) and `enable()` ran. Consequence: after installing or upgrading the extension, the user must log out and back in once before enabling can work.

### Event shape

Output of `scenario-watch.sh` (one client copies text, holds the clipboard 3 seconds, then exits):

```
PASTA-SPIKE owner-changed source=[object instance wrapper GType:MetaSelectionSourceWayland ...] mimetypes=["text/plain","text/plain;charset=utf-8"]
PASTA-SPIKE owner-changed source=null mimetypes=[]
PASTA-SPIKE owner-changed source=[object instance wrapper GIName:Meta.SelectionSourceMemory ...] mimetypes=["text/plain;charset=utf-8"]
```

The first event is the real copy, with a Wayland source and both text mimetypes. The second has a null source and an empty mimetype list, which follows the source app exiting (the clipboard became ownerless). The third has a memory source with a single mimetype, which is the shell's own in-memory replay of the content after the owner vanished (inferred from the source type; not separately confirmed). Events with empty mimetypes or a memory source should not be treated as new user copies.

### Large payloads (question 1)

Measured by `scenario-offer.sh` in the nested shell; the extension offers on each `owner-changed` with a non-empty mimetype list, Pasta reads each pipe to EOF. All payloads arrived with the expected byte count and SHA-256 prefix, no truncation, no `steal_fds` problems.

| Payload | Bytes | Pasta `Offer` to `RECEIVED` | Extension `offered` | `RECEIVED` lines |
|---|---|---|---|---|
| text/plain;charset=utf-8 | 10 | 1 ms | 1.83 ms | 2 |
| image/png (about 3 MB) | 3173432 | 21 ms | 22.4 ms | 2 |
| image/png (about 15 MB, incompressible) | 15556313 | 50 ms | 50.7 ms | 1 |

The Xwayland baseline measured earlier via `xclip` was 3.2 MB in about 40 ms and 15.6 MB in about 145 ms, so the pipe path is faster (about 2x to 3x) and the shell side adds under 2 ms of overhead on top of Pasta's own read time.

Each text and 3 MB set produced two `owner-changed` events and so two `RECEIVED` lines: first a `MetaSelectionSourceWayland` source (the real owner), then a `Meta.SelectionSourceMemory` source with the same mimetypes, followed by a `source=null` event with an empty mimetype list in between. The memory-source event is a replay by the clipboard manager re-owning the selection (inferred, not traced to its origin), so the real implementation needs deduplication by content hash or by source type. The 15 MB set showed only the Wayland-source event before the scenario ended (the replay may simply not have arrived yet, inferred).

### Peer verification (question 2)

Both directions show executable-based gating, but with different scopes and enforcement levels.

**Pasta → extension**: Pasta holds `com.pasta.Launcher` and serves `Offer` calls only from a caller whose executable path is exactly `/usr/bin/gnome-shell` (verified via `/proc/<pid>/exe`). Reject-caller scenario (`scenario-reject-caller.sh`, Task 2): when busctl attempts the call, D-Bus returns `org.freedesktop.DBus.Error.AccessDenied` and Pasta logs `rejected Offer from /usr/bin/busctl`. The gate is tight.

**Extension → Pasta**: The extension calls `Offer` on whoever holds `com.pasta.Launcher`, and will refuse to send content if the name owner's executable basename is not `pasta-launcher`. Impostor scenario (`scenario-impostor-pasta.sh`, Task 4): when a Python process claims the name, the extension logs `PASTA-SPIKE refusing to offer: com.pasta.Launcher is owned by /usr/bin/python3` and sends nothing. The check is basename-only (not full path or file ownership), so any native executable named `pasta-launcher` running as the user passes it—including a copied binary, a hard link, or any program compiled under that name.

**Task 5 refusals**: (1) Non-Pasta caller to the bridge: `clip_tool.py poke-bridge` from python gets `GDBus.Error:org.freedesktop.DBus.Error.AccessDenied: /usr/bin/python3.14 is not pasta-launcher` and the shell logs `rejected SetClipboard from /usr/bin/python3.14` (the exe path is the resolved interpreter, python3.14, not the `python3` symlink). (2) Impostor bridge: with the extension disabled and a Python process owning `com.pasta.Launcher.ShellBridge`, `pasta-launcher write` logs `refusing to write: com.pasta.Launcher.ShellBridge is owned by /usr/bin/python3.14`, exits non-zero, and the impostor logs no `IMPOSTOR got` call, so no content reached it.

`SHELL_EXE` is hard-coded to `/usr/bin/gnome-shell`, which is correct for Debian/Ubuntu, Fedora and Arch packaging but not for layouts such as NixOS store paths.

### Write-back

`scenario-write.sh` (Task 5) writes text (16 bytes) and a 3173374-byte PNG through `SetClipboard`: Pasta logs `WROTE` and the shell logs `SetClipboard` with the same 12-hex hash as the source file, and the resulting `owner-changed` makes the extension offer the bytes straight back, so Pasta logs `RECEIVED` with the identical hash (text 0 ms, PNG 14 ms for the echo read).

The shell-side timestamps put the text write and the PNG write plus its echo within about 60 ms of each other end to end (about 3 MB image; not separately timed, so treat as an upper bound on one write).

The echo means a write by Pasta is indistinguishable from a user copy at the Offer layer; the real implementation needs its own loop suppression (inference: for example by remembering the hash it just wrote).

## Implications for the real implementation

## Open items
