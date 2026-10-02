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

### Peer verification (question 2)

### Write-back

## Implications for the real implementation

## Open items
