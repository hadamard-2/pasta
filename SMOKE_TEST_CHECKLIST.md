# Pasta Smoke Test Checklist

Use this checklist after each Linux UI-focused phase so we can verify behavior stayed intact and keep the MVP target honest.

## Automated Baseline

Run:

```bash
cargo test
```

Current baseline:

- 41 tests passing on 2026-04-11

## Manual Smoke Pass

### Launcher

- Press `Meta + Space` to show the launcher.
- Press `Escape` to hide it.
- Press `Meta + Space` again repeatedly to ensure show/hide does not get stuck.
- Confirm the launcher can both open and hide via `Meta + Space` without getting stuck in an intermediate transition.

### Global shortcut registration (Linux)

- On the first launch after a fresh install, confirm the desktop's permission dialog appears and that approving it makes `Meta + Space` work.
- Restart Pasta and confirm the shortcut still works **without** a second dialog, and that stderr carries one `pasta: global shortcut registered with the desktop portal (…)` line naming the granted trigger.
- Confirm only one launcher opens per keypress — two listeners running at once would open it twice.
- On a desktop with no GlobalShortcuts portal, confirm stderr explains the fallback and that the evdev path still works for a user in the `input` group.
- With neither available, confirm the tray icon and `pasta-launcher --show` still open the launcher.

### GNOME clipboard extension (GNOME 50, live session)

- Run `gnome-extension/tests/run-all.sh` first (quit Pasta before running it) and confirm four `PASS` lines.
- On a fresh install, open the launcher and confirm the banner says to log out and back in; log out and in, confirm it now offers **Enable**; click it and confirm the banner disappears.
- Copy text in a terminal and in a browser; confirm both appear in history.
- Take a screenshot to the clipboard; confirm an image item appears.
- Copy an image file in Nautilus; confirm the history item is the image, not its path.
- Copy a password from KeePassXC; confirm it is stored as a masked secret.
- Copy a text item and an image item back out of history and paste them into another app.
- With secret auto-clear enabled, copy a secret from history and confirm the clipboard is empty about 30 seconds later.
- Disable the extension in GNOME's Extensions app and confirm the banner returns with **Enable**.

### Tray

- Confirm a tray/status icon appears in the host bar (`waybar`, KDE tray, or equivalent).
- Hover the icon and confirm a tooltip is shown and clearly identifies Pasta.
- Click the tray icon if supported and confirm it does not crash or wedge the app.

### Search

- Type a short query and confirm results filter.
- Backspace quickly through the query and confirm results recover.
- Type a tag-only search with `/tag`.

### Navigation

- Move selection with `Up` and `Down`.
- Move selection with `Ctrl+J`, `Ctrl+K`, `Ctrl+L`, and `Ctrl+;`.
- Confirm scrolling follows the selected row.

### Clipboard Actions

- Press `Enter` on a normal item and confirm it copies.
- Click a row and confirm current click behavior still matches expectations.
- Delete an item with `Delete` or `Ctrl+Backspace`.
- Copy an item from partway down the list with `Enter` or a double click, reopen the launcher, and confirm it now sits at the top with a refreshed relative time.
- Re-copy something already in history from another app, and confirm the existing row moves to the top of an already-open launcher rather than being added a second time or staying put.
- Confirm the preview pane still reports the item's original capture time after it has been hoisted.

### Secret Flow

- Confirm a locked secret's list row shows a few leading characters followed by dots, not the full value, and that two different secrets remain tellable apart.
- Confirm a secret you have given a name with `F2` keeps showing that name in the list.
- Select a secret item and reveal it with `Enter` or `Ctrl+R`, and confirm the list row unmasks alongside the preview pane and re-masks when the reveal window lapses.
- Confirm the current Linux auth behavior matches expectations for this build.
- Copy a revealed secret and confirm auto-clear still behaves as expected.
- Copy a URL and confirm it lands in history as a normal item — unmasked, and with no auth prompt to view it.

### Editors

- Open info editor with `Ctrl+I`, type, save, and cancel.
- Rename an item with `F2`, type, save, and cancel.
- Open tag editor with `Ctrl+T`: confirm it is prefilled with the item's current tags, that deleting one removes it on save, and that clearing the field removes them all without dropping the item's bowl.
- Open bowl editor with `Ctrl+B`: confirm it is prefilled with the current bowl and that a blank field removes the item from its bowl.
- Confirm the preview pane shows the item's bowl (tags and the info note are deliberately not shown there).
- Open parameter editor with `Ctrl+P`.
- Open parameter fill flow by copying a parameterized item.

### Emoji Picker

- Type `e` to surface the emoji affordance and enter the picker.
- Pick an emoji with `Enter` and confirm the launcher closes and the glyph is on the clipboard.
- Pick an emoji by clicking a tile and confirm the launcher closes the same way.
- Reopen the launcher and confirm it comes back to the normal results list, not the emoji grid.
- Confirm the picked emoji did **not** create a clipboard history entry.

### Transforms

- Open transforms with `Tab`.
- Run at least one encode transform and one decode transform.
- On a multi-line item, run `l` (join lines) and `L` (strip newlines) and confirm the pasted result matches.
- Exit transforms with `Tab` or `Escape`.

### Visual Pass

- Check the launcher in light mode.
- Check the launcher in dark mode.
- Verify hover, selected row, borders, editor panels, and tag chips remain clearly visible.
