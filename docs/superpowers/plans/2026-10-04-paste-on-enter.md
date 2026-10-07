# Paste on Enter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enter and double-click on an item put it on the clipboard, hide Pasta, and paste it into the window that regains focus — on GNOME Wayland (through the bundled extension), X11 and macOS — while Ctrl+Enter (Cmd+Enter) keeps today's copy-only behaviour and every other desktop stays copy-only.

**Architecture:** The app layer picks a `Delivery` (`Paste` or `CopyOnly`) from the key or click, writes the clipboard through one helper that calls `platform::begin_paste()` between the GPUI write and the platform write, stores the returned `PasteTicket` on the view, and the transition loop hands it to `platform::request_paste()` once the window is hidden. Each platform waits for focus to leave Pasta and presses the paste shortcut: the GNOME extension through a Clutter virtual keyboard (new D-Bus method `Paste(as)`, reached through a now-serial bridge worker so it always follows the write), X11 through XTest, macOS through CGEvent.

**Tech Stack:** Rust 2024 + GPUI 0.2.2; `x11rb` 0.13.2 (new `xtest` feature); `zbus` 5 blocking client; GJS ESM extension for GNOME Shell 50.1 (Clutter virtual input device); POSIX `sh` + Python 3/PyGObject (GTK 4) nested-shell harness; macOS Cocoa/objc + ApplicationServices FFI.

**Spec:** `docs/superpowers/specs/2026-10-04-paste-on-enter-design.md`

## Global Constraints

- Enter / double-click → `Delivery::Paste` only when `paste_supported()`; the action modifier (Ctrl on Linux, Cmd on macOS) always → `Delivery::CopyOnly`. `CopyOnly` behaves exactly as the code does today, including the secret reveal-and-stay flow.
- No clipboard restore after a paste. No setting to disable pasting.
- Ctrl+R (reveal and copy) and the palette's "Reveal secret" stay copy-only.
- Every clipboard write on a delivery path goes GPUI write → `begin_paste()` (only for `Paste`) → platform write, through `write_text_for_delivery` / `write_image_for_delivery`.
- Pasta never presses keys into its own windows. Focus wait limit: 1000 ms on every platform. Any failure leaves the item on the clipboard and logs `warning: paste skipped: <what was observed>`.
- One terminal list, `TERMINAL_APP_IDS` in `src/platform/linux/paste.rs`, matched case-insensitively; GNOME receives it as the `Paste` argument. Terminals get Ctrl+Shift+V, everything else Ctrl+V; macOS always Cmd+V.
- Extension: UUID `clipboard@pasta.launcher`, `"shell-version": ["50"]` unchanged, `"version"` 2 → 3. `Paste` accepts only callers whose executable basename is `pasta-launcher` (same check as `SetClipboard`). Log prefix `pasta-clipboard:`. No `*_sync` D-Bus calls in the extension.
- Comments justify this side's own choices; never describe the shell's internals from Pasta or Pasta's from the extension.
- Never load the extension into the live GNOME session or call `org.gnome.Shell.Extensions` on the live bus. Exercise it only through `gnome-extension/tests/nested-shell.sh`. That harness refuses to run while a live `pasta-launcher` runs: stop it by PID (`pgrep -x pasta-launcher`, then `kill <pid>`) and restart afterwards with `gtk-launch com.pasta.launcher`. Never `pkill -f`.
- CI parity before calling a task done: `cargo fmt --all -- --check`, `RUSTFLAGS="-D warnings" cargo test`, `cargo clippy --all-targets --no-deps` with no warnings beyond `main`'s count (record `main`'s count once at the start: `cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning'`). `cargo test` builds the `examples/`, so example code must be warning-free too.
- Markdown files: one line per paragraph, never hard-wrapped.
- Conventional Commits, one commit per task (the commit step in each task). Never `git push`. Pasta is a solo project that integrates on `main`: if the executing workflow uses an isolated worktree, use branch `feat/paste-on-enter` and leave merging to the maintainer.

## Rulings this plan makes beyond the spec text (for the maintainer to confirm)

1. **Serial bridge worker isolates panics.** The old thread-per-write design contained a panicking write to its own thread; one long-lived worker would die on the first panic and silently drop every later write. The worker wraps each job in `catch_unwind` and logs. Cost if wrong: none functionally; a few lines.
2. **Palette row label "Paste into previous app".** The spec named the row "Paste". Cost if wrong: one string.
3. **X11 code lives in `src/platform/linux/paste/x11.rs`.** `src/platform/linux/mod.rs` is already 2,758 lines. Cost if wrong: a file move.
4. **GNOME Terminal is listed under three IDs** (`org.gnome.Terminal` Wayland app ID, `gnome-terminal` / `gnome-terminal-server` X11 class strings; its desktop file says `StartupWMClass=Gnome-terminal`), and Ptyxis also as `org.gnome.Ptyxis.Devel`. Cost if wrong: dead list entries.
5. **No paste-path logging on success.** The extension never logs which app it pasted into, to keep the user's activity out of the journal; only failures are logged. Cost if wrong: harder field debugging.

---

## File Structure

- Modify `src/app/actions.rs` — `Delivery`, `action_modifier_held`, `delivery_command_items`, `CommandAction::Paste` + `shortcut_label_for`, write helpers, `deliver_selected` / `deliver_index` (renamed from `copy_selected_to_clipboard` / `copy_index_to_clipboard`), delivery-aware emoji, parameter-fill and reveal paths, key routing, tests.
- Modify `src/app/state.rs` — `pending_paste: Option<PasteTicket>` on `LauncherView`.
- Modify `src/app/view.rs` — double-click and emoji-tile click pass a `Delivery`.
- Modify `src/app/runtime.rs` — transition loop calls `request_paste` after hiding.
- Create `src/platform/linux/paste.rs` — terminal list, classifier, `PasteTicket`, `paste_supported`, `begin_paste`, `request_paste`.
- Create `src/platform/linux/paste/x11.rs` — X11 owner query, focus wait, `WM_CLASS`, keycode lookup, XTest.
- Modify `src/platform/linux/mod.rs` — `mod paste` + re-exports; GNOME bridge serial worker (`BridgeJob`, `spawn_serial_worker`, `queue_gnome_paste`).
- Modify `src/platform/linux/gnome_bridge_client.rs` — shared `verified_bridge_owner`, new `paste`.
- Create `src/platform/macos/paste.rs`; modify `src/platform/macos/mod.rs` — re-exports.
- Modify `Cargo.toml` — `x11rb` features gain `"xtest"`.
- Create `examples/gnome_bridge_paste.rs`; modify `examples/gnome_bridge_write.rs` — `#[allow(dead_code)]` on the shared client module.
- Modify `gnome-extension/clipboard@pasta.launcher/bridge.js` — `Paste` method, focus wait, virtual keyboard; `metadata.json` — version 3, description.
- Modify `gnome-extension/tests/clip_tool.py` (`key-catcher`, `poke-paste`), `gnome-extension/tests/run-all.sh`; create `gnome-extension/tests/scenario-paste.sh`.
- Modify `docs/linux-platform-notes.md`, `SMOKE_TEST_CHECKLIST.md`, `AGENTS.md`.

No packaging change: no new extension file is added, and `bridge.js` already ships.

---

### Task 1: Delivery routing in the app, platform stubs that keep behaviour unchanged

After this task, the app chooses `Paste` vs `CopyOnly` everywhere and threads a `PasteTicket` to the transition loop, but both platforms report `paste_supported() == false`, so every delivery is still copy-only. Behaviour is unchanged; the palette still shows only "Copy" (Enter).

**Files:**
- Create: `src/platform/linux/paste.rs`, `src/platform/macos/paste.rs`
- Modify: `src/platform/linux/mod.rs` (module list near line 37, re-exports near line 43), `src/platform/macos/mod.rs`
- Modify: `src/app/actions.rs`, `src/app/state.rs:253`, `src/app/view.rs:1872,2527`, `src/app/runtime.rs:355-365`
- Test: `src/app/actions.rs` (`mod tests` at the end of the file)

**Interfaces:**
- Consumes: nothing new.
- Produces (used by every later task):
  - `crate::platform::PasteTicket` (opaque, not `Clone`)
  - `crate::platform::paste_supported() -> bool`
  - `crate::platform::begin_paste() -> Option<PasteTicket>`
  - `crate::platform::request_paste(ticket: PasteTicket, cx: &mut gpui::App)`
  - `app::actions::Delivery { Paste, CopyOnly }` with `Delivery::for_choice(copy_only_requested: bool, paste_supported: bool) -> Delivery` and `Delivery::chosen(copy_only_requested: bool) -> Delivery`
  - `app::actions::action_modifier_held(modifiers: &gpui::Modifiers) -> bool`
  - `LauncherView::pending_paste: Option<PasteTicket>`

- [ ] **Step 1: Create the Linux stub**

Create `src/platform/linux/paste.rs`:

```rust
//! Pasting into the window that regains focus after the launcher hides.

use gpui::App;

/// Proof that a paste was prepared before the platform clipboard write,
/// carrying what the platform needs to wait for that write.
pub(crate) struct PasteTicket(());

/// Whether this session can paste on the user's behalf.
pub(crate) fn paste_supported() -> bool {
    false
}

/// Prepares a paste; call it after the GPUI clipboard write and before the
/// platform one. `None` means the delivery falls back to copy-only.
pub(crate) fn begin_paste() -> Option<PasteTicket> {
    paste_supported().then_some(PasteTicket(()))
}

/// Presses the paste shortcut in the window that next holds focus, once it is
/// not one of Pasta's. Never blocks the caller.
pub(crate) fn request_paste(_ticket: PasteTicket, _cx: &mut App) {}
```

In `src/platform/linux/mod.rs`, add `mod paste;` after `mod gnome_extension_status;` (line 40), and after the `pub(crate) use global_shortcuts::...;` line (43) add:

```rust
pub(crate) use paste::{PasteTicket, begin_paste, paste_supported, request_paste};
```

- [ ] **Step 2: Create the macOS stub**

Create `src/platform/macos/paste.rs` with exactly the same content as the Linux stub above. In `src/platform/macos/mod.rs` add `mod paste;` after `mod menu;` and, after the `pub(crate) use menu::{...};` block:

```rust
pub(crate) use paste::{PasteTicket, begin_paste, paste_supported, request_paste};
```

- [ ] **Step 3: Write the failing tests**

Append inside `mod tests` at the end of `src/app/actions.rs`:

```rust
    #[test]
    fn enter_pastes_only_where_supported_and_unmodified() {
        assert_eq!(Delivery::for_choice(false, true), Delivery::Paste);
        assert_eq!(Delivery::for_choice(true, true), Delivery::CopyOnly);
        assert_eq!(Delivery::for_choice(false, false), Delivery::CopyOnly);
        assert_eq!(Delivery::for_choice(true, false), Delivery::CopyOnly);
    }

    #[test]
    fn palette_offers_paste_above_copy_only_where_supported() {
        let actions = |supported| {
            delivery_command_items(supported)
                .iter()
                .map(|command| command.action)
                .collect::<Vec<_>>()
        };
        assert!(actions(true) == vec![CommandAction::Paste, CommandAction::Copy]);
        assert!(actions(false) == vec![CommandAction::Copy]);
    }

    #[test]
    fn copy_moves_off_enter_when_paste_takes_it() {
        assert_eq!(CommandAction::Paste.shortcut_label_for(true), "Enter");
        assert_eq!(CommandAction::Copy.shortcut_label_for(false), "Enter");
        let expected = if cfg!(target_os = "macos") {
            "⌘↩"
        } else {
            "Ctrl+Enter"
        };
        assert_eq!(CommandAction::Copy.shortcut_label_for(true), expected);
    }

    #[test]
    fn action_modifier_is_ctrl_on_linux_and_cmd_on_macos() {
        let ctrl = gpui::Modifiers {
            control: true,
            ..Default::default()
        };
        let cmd = gpui::Modifiers {
            platform: true,
            ..Default::default()
        };
        assert_eq!(action_modifier_held(&ctrl), cfg!(not(target_os = "macos")));
        assert_eq!(action_modifier_held(&cmd), cfg!(target_os = "macos"));
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --bin pasta-launcher -- enter_pastes palette_offers copy_moves action_modifier 2>&1 | tail -20`
Expected: compile errors — `Delivery`, `delivery_command_items`, `CommandAction::Paste`, `shortcut_label_for`, `action_modifier_held` not found.

- [ ] **Step 5: Add `Delivery`, `action_modifier_held`, `CommandAction::Paste`, labels and palette rows**

In `src/app/actions.rs`, add `Paste,` as the first variant of `enum CommandAction` (before `Copy,`).

Replace the head of `impl CommandAction` — from the doc comment `/// The keyboard shortcut that runs this action directly...` through the `CommandAction::Copy => "Enter",` arm — with:

```rust
    /// The keyboard shortcut that runs this action directly from the results
    /// list, shown faded beside its palette row. Must be kept in step with the
    /// key routing in `handle_keystroke`.
    pub(crate) fn shortcut_label(self) -> &'static str {
        self.shortcut_label_for(paste_supported())
    }

    /// `shortcut_label` for a platform that can (or cannot) paste: Enter
    /// pastes where it can, which moves copy-only to the action modifier.
    pub(crate) fn shortcut_label_for(self, paste_supported: bool) -> &'static str {
        let mac = cfg!(target_os = "macos");
        match self {
            CommandAction::Paste => "Enter",
            CommandAction::Copy => {
                if !paste_supported {
                    "Enter"
                } else if mac {
                    "⌘↩"
                } else {
                    "Ctrl+Enter"
                }
            }
```

(The remaining arms, from `CommandAction::TogglePin =>` on, stay as they are.)

After the `impl CommandItem { ... }` block, add:

```rust
/// The palette's delivery rows: "Paste" (Enter) above "Copy" where this
/// platform can paste, otherwise "Copy" alone, which Enter then runs.
fn delivery_command_items(paste_supported: bool) -> Vec<CommandItem> {
    let mut items = Vec::new();
    if paste_supported {
        items.push(CommandItem::new(
            CommandAction::Paste,
            "Paste into previous app",
            "paste insert type enter",
        ));
    }
    items.push(CommandItem::new(
        CommandAction::Copy,
        "Copy to clipboard",
        "copy paste clipboard",
    ));
    items
}

/// What choosing an item does with it: paste it into the window that regains
/// focus once the launcher hides, or only put it on the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    Paste,
    CopyOnly,
}

impl Delivery {
    /// Enter and double-click paste where the platform can; holding the
    /// action modifier asks for copy-only.
    pub(crate) fn for_choice(copy_only_requested: bool, paste_supported: bool) -> Self {
        if paste_supported && !copy_only_requested {
            Delivery::Paste
        } else {
            Delivery::CopyOnly
        }
    }

    pub(crate) fn chosen(copy_only_requested: bool) -> Self {
        Self::for_choice(copy_only_requested, paste_supported())
    }
}

/// The platform's app-shortcut modifier: Cmd (`platform`) on macOS, Ctrl on
/// Linux — GPUI maps `platform` to Super/Meta there.
pub(crate) fn action_modifier_held(modifiers: &gpui::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.platform && !modifiers.control
    } else {
        modifiers.control && !modifiers.platform
    }
}
```

In `command_palette_items`, replace

```rust
        let mut all = vec![
            CommandItem::new(
                CommandAction::Copy,
                "Copy to clipboard",
                "copy paste clipboard",
            ),
            CommandItem::new(
                CommandAction::TogglePin,
```

with

```rust
        let mut all = delivery_command_items(paste_supported());
        all.extend([
            CommandItem::new(
                CommandAction::TogglePin,
```

and close that literal with `]);` instead of `];` (the line after the `CommandAction::Transform` item).

In `execute_command`, replace `CommandAction::Copy => self.copy_selected_to_clipboard(cx),` with:

```rust
            CommandAction::Paste => self.deliver_selected(Delivery::Paste, cx),
            CommandAction::Copy => self.deliver_selected(Delivery::CopyOnly, cx),
```

- [ ] **Step 6: Add `pending_paste` to the view**

In `src/app/state.rs`, after `pub(crate) pending_exit: Option<LauncherExitIntent>,` add:

```rust
    /// A paste prepared by the last delivery, handed to the platform once the
    /// launcher has hidden.
    pub(crate) pending_paste: Option<PasteTicket>,
```

In `LauncherView::new` (`src/app/actions.rs`), after `pending_exit: None,` add `pending_paste: None,`. In `reset_for_show`, after `self.reveal_until = None;` add `self.pending_paste = None;`.

- [ ] **Step 7: Add the write helpers and make every delivery path use them**

In `src/app/actions.rs`, directly above `pub(crate) fn copy_selected_to_clipboard`, add:

```rust
    /// Puts `text` on the clipboard and, for a paste, prepares it. The order
    /// is the same on every path: GPUI write, then `begin_paste`, then the
    /// platform write — on X11 the ticket records the owner the GPUI write
    /// just set, so the paste waits for the platform write to take over.
    fn write_text_for_delivery(
        &mut self,
        text: &str,
        delivery: Delivery,
        cx: &mut Context<Self>,
    ) -> Option<PasteTicket> {
        self.mark_self_clipboard_write(text.as_bytes(), cx);
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        let ticket = match delivery {
            Delivery::Paste => begin_paste(),
            Delivery::CopyOnly => None,
        };
        // On Wayland, the GPUI window must stay alive to serve paste requests.
        // Since Pasta destroys the window on hide, also write via wl-clipboard-rs
        // which forks a background process to serve the data independently.
        #[cfg(target_os = "linux")]
        write_clipboard_text(text);
        ticket
    }

    /// Image counterpart of [`Self::write_text_for_delivery`], same order.
    fn write_image_for_delivery(
        &mut self,
        bytes: Vec<u8>,
        mime_type: &str,
        delivery: Delivery,
        cx: &mut Context<Self>,
    ) -> Option<PasteTicket> {
        let format = ImageFormat::from_mime_type(mime_type).unwrap_or(ImageFormat::Png);
        self.mark_self_clipboard_write(&bytes, cx);
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            format,
            bytes.clone(),
        )));
        let ticket = match delivery {
            Delivery::Paste => begin_paste(),
            Delivery::CopyOnly => None,
        };
        // Same reason as for text: the GPUI window does not outlive the hide.
        #[cfg(target_os = "linux")]
        write_clipboard_image_bytes(&bytes, mime_type);
        ticket
    }
```

Replace the whole of `pub(crate) fn copy_selected_to_clipboard(&mut self, cx: &mut Context<Self>) { ... }` with:

```rust
    pub(crate) fn deliver_selected(&mut self, delivery: Delivery, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(self.selected_index).cloned() else {
            return;
        };

        if item.item_type == ClipboardItemType::Password && !self.can_copy_secret_now(item.id) {
            self.reveal_secret(item.id, Some(delivery), cx);
            return;
        }

        if !item.parameters.is_empty() {
            self.open_parameter_fill_prompt(item.id, &item.parameters, cx);
            return;
        }

        let _ = self.storage.touch_clipboard_item(item.id);

        if let Some(image) = &item.image {
            let Ok(bytes) = std::fs::read(&image.path) else {
                show_macos_notification("Pasta", "Couldn't read image from disk.");
                return;
            };
            self.pending_paste = self.write_image_for_delivery(bytes, &image.mime_type, delivery, cx);
            self.begin_close_transition(LauncherExitIntent::Hide);
            cx.notify();
            return;
        }

        let ticket = self.write_text_for_delivery(&item.content, delivery, cx);
        if item.item_type == ClipboardItemType::Password {
            self.schedule_secret_autoclear(&item.content, cx);
            // Copy-only keeps the secret on screen for its reveal window; a
            // paste hides the launcher like any other item.
            if ticket.is_none() {
                self.revealed_secret_id = Some(item.id);
                self.reveal_until = Some(Instant::now() + Duration::from_secs(12));
                cx.notify();
                return;
            }
        }
        self.pending_paste = ticket;
        self.begin_close_transition(LauncherExitIntent::Hide);
        cx.notify();
    }
```

Replace `copy_index_to_clipboard` with:

```rust
    pub(crate) fn deliver_index(&mut self, index: usize, delivery: Delivery, cx: &mut Context<Self>) {
        self.selected_index = index;
        self.selection_changed_at = Instant::now();
        self.scroll_result_into_view(self.selected_index, ScrollStrategy::Center);
        self.deliver_selected(delivery, cx);
    }
```

In `copy_selected_emoji`, change the signature to `pub(crate) fn copy_selected_emoji(&mut self, delivery: Delivery, cx: &mut Context<Self>)` and replace

```rust
        self.mark_self_clipboard_write(glyph.as_bytes(), cx);
        cx.write_to_clipboard(ClipboardItem::new_string(glyph.to_owned()));
        #[cfg(target_os = "linux")]
        write_clipboard_text(glyph);
```

with

```rust
        self.pending_paste = self.write_text_for_delivery(glyph, delivery, cx);
```

In `commit_parameter_fill_prompt`, change the signature to `pub(crate) fn commit_parameter_fill_prompt(&mut self, delivery: Delivery, cx: &mut Context<Self>)` and replace

```rust
        self.mark_self_clipboard_write(rendered.as_bytes(), cx);
        #[cfg(target_os = "linux")]
        write_clipboard_text(&rendered);
        cx.write_to_clipboard(ClipboardItem::new_string(rendered));
```

with

```rust
        self.pending_paste = self.write_text_for_delivery(&rendered, delivery, cx);
```

(This also changes the fill path's write order to GPUI first, matching the other paths, as the spec requires.)

In `reveal_secret`, change the signature to `pub(crate) fn reveal_secret(&mut self, item_id: i64, then_deliver: Option<Delivery>, cx: &mut Context<Self>)` and replace

```rust
        if copy_after && let Some(ix) = self.items.iter().position(|i| i.id == item.id) {
            self.copy_index_to_clipboard(ix, cx);
            return;
        }
```

with

```rust
        if let Some(delivery) = then_deliver
            && let Some(ix) = self.items.iter().position(|i| i.id == item.id)
        {
            self.deliver_index(ix, delivery, cx);
            return;
        }
```

In `reveal_and_copy_selected_secret`, replace `self.reveal_secret(item.id, true, cx);` with `self.reveal_secret(item.id, Some(Delivery::CopyOnly), cx);` and `self.copy_selected_to_clipboard(cx);` with `self.deliver_selected(Delivery::CopyOnly, cx);`.

- [ ] **Step 8: Route keys and clicks**

In `handle_keystroke`, replace

```rust
        // On macOS, Cmd (modifiers.platform) is the action modifier.
        // On Linux, Ctrl (modifiers.control) is the standard app shortcut
        // modifier — GPUI maps modifiers.platform to Super/Meta on Linux.
        let action_mod = if cfg!(target_os = "macos") {
            modifiers.platform && !modifiers.control
        } else {
            modifiers.control && !modifiers.platform
        };
```

with `let action_mod = action_modifier_held(modifiers);`. In the same function's `"enter" | "return"` arm (the one that checks `showing_emoji_affordance` and `ExportBowl` first), replace `self.copy_selected_to_clipboard(cx);` with `self.deliver_selected(Delivery::chosen(action_mod), cx);`.

In `handle_emoji_search_keystroke`, replace `"enter" | "return" => self.copy_selected_emoji(cx),` with:

```rust
            "enter" | "return" => self.copy_selected_emoji(
                Delivery::chosen(action_modifier_held(&event.keystroke.modifiers)),
                cx,
            ),
```

In `handle_parameter_fill_keystroke`, replace `self.commit_parameter_fill_prompt(cx);` with `self.commit_parameter_fill_prompt(Delivery::chosen(action_modifier_held(modifiers)), cx);`.

In `src/app/view.rs`, add `use super::actions::Delivery;` next to the file's other `use` lines; at line 2527 replace `this.copy_index_to_clipboard(ix, cx);` with `this.deliver_index(ix, Delivery::chosen(false), cx);`; at line 1872 replace `this.copy_selected_emoji(cx);` with `this.copy_selected_emoji(Delivery::chosen(false), cx);`.

- [ ] **Step 9: Hand the ticket to the platform after hiding**

In `src/app/runtime.rs`, `spawn_launcher_transition_loop`, replace the `Some(LauncherExitIntent::Hide) => { ... }` arm with:

```rust
                            Some(LauncherExitIntent::Hide) => {
                                let paste = view.pending_paste.take();
                                #[cfg(target_os = "macos")]
                                {
                                    cx.hide();
                                }
                                #[cfg(target_os = "linux")]
                                {
                                    window.remove_window();
                                    cx.global_mut::<LauncherState>().window = None;
                                }
                                // Requested only now, so the focus wait starts
                                // once the launcher is already going away.
                                if let Some(ticket) = paste {
                                    request_paste(ticket, cx);
                                }
                            }
```

- [ ] **Step 10: Run the tests and the CI checks**

Run: `cargo test --bin pasta-launcher 2>&1 | tail -5`
Expected: all pass, including the four new tests.
Run: `cargo fmt --all && RUSTFLAGS="-D warnings" cargo test 2>&1 | tail -3 && cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning'`
Expected: tests pass; clippy count equals `main`'s.
Run: `grep -rn "copy_selected_to_clipboard\|copy_index_to_clipboard\|copy_after" src`
Expected: no output.

- [ ] **Step 11: Commit**

```bash
git add src/app src/platform/linux/paste.rs src/platform/linux/mod.rs src/platform/macos/paste.rs src/platform/macos/mod.rs
git commit -m "refactor(app): route item delivery through a paste-or-copy choice" -m "- Delivery::{Paste, CopyOnly} chosen from Enter/double-click vs the action modifier
- one write helper fixes the order GPUI write, begin_paste, platform write on every path
- the transition loop hands a prepared paste to the platform after hiding
- platform stubs report no paste support yet, so behaviour is unchanged"
```

---

### Task 2: GNOME — serial bridge worker, terminal list, `paste` client

After this task, on the GNOME extension path Enter and double-click paste (once Task 3's extension is installed; until then the call fails with an unknown-method error and logs `paste skipped`).

**Files:**
- Modify: `src/platform/linux/paste.rs` (replace the stub)
- Modify: `src/platform/linux/mod.rs:798-810` (`write_through_gnome_bridge`) and its `mod tests`
- Modify: `src/platform/linux/gnome_bridge_client.rs`
- Create: `examples/gnome_bridge_paste.rs`; Modify: `examples/gnome_bridge_write.rs`

**Interfaces:**
- Consumes: Task 1's `PasteTicket`, `paste_supported`, `begin_paste`, `request_paste` signatures.
- Produces:
  - `paste::TERMINAL_APP_IDS: &[&str]`, `paste::is_terminal<'a>(ids: impl IntoIterator<Item = &'a str>) -> bool` (Task 4 uses both)
  - `paste::supported_on(path: ClipboardPath) -> bool`; the private `enum Ticket` that Task 4 extends with `X11 { owner_before: u32 }`
  - `gnome_bridge_client::paste(terminal_app_ids: &[&str]) -> Result<(), String>` calling D-Bus `com.pasta.Launcher.ShellBridge1.Paste(as)` (Task 3 implements the server side)
  - `mod.rs`: `fn queue_gnome_paste()`

- [ ] **Step 1: Write the failing tests**

Append to `src/platform/linux/paste.rs` (the stub file; tests are added before the implementation is replaced):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_are_recognised_case_insensitively() {
        assert!(is_terminal(["org.gnome.Ptyxis"]));
        assert!(is_terminal(["ORG.GNOME.PTYXIS"]));
        assert!(is_terminal(["com.mitchellh.ghostty"]));
        assert!(!is_terminal(["org.gnome.TextEditor"]));
        assert!(!is_terminal([""]));
    }

    #[test]
    fn any_offered_id_can_mark_a_terminal() {
        // X11 WM_CLASS offers an instance and a class string.
        assert!(is_terminal(["gnome-terminal-server", "Gnome-terminal"]));
        assert!(is_terminal(["unknown", "kitty"]));
        assert!(!is_terminal(["firefox", "Firefox"]));
    }

    #[test]
    fn only_the_gnome_extension_path_pastes_so_far() {
        assert!(supported_on(ClipboardPath::GnomeExtension));
        assert!(!supported_on(ClipboardPath::Wayland));
        assert!(!supported_on(ClipboardPath::X11));
    }
}
```

Append inside `mod tests` of `src/platform/linux/mod.rs`:

```rust
    #[test]
    fn serial_worker_runs_jobs_in_send_order() {
        let (done_tx, done_rx) = mpsc::channel();
        let jobs = spawn_serial_worker("test-serial-order", move |n: u32| {
            // A slow first job must still finish before the second starts.
            if n == 0 {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            done_tx.send(n).unwrap();
        })
        .unwrap();
        for n in 0..5 {
            jobs.send(n).unwrap();
        }
        let seen: Vec<u32> = (0..5)
            .map(|_| {
                done_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap()
            })
            .collect();
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn serial_worker_survives_a_panicking_job() {
        let (done_tx, done_rx) = mpsc::channel();
        let jobs = spawn_serial_worker("test-serial-panic", move |n: u32| {
            if n == 0 {
                panic!("job 0 fails on purpose");
            }
            done_tx.send(n).unwrap();
        })
        .unwrap();
        jobs.send(0).unwrap();
        jobs.send(1).unwrap();
        assert_eq!(
            done_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            1
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --bin pasta-launcher -- terminal serial_worker only_the_gnome 2>&1 | tail -20`
Expected: compile errors — `is_terminal`, `supported_on`, `ClipboardPath` (not imported), `spawn_serial_worker` not found.

- [ ] **Step 3: Replace the Linux paste stub**

Replace everything above `#[cfg(test)]` in `src/platform/linux/paste.rs` with:

```rust
//! Pasting into the window that regains focus after the launcher hides:
//! through the GNOME Shell extension on GNOME, not at all on other Wayland
//! desktops.

use gpui::App;

use super::{ClipboardPath, clipboard_path, queue_gnome_paste};

/// Windows whose app ID (Wayland `app_id`, or either `WM_CLASS` string on
/// X11) is listed here get Ctrl+Shift+V: terminal emulators paste with it,
/// and give Ctrl+V another meaning. Matched case-insensitively.
pub(crate) const TERMINAL_APP_IDS: &[&str] = &[
    "org.gnome.Ptyxis",
    "org.gnome.Ptyxis.Devel",
    "org.gnome.Console",
    "kgx",
    "org.gnome.Terminal",
    "gnome-terminal",
    "gnome-terminal-server",
    "com.mitchellh.ghostty",
    "kitty",
    "Alacritty",
    "org.wezfurlong.wezterm",
    "foot",
    "footclient",
    "org.kde.konsole",
    "konsole",
    "xfce4-terminal",
    "com.gexperts.Tilix",
    "terminator",
];

/// Whether any of a window's identifiers names a terminal.
pub(crate) fn is_terminal<'a>(ids: impl IntoIterator<Item = &'a str>) -> bool {
    ids.into_iter().any(|id| {
        TERMINAL_APP_IDS
            .iter()
            .any(|terminal| terminal.eq_ignore_ascii_case(id))
    })
}

enum Ticket {
    Gnome,
}

/// Proof that a paste was prepared before the platform clipboard write,
/// carrying what the platform needs to wait for that write.
pub(crate) struct PasteTicket(Ticket);

fn supported_on(path: ClipboardPath) -> bool {
    matches!(path, ClipboardPath::GnomeExtension)
}

/// Whether this session can paste on the user's behalf.
pub(crate) fn paste_supported() -> bool {
    supported_on(clipboard_path())
}

/// Prepares a paste; call it after the GPUI clipboard write and before the
/// platform one. `None` means the delivery falls back to copy-only.
pub(crate) fn begin_paste() -> Option<PasteTicket> {
    match clipboard_path() {
        ClipboardPath::GnomeExtension => Some(PasteTicket(Ticket::Gnome)),
        ClipboardPath::X11 | ClipboardPath::Wayland => None,
    }
}

/// Presses the paste shortcut in the window that next holds focus, once it is
/// not one of Pasta's. Never blocks the caller.
pub(crate) fn request_paste(ticket: PasteTicket, _cx: &mut App) {
    match ticket.0 {
        // Queued behind the clipboard write, so the shell pastes the new content.
        Ticket::Gnome => queue_gnome_paste(),
    }
}
```

- [ ] **Step 4: Add `paste` to the bridge client**

In `src/platform/linux/gnome_bridge_client.rs`, replace the body of `set_clipboard` from `let conn = ...` through the closing `}` of the `if !is_shell_executable(&exe) { ... }` block with:

```rust
    let conn = zbus::blocking::Connection::session().map_err(|err| err.to_string())?;
    let owner = verified_bridge_owner(&conn)?;
```

(The pipe/feeder code after it is unchanged.) Then add, after `set_clipboard`:

```rust
/// Unique name of the bridge's current owner, after checking that it is the
/// installed gnome-shell.
fn verified_bridge_owner(
    conn: &zbus::blocking::Connection,
) -> Result<zbus::names::OwnedUniqueName, String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn).map_err(|err| err.to_string())?;
    let name = zbus::names::BusName::try_from(BRIDGE_NAME).map_err(|err| err.to_string())?;
    let owner = dbus.get_name_owner(name).map_err(|err| err.to_string())?;
    let pid = dbus
        .get_connection_unix_process_id((&owner).into())
        .map_err(|err| err.to_string())?;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).map_err(|err| err.to_string())?;
    if !is_shell_executable(&exe) {
        return Err(format!(
            "{BRIDGE_NAME} is owned by {}, not {SHELL_EXE}",
            exe.display()
        ));
    }
    Ok(owner)
}

/// Calls the bridge's `Paste`: the shell presses Ctrl+Shift+V in the window
/// that next holds focus if that window's app ID is in `terminal_app_ids`,
/// Ctrl+V otherwise, or answers with an error if no such window comes. Blocks
/// until it answers; call it off the UI thread.
pub(crate) fn paste(terminal_app_ids: &[&str]) -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|err| err.to_string())?;
    let owner = verified_bridge_owner(&conn)?;
    conn.call_method(
        Some(owner.as_str()),
        BRIDGE_PATH,
        Some(BRIDGE_IFACE),
        "Paste",
        &(terminal_app_ids,),
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}
```

- [ ] **Step 5: Replace thread-per-write with the serial worker**

In `src/platform/linux/mod.rs`, replace the doc comment and body of `fn write_through_gnome_bridge` (lines 796-810) with:

```rust
/// Work for the GNOME Shell bridge. Done in order on one thread, so a paste
/// request always reaches the shell after the write queued before it.
enum BridgeJob {
    Write { mimetype: String, bytes: Vec<u8> },
    Paste,
}

/// Runs `run` on each job sent to the returned channel, one at a time and in
/// send order, on a thread named `name`. A panicking job is logged and the
/// worker carries on with the next.
fn spawn_serial_worker<J: Send + 'static>(
    name: &str,
    mut run: impl FnMut(J) + Send + 'static,
) -> std::io::Result<mpsc::Sender<J>> {
    let (tx, rx) = mpsc::channel::<J>();
    let thread_name = name.to_owned();
    std::thread::Builder::new()
        .name(thread_name.clone())
        .spawn(move || {
            for job in rx {
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(job)));
                if outcome.is_err() {
                    eprintln!("warning: a {thread_name} job panicked; continuing");
                }
            }
        })?;
    Ok(tx)
}

fn bridge_jobs() -> Option<&'static mpsc::Sender<BridgeJob>> {
    static JOBS: OnceLock<Option<mpsc::Sender<BridgeJob>>> = OnceLock::new();
    JOBS.get_or_init(
        || match spawn_serial_worker("pasta-gnome-bridge", run_bridge_job) {
            Ok(jobs) => Some(jobs),
            Err(err) => {
                eprintln!("warning: could not start the GNOME bridge worker thread: {err}");
                None
            }
        },
    )
    .as_ref()
}

/// Bridge calls wait for the shell (to read a payload, or to find a window to
/// paste into), so they run here, off the UI thread.
fn run_bridge_job(job: BridgeJob) {
    match job {
        BridgeJob::Write { mimetype, bytes } => {
            if let Err(err) = gnome_bridge_client::set_clipboard(&mimetype, bytes) {
                eprintln!("warning: GNOME clipboard write of {mimetype} failed: {err}");
            }
        }
        BridgeJob::Paste => {
            if let Err(err) = gnome_bridge_client::paste(paste::TERMINAL_APP_IDS) {
                eprintln!("warning: paste skipped: {err}");
            }
        }
    }
}

fn queue_bridge_job(job: BridgeJob) {
    let Some(jobs) = bridge_jobs() else {
        return;
    };
    if jobs.send(job).is_err() {
        eprintln!("warning: the GNOME bridge worker has stopped; request dropped");
    }
}

fn write_through_gnome_bridge(mimetype: &str, bytes: Vec<u8>) {
    queue_bridge_job(BridgeJob::Write {
        mimetype: mimetype.to_owned(),
        bytes,
    });
}

/// Asks the shell to paste once every write queued before this has landed.
fn queue_gnome_paste() {
    queue_bridge_job(BridgeJob::Paste);
}
```

- [ ] **Step 6: Add the paste example and keep both examples warning-free**

Create `examples/gnome_bridge_paste.rs`:

```rust
//! Test-only: asks the GNOME extension bridge to paste, using Pasta's real
//! client code. The isolated harness in gnome-extension/tests runs it under
//! the name `pasta-launcher`, the only caller the bridge serves.
//!
//!     cargo build --example gnome_bridge_paste
//!     gnome_bridge_paste [terminal_app_id...]

#[cfg(target_os = "linux")]
#[allow(dead_code)] // the shared client module also carries set_clipboard
#[path = "../src/platform/linux/gnome_bridge_client.rs"]
mod gnome_bridge_client;

#[cfg(target_os = "linux")]
fn main() {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    match gnome_bridge_client::paste(&ids) {
        Ok(()) => println!("PASTED"),
        Err(err) => {
            eprintln!("refused: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
```

In `examples/gnome_bridge_write.rs`, add `#[allow(dead_code)] // the shared client module also carries paste` between `#[cfg(target_os = "linux")]` and `#[path = ...]` above `mod gnome_bridge_client;`.

- [ ] **Step 7: Run the tests and CI checks**

Run: `cargo test --bin pasta-launcher 2>&1 | tail -5`
Expected: all pass, including the five new tests (the panicking-job test prints a panic message to stderr; that is expected).
Run: `cargo fmt --all && RUSTFLAGS="-D warnings" cargo test 2>&1 | tail -3 && cargo build --examples && cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning'`
Expected: pass; both examples build; clippy count equals `main`'s.

- [ ] **Step 8: Commit**

```bash
git add src/platform/linux examples
git commit -m "feat(linux): paste through the GNOME extension on Enter" -m "- GNOME bridge writes and paste requests go through one serial worker, so a paste always follows its write
- terminal app IDs get Ctrl+Shift+V; the list is sent with each Paste call
- gnome_bridge_paste example drives the real client from the extension harness"
```

---

### Task 3: Extension — `Paste` method and nested-shell scenario

**Files:**
- Modify: `gnome-extension/clipboard@pasta.launcher/bridge.js`, `gnome-extension/clipboard@pasta.launcher/metadata.json`
- Modify: `gnome-extension/tests/clip_tool.py`, `gnome-extension/tests/run-all.sh`
- Create: `gnome-extension/tests/scenario-paste.sh`

**Interfaces:**
- Consumes: Task 2's `gnome_bridge_paste` example (`PASTED` on stdout, `refused: <error>` on stderr with exit 1) and the D-Bus signature `Paste(as terminal_app_ids)`.
- Produces: `com.pasta.Launcher.ShellBridge1.Paste(as)` — returns nothing on success; `AccessDenied` for callers whose executable basename is not `pasta-launcher` (log `pasta-clipboard: rejected Paste from <exe>`); `org.freedesktop.DBus.Error.Failed` with message `no window other than the caller's took focus within 1000 ms` on timeout.

- [ ] **Step 1: Write the failing scenario**

Add to `COMMANDS` in `gnome-extension/tests/clip_tool.py` the entries `"key-catcher": key_catcher,` and `"poke-paste": poke_paste,`, and add these functions above `COMMANDS`:

```python
def key_catcher(app_id, seconds):
    """Show a window under `app_id` that prints each Ctrl+V / Ctrl+Shift+V it receives."""

    def activate(app):
        win = Gtk.ApplicationWindow(application=app)
        keys = Gtk.EventControllerKey()

        def pressed(_controller, keyval, _keycode, state):
            if Gdk.keyval_to_lower(keyval) == Gdk.KEY_v and state & Gdk.ModifierType.CONTROL_MASK:
                shifted = state & Gdk.ModifierType.SHIFT_MASK
                print("KEY ctrl+shift+v" if shifted else "KEY ctrl+v", flush=True)
            return False

        keys.connect("key-pressed", pressed)
        win.add_controller(keys)
        win.present()
        print("WINDOW shown", flush=True)
        GLib.timeout_add_seconds(int(seconds), lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id=app_id, flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def poke_paste():
    """Call Paste as a process that is not pasta-launcher."""
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    try:
        connection.call_sync(
            "com.pasta.Launcher.ShellBridge", "/com/pasta/Launcher/ShellBridge",
            "com.pasta.Launcher.ShellBridge1", "Paste",
            GLib.Variant("(as)", ([],)), None, Gio.DBusCallFlags.NONE, 5000, None)
        print("POKE accepted", flush=True)
    except GLib.Error as error:
        print(f"POKE rejected: {error.message}", flush=True)
```

Create `gnome-extension/tests/scenario-paste.sh` (and `chmod +x` it):

```sh
#!/bin/sh
# The bridge presses the paste shortcut in the focused window: Ctrl+V in an
# ordinary app, Ctrl+Shift+V in one the caller lists as a terminal. It gives
# up when no other window takes focus and refuses callers that are not Pasta.
set -eu
. "$HERE/lib.sh"

paste_as_pasta() {
    cp "$REPO_ROOT/target/debug/examples/gnome_bridge_paste" "$NEST/writer/pasta-launcher"
    "$NEST/writer/pasta-launcher" "$@"
}

# 1. Nothing to paste into: no window is focused.
if paste_as_pasta dev.pasta.FakeTerminal 2>"$NEST/paste-none.err"; then
    echo "FAIL Paste succeeded with no window to paste into"
    exit 1
fi
grep -q "no window other than the caller's took focus within 1000 ms" "$NEST/paste-none.err" \
    || { echo "FAIL unexpected error: $(cat "$NEST/paste-none.err")"; exit 1; }

# 2. An ordinary app gets Ctrl+V.
python3 "$HERE/clip_tool.py" key-catcher dev.pasta.KeyCatcher 15 >"$NEST/catcher.log" 2>&1 &
CATCHER_PID=$!
wait_for_line "$NEST/catcher.log" "WINDOW shown" 10
out=$(paste_as_pasta dev.pasta.FakeTerminal)
[ "$out" = "PASTED" ] || { echo "FAIL paste said: $out"; exit 1; }
wait_for_line "$NEST/catcher.log" "^KEY ctrl\+v$" 5
refute_line "$NEST/catcher.log" "^KEY ctrl\+shift\+v$"
kill "$CATCHER_PID" 2>/dev/null || true
wait "$CATCHER_PID" 2>/dev/null || true

# 3. A listed terminal gets Ctrl+Shift+V.
python3 "$HERE/clip_tool.py" key-catcher dev.pasta.FakeTerminal 15 >"$NEST/terminal.log" 2>&1 &
CATCHER_PID=$!
wait_for_line "$NEST/terminal.log" "WINDOW shown" 10
out=$(paste_as_pasta dev.pasta.FakeTerminal)
[ "$out" = "PASTED" ] || { echo "FAIL paste said: $out"; exit 1; }
wait_for_line "$NEST/terminal.log" "^KEY ctrl\+shift\+v$" 5
refute_line "$NEST/terminal.log" "^KEY ctrl\+v$"
kill "$CATCHER_PID" 2>/dev/null || true
wait "$CATCHER_PID" 2>/dev/null || true

# 4. A caller that is not pasta-launcher is refused.
poke=$(python3 "$HERE/clip_tool.py" poke-paste)
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected: $poke"; exit 1 ;;
esac
wait_for_line "$SHELL_LOG" "pasta-clipboard: rejected Paste from /usr/bin/python3" 5
echo "PASS scenario-paste"
```

In `gnome-extension/tests/run-all.sh`, change the build line to `cargo build --manifest-path "$HERE/../../Cargo.toml" --bin pasta-launcher --example gnome_bridge_write --example gnome_bridge_paste || exit 1` and the loop list to `for scenario in capture startup write refusals hide paste; do`.

- [ ] **Step 2: Run the scenario to verify it fails**

Stop the live Pasta first (Global Constraints). Run: `cargo build --example gnome_bridge_paste && gnome-extension/tests/nested-shell.sh gnome-extension/tests/scenario-paste.sh`
Expected: `FAIL unexpected error:` naming `UnknownMethod` (the extension has no `Paste` yet).

- [ ] **Step 3: Implement `Paste` in the extension**

In `gnome-extension/clipboard@pasta.launcher/bridge.js`:

Add `import Clutter from 'gi://Clutter';` above `import Gio from 'gi://Gio';`, and change the peer import to `import {executableOf, exeBasename, pidOf} from './peer.js';`.

After `const MAX_BYTES = ...;` add `const FOCUS_TIMEOUT_MS = 1000;`. In `BRIDGE_XML`, after the `SetClipboard` method element add:

```xml
    <method name="Paste">
      <arg type="as" name="terminal_app_ids" direction="in"/>
    </method>
```

At the top of the `ShellBridge` constructor add:

```js
        this._keyboard = null;
        this._focusWaits = new Set();
```

Add these methods to `ShellBridge`, after `_setClipboard`:

```js
    async PasteAsync(params, invocation) {
        // As with SetClipboardAsync: every path answers, and this catch makes
        // sure an unexpected throw does too.
        try {
            await this._paste(params, invocation);
        } catch (e) {
            console.warn(`pasta-clipboard: Paste failed: ${e.message}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', e.message);
        }
    }

    async _paste([terminalAppIds], invocation) {
        let callerPid, exe;
        try {
            callerPid = await pidOf(invocation.get_sender());
            exe = GLib.file_read_link(`/proc/${callerPid}/exe`);
        } catch (e) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `could not identify caller: ${e.message}`);
            return;
        }
        if (exeBasename(exe) !== ALLOWED_CALLER_EXE) {
            console.warn(`pasta-clipboard: rejected Paste from ${exe}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `${exe} is not ${ALLOWED_CALLER_EXE}`);
            return;
        }
        const window = await this._focusAwayFrom(callerPid);
        if (!window) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed',
                `no window other than the caller's took focus within ${FOCUS_TIMEOUT_MS} ms`);
            return;
        }
        const appId = (window.get_wm_class() ?? '').toLowerCase();
        const terminal = terminalAppIds.some(id => id.toLowerCase() === appId);
        this._press(terminal
            ? [Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_v]
            : [Clutter.KEY_Control_L, Clutter.KEY_v]);
        invocation.return_value(null);
    }

    /** Resolves with the focused window once it is not `pid`'s, or with null after FOCUS_TIMEOUT_MS. */
    _focusAwayFrom(pid) {
        const display = global.display;
        const usable = () => {
            const window = display.focus_window;
            return window && window.get_pid() !== pid ? window : null;
        };
        const now = usable();
        if (now)
            return Promise.resolve(now);
        return new Promise(resolve => {
            let focusId = 0;
            let timeoutId = 0;
            const finish = window => {
                if (focusId)
                    display.disconnect(focusId);
                if (timeoutId)
                    GLib.source_remove(timeoutId);
                focusId = timeoutId = 0;
                this._focusWaits.delete(finish);
                resolve(window);
            };
            focusId = display.connect('notify::focus-window', () => {
                const window = usable();
                if (window)
                    finish(window);
            });
            timeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, FOCUS_TIMEOUT_MS, () => {
                timeoutId = 0;
                finish(null);
                return GLib.SOURCE_REMOVE;
            });
            this._focusWaits.add(finish);
        });
    }

    /** Presses `keyvals` in order and releases them in reverse, through a virtual keyboard. */
    _press(keyvals) {
        if (!this._keyboard) {
            const seat = global.stage.context.get_backend().get_default_seat();
            this._keyboard = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        }
        let time = GLib.get_monotonic_time();
        for (const keyval of keyvals)
            this._keyboard.notify_keyval(time++, keyval, Clutter.KeyState.PRESSED);
        for (const keyval of [...keyvals].reverse())
            this._keyboard.notify_keyval(time++, keyval, Clutter.KeyState.RELEASED);
    }
```

At the top of `destroy()` add:

```js
        for (const finish of [...this._focusWaits])
            finish(null);
        this._keyboard?.run_dispose();
        this._keyboard = null;
```

`executableOf` stays imported for `_setClipboard`.

In `metadata.json`, set `"version": 3` and the description to `"Lets the Pasta clipboard manager see, set and paste the clipboard on GNOME, and keeps Pasta's windows out of the dock, Alt+Tab and the overview."`.

- [ ] **Step 4: Run the scenario to verify it passes**

Run: `gnome-extension/tests/nested-shell.sh gnome-extension/tests/scenario-paste.sh`
Expected: `PASS scenario-paste`.

If case 2 or 3 times out waiting for a `KEY` line while `PASTED` was printed, the virtual keyboard's events are not reaching clients in the headless shell. Do not weaken the test: stop and report this with `$NEST/shell.log` and the catcher log, because the spec's GNOME mechanism rests on it.

- [ ] **Step 5: Run the whole extension suite**

Run: `gnome-extension/tests/run-all.sh`
Expected: every scenario prints `PASS`; exit status 0. Afterwards restart the live Pasta with `gtk-launch com.pasta.launcher`.

- [ ] **Step 6: Commit**

```bash
git add gnome-extension
git commit -m "feat(gnome-extension): add a Paste method to the shell bridge" -m "- waits up to 1000 ms for a window that is not the caller's to take focus
- presses Ctrl+Shift+V for app IDs the caller lists as terminals, Ctrl+V otherwise, through a virtual keyboard
- refuses callers that are not pasta-launcher, like SetClipboard
- nested-shell scenario covers both shortcuts, the timeout and the refusal
- extension version 3"
```

---

### Task 4: X11 — XTest paste

No X11 session is available on the reference machine (GNOME 50 has none and `Xvfb` is not installed), so this task's end-to-end check is left to the maintainer's manual pass (Task 6). Unit tests cover the parsing and lookup.

**Files:**
- Create: `src/platform/linux/paste/x11.rs`
- Modify: `src/platform/linux/paste.rs`, `Cargo.toml` (line 118, `x11rb`)

**Interfaces:**
- Consumes: Task 2's `Ticket`, `supported_on`, `is_terminal`.
- Produces: `x11::clipboard_owner() -> Option<u32>`, `x11::paste(owner_before: u32) -> Result<(), String>`, `x11::wm_class_names(raw: &[u8]) -> Vec<&str>`, `x11::keycode_for_keysym(min_keycode: u8, keysyms_per_keycode: u8, keysyms: &[u32], wanted: u32) -> Option<u8>`.

- [ ] **Step 1: Enable the XTest binding**

In `Cargo.toml`, change `x11rb = { version = "0.13.2", features = ["randr", "xfixes"] }` to `x11rb = { version = "0.13.2", features = ["randr", "xfixes", "xtest"] }`.

- [ ] **Step 2: Write the failing tests**

Create `src/platform/linux/paste/x11.rs` containing only:

```rust
//! XTest paste for X11 sessions.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_yields_instance_and_class() {
        assert_eq!(
            wm_class_names(b"gnome-terminal-server\0Gnome-terminal\0"),
            vec!["gnome-terminal-server", "Gnome-terminal"]
        );
        assert_eq!(wm_class_names(b"kitty\0kitty"), vec!["kitty", "kitty"]);
        assert!(wm_class_names(b"").is_empty());
    }

    #[test]
    fn keycodes_are_found_by_keysym() {
        // Keycodes 8..=10, two keysyms each.
        let keysyms = [0xffe3, 0, 0x0076, 0x0056, 0xffe1, 0];
        assert_eq!(keycode_for_keysym(8, 2, &keysyms, 0xffe3), Some(8));
        assert_eq!(keycode_for_keysym(8, 2, &keysyms, 0x0076), Some(9));
        assert_eq!(keycode_for_keysym(8, 2, &keysyms, 0xffe1), Some(10));
        assert_eq!(keycode_for_keysym(8, 2, &keysyms, 0x0071), None);
        assert_eq!(keycode_for_keysym(8, 0, &keysyms, 0xffe3), None);
    }
}
```

In `src/platform/linux/paste.rs`, add `mod x11;` below the module doc comment, and in its `mod tests` replace `only_the_gnome_extension_path_pastes_so_far` with:

```rust
    #[test]
    fn gnome_and_x11_paste_other_wayland_desktops_do_not() {
        assert!(supported_on(ClipboardPath::GnomeExtension));
        assert!(supported_on(ClipboardPath::X11));
        assert!(!supported_on(ClipboardPath::Wayland));
    }
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test --bin pasta-launcher -- wm_class keycodes_are gnome_and_x11 2>&1 | tail -20`
Expected: compile errors for `wm_class_names` and `keycode_for_keysym`; `gnome_and_x11...` would fail on the X11 assertion.

- [ ] **Step 4: Implement the X11 paste**

Insert above `#[cfg(test)]` in `src/platform/linux/paste/x11.rs`:

```rust
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, Window,
};
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::wrapper::ConnectionExt as _;

const XK_CONTROL_L: u32 = 0xffe3;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_V: u32 = 0x0076;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const WAIT_LIMIT: Duration = Duration::from_millis(1000);

fn intern(conn: &impl Connection, name: &[u8]) -> Option<u32> {
    let atom = conn.intern_atom(false, name).ok()?.reply().ok()?.atom;
    (atom != x11rb::NONE).then_some(atom)
}

fn selection_owner(conn: &impl Connection, selection: u32) -> Option<Window> {
    Some(conn.get_selection_owner(selection).ok()?.reply().ok()?.owner)
}

fn first_u32(conn: &impl Connection, window: Window, property: u32, kind: AtomEnum) -> Option<u32> {
    let reply = conn
        .get_property(false, window, property, kind, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
}

/// The window that owns the CLIPBOARD selection right now (`x11rb::NONE` when
/// nobody does), or `None` when the X server cannot be asked.
pub(super) fn clipboard_owner() -> Option<Window> {
    let (conn, _) = x11rb::connect(None).ok()?;
    let clipboard = intern(&conn, b"CLIPBOARD")?;
    selection_owner(&conn, clipboard)
}

/// The strings of a `WM_CLASS` value: instance name, then class name.
pub(super) fn wm_class_names(raw: &[u8]) -> Vec<&str> {
    raw.split(|&byte| byte == 0)
        .filter(|part| !part.is_empty())
        .filter_map(|part| std::str::from_utf8(part).ok())
        .collect()
}

/// The first keycode whose keysyms include `wanted`, in a `GetKeyboardMapping`
/// reply that starts at `min_keycode`.
pub(super) fn keycode_for_keysym(
    min_keycode: u8,
    keysyms_per_keycode: u8,
    keysyms: &[u32],
    wanted: u32,
) -> Option<u8> {
    if keysyms_per_keycode == 0 {
        return None;
    }
    let index = keysyms
        .chunks(usize::from(keysyms_per_keycode))
        .position(|syms| syms.contains(&wanted))?;
    u8::try_from(usize::from(min_keycode) + index).ok()
}

/// Waits until the CLIPBOARD owner is no longer `owner_before` and a window
/// that is not this process's is active, then presses the paste shortcut
/// there with XTest. The platform clipboard write hands ownership to a forked
/// helper process after it has returned, which is why the owner is waited for.
pub(super) fn paste(owner_before: Window) -> Result<(), String> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|err| err.to_string())?;
    let root = conn.setup().roots[screen_num].root;
    if conn
        .extension_information(xtest::X11_EXTENSION_NAME)
        .map_err(|err| err.to_string())?
        .is_none()
    {
        return Err("the X server has no XTEST extension".to_owned());
    }
    let clipboard = intern(&conn, b"CLIPBOARD").ok_or("could not intern CLIPBOARD")?;
    let active = intern(&conn, b"_NET_ACTIVE_WINDOW").ok_or("could not intern _NET_ACTIVE_WINDOW")?;
    let pid = intern(&conn, b"_NET_WM_PID").ok_or("could not intern _NET_WM_PID")?;
    let our_pid = std::process::id();

    let deadline = Instant::now() + WAIT_LIMIT;
    let target = loop {
        let owner = selection_owner(&conn, clipboard).unwrap_or(x11rb::NONE);
        let owner_changed = owner != x11rb::NONE && owner != owner_before;
        let focused =
            first_u32(&conn, root, active, AtomEnum::WINDOW).filter(|&window| window != x11rb::NONE);
        if owner_changed
            && let Some(window) = focused
            && first_u32(&conn, window, pid, AtomEnum::CARDINAL) != Some(our_pid)
        {
            break window;
        }
        if Instant::now() >= deadline {
            let limit = WAIT_LIMIT.as_millis();
            return Err(if owner_changed {
                format!("no window other than Pasta's was active within {limit} ms")
            } else {
                format!("the clipboard owner did not change within {limit} ms")
            });
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let wm_class = conn
        .get_property(false, target, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .map_err(|err| err.to_string())?
        .reply()
        .map(|reply| reply.value)
        .unwrap_or_default();
    let terminal = super::is_terminal(wm_class_names(&wm_class));

    let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
    let mapping = conn
        .get_keyboard_mapping(min, max - min + 1)
        .map_err(|err| err.to_string())?
        .reply()
        .map_err(|err| err.to_string())?;
    let keycode = |keysym: u32| {
        keycode_for_keysym(min, mapping.keysyms_per_keycode, &mapping.keysyms, keysym)
            .ok_or_else(|| format!("no keycode for keysym {keysym:#x}"))
    };
    let mut keys = vec![keycode(XK_CONTROL_L)?];
    if terminal {
        keys.push(keycode(XK_SHIFT_L)?);
    }
    keys.push(keycode(XK_V)?);

    for &key in &keys {
        conn.xtest_fake_input(KEY_PRESS_EVENT, key, x11rb::CURRENT_TIME, root, 0, 0, 0)
            .map_err(|err| err.to_string())?;
    }
    for &key in keys.iter().rev() {
        conn.xtest_fake_input(KEY_RELEASE_EVENT, key, x11rb::CURRENT_TIME, root, 0, 0, 0)
            .map_err(|err| err.to_string())?;
    }
    conn.sync().map_err(|err| err.to_string())
}
```

- [ ] **Step 5: Wire X11 into the Linux paste entry points**

In `src/platform/linux/paste.rs`:

Change the module doc's first sentence to `//! Pasting into the window that regains focus after the launcher hides: through the GNOME Shell extension on GNOME, through XTest on X11, and not at all on other Wayland desktops.` (keep it wrapped like the original comment).

Replace `enum Ticket { Gnome, }` with:

```rust
enum Ticket {
    Gnome,
    /// The CLIPBOARD owner right after the GPUI write; the paste waits for
    /// the platform write to replace it.
    X11 { owner_before: u32 },
}
```

Replace `fn supported_on` with:

```rust
fn supported_on(path: ClipboardPath) -> bool {
    matches!(path, ClipboardPath::GnomeExtension | ClipboardPath::X11)
}
```

Replace the body of `begin_paste` with:

```rust
    match clipboard_path() {
        ClipboardPath::GnomeExtension => Some(PasteTicket(Ticket::Gnome)),
        ClipboardPath::X11 => match x11::clipboard_owner() {
            Some(owner_before) => Some(PasteTicket(Ticket::X11 { owner_before })),
            None => {
                eprintln!("warning: paste skipped: could not read the X11 clipboard owner");
                None
            }
        },
        ClipboardPath::Wayland => None,
    }
```

Replace the body of `request_paste` with:

```rust
    match ticket.0 {
        // Queued behind the clipboard write, so the shell pastes the new content.
        Ticket::Gnome => queue_gnome_paste(),
        Ticket::X11 { owner_before } => {
            let spawned = std::thread::Builder::new()
                .name("pasta-x11-paste".to_owned())
                .spawn(move || {
                    if let Err(err) = x11::paste(owner_before) {
                        eprintln!("warning: paste skipped: {err}");
                    }
                });
            if let Err(err) = spawned {
                eprintln!("warning: paste skipped: could not start the X11 paste thread: {err}");
            }
        }
    }
```

- [ ] **Step 6: Run the tests and CI checks**

Run: `cargo test --bin pasta-launcher 2>&1 | tail -5`
Expected: all pass, including the three new or replaced tests.
Run: `cargo fmt --all && RUSTFLAGS="-D warnings" cargo test 2>&1 | tail -3 && cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning'`
Expected: pass; clippy count equals `main`'s.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/platform/linux/paste.rs src/platform/linux/paste
git commit -m "feat(linux): paste on X11 through XTest" -m "- records the CLIPBOARD owner after the GPUI write and waits for xclip to replace it before pasting
- waits up to 1000 ms for a non-Pasta active window, classifies it by WM_CLASS, sends the shortcut with XTest
- enables x11rb's xtest feature"
```

---

### Task 5: macOS — Cmd+V through CGEvent

No macOS machine is available: this task is verified by CI's macOS build and the maintainer's manual pass on a Mac. Locally only Linux compiles; the macOS file is compiled by CI.

**Files:**
- Modify: `src/platform/macos/paste.rs` (replace the stub)

**Interfaces:**
- Consumes: Task 1's signatures.
- Produces: the same four items for macOS, now live.

- [ ] **Step 1: Replace the macOS stub**

Replace `src/platform/macos/paste.rs` with:

```rust
//! Pasting into the app that becomes frontmost after the launcher hides, by
//! posting Cmd+V through Quartz Event Services, which requires the
//! Accessibility permission.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::*;

const KCG_HID_EVENT_TAP: u32 = 0;
const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
/// Virtual key code of the ANSI-layout V key.
const KVK_ANSI_V: u16 = 0x09;
const POLL_INTERVAL: Duration = Duration::from_millis(16);
/// About 1000 ms at `POLL_INTERVAL`.
const POLL_ATTEMPTS: u32 = 63;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: id) -> u8;
    static kAXTrustedCheckOptionPrompt: id;
    fn CGEventCreateKeyboardEvent(source: *const c_void, keycode: u16, key_down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// Proof that a paste was prepared before the clipboard write.
pub(crate) struct PasteTicket(());

/// Whether this session can paste on the user's behalf.
pub(crate) fn paste_supported() -> bool {
    true
}

/// Prepares a paste. Without the Accessibility permission, asks for it (once
/// per run, through the system's own dialog) and returns `None`, so the
/// delivery falls back to copy-only.
pub(crate) fn begin_paste() -> Option<PasteTicket> {
    if unsafe { AXIsProcessTrusted() } != 0 {
        return Some(PasteTicket(()));
    }
    static PROMPTED: AtomicBool = AtomicBool::new(false);
    if !PROMPTED.swap(true, Ordering::Relaxed) {
        unsafe {
            let yes: id = msg_send![class!(NSNumber), numberWithBool: YES];
            let options: id = msg_send![class!(NSDictionary),
                dictionaryWithObject: yes
                forKey: kAXTrustedCheckOptionPrompt
            ];
            AXIsProcessTrustedWithOptions(options);
        }
    }
    eprintln!("warning: paste skipped: Pasta lacks the Accessibility permission; copied only");
    None
}

fn frontmost_pid() -> Option<i32> {
    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![workspace, frontmostApplication];
        if app == nil {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        Some(pid)
    }
}

fn post_command_v() {
    unsafe {
        for key_down in [true, false] {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), KVK_ANSI_V, key_down);
            if event.is_null() {
                eprintln!("warning: paste skipped: could not create a key event");
                return;
            }
            CGEventSetFlags(event, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }
}

/// Posts Cmd+V once another app is frontmost, checking on the main thread
/// every `POLL_INTERVAL` for up to about a second. Never blocks the caller.
pub(crate) fn request_paste(_ticket: PasteTicket, cx: &mut App) {
    let our_pid = std::process::id() as i32;
    cx.spawn(async move |cx| {
        for _ in 0..POLL_ATTEMPTS {
            if frontmost_pid().is_some_and(|pid| pid != our_pid) {
                post_command_v();
                return;
            }
            cx.background_executor().timer(POLL_INTERVAL).await;
        }
        eprintln!("warning: paste skipped: no other app became frontmost within 1000 ms");
    })
    .detach();
}
```

- [ ] **Step 2: Check what can be checked locally**

Run: `cargo fmt --all -- --check && RUSTFLAGS="-D warnings" cargo test 2>&1 | tail -3`
Expected: pass (the macOS file is excluded from the Linux build, but `rustfmt` formats it).
Read the file once more against the names in `src/main.rs`'s macOS imports (`id`, `nil`, `YES`, `class!`, `msg_send!`, `Duration`, `App`) and confirm each is imported there; if one is not, import it explicitly in `paste.rs` rather than editing `main.rs`.

- [ ] **Step 3: Commit**

```bash
git add src/platform/macos/paste.rs
git commit -m "feat(macos): paste with Cmd+V after the launcher hides" -m "- posts Cmd+V through CGEvent once another app is frontmost, waiting up to about a second
- without the Accessibility permission, prompts once per run and copies only"
```

CI's macOS job is the first compile of this file; if it fails there, fix forward in a `fix(macos):` commit.

---

### Task 6: Documentation, full verification, manual pass

**Files:**
- Modify: `docs/linux-platform-notes.md`, `SMOKE_TEST_CHECKLIST.md`, `AGENTS.md`

- [ ] **Step 1: Linux platform notes**

In `docs/linux-platform-notes.md`, add a new section directly before `## Single-instance guard + \`--show\` trigger`:

```markdown
## Pasting on Enter

Enter and double-click paste the chosen item into the window that regains focus; Ctrl+Enter only copies (`Delivery` in `src/app/actions.rs`). Which session types can paste is decided by the clipboard path (`paste_supported()` in `src/platform/linux/paste.rs`): the GNOME extension path and X11 paste, every other Wayland desktop copies only — KDE, Sway and Hyprland would each need their own injection protocol, and `/dev/uinput` is root-only on stock installs. Terminals get Ctrl+Shift+V and everything else Ctrl+V, chosen by app ID against `TERMINAL_APP_IDS`; the GNOME extension receives that list with each request, so adding a terminal needs no extension update.

- **GNOME:** `com.pasta.Launcher.ShellBridge1.Paste(as)` waits up to 1000 ms for a window whose PID is not the caller's to take focus, then presses the shortcut through a Clutter virtual keyboard (the mechanism GNOME's on-screen keyboard uses). Bridge writes and paste requests share one serial worker thread, which is what guarantees the extension sets the clipboard before it pastes. An extension older than version 3 answers `UnknownMethod`; Pasta logs `paste skipped` and the item stays copied.
- **X11:** verified by `strace` that `xclip` takes the CLIPBOARD selection in its forked child, after the process Pasta waits on has exited, and from GPUI 0.2.2's source that its X11 write first makes GPUI's own window the owner (with an empty string for images). So `begin_paste` records the owner right after the GPUI write and the paste thread waits for it to change before pressing keys through XTest. Not yet exercised end to end on a real X11 session.
- Pasting never restores the previous clipboard content; that is a deliberate decision, not an omission.
```

- [ ] **Step 2: Smoke checklist**

In `SMOKE_TEST_CHECKLIST.md`:

Under `### Clipboard Actions`, replace `- Press \`Enter\` on a normal item and confirm it copies.` with:

```markdown
- Focus a text field in another app, open the launcher, press `Enter` on a normal item, and confirm the launcher hides and the item is pasted into that field.
- Repeat with a double click on a row.
- Press `Ctrl+Enter` (`Cmd+Enter` on macOS) on an item and confirm it is copied but not pasted.
- Paste an image item into an app that accepts images (e.g. a chat or document editor).
- With a terminal focused (Ptyxis, Ghostty, GNOME Terminal), press `Enter` on an item and confirm it is pasted, not a literal `^V`.
- Open the launcher from the desktop with no window focused, press `Enter`, and confirm nothing is typed anywhere and the item is on the clipboard.
- On a desktop that cannot paste (KDE, Sway, Hyprland on Wayland), confirm `Enter` copies exactly as before and the command palette shows only "Copy" with `Enter`.
```

Under `### Secret Flow`, add:

```markdown
- Press `Enter` on a masked secret with a text field focused behind the launcher: confirm authentication, then the secret is pasted and the launcher hides; after 30 seconds the clipboard is cleared if auto-clear is on.
- Press `Ctrl+Enter` on a masked secret and confirm today's behaviour: authenticate, copy, launcher stays open with the 12-second reveal.
```

Under `### Editors`, replace `- Open parameter fill flow by copying a parameterized item.` with `- Open the parameter fill flow with \`Enter\` on a parameterized item, fill it, press \`Enter\`, and confirm the rendered text is pasted; repeat with \`Ctrl+Enter\` to commit and confirm it is copied only.`

Under `### Emoji Picker`, replace `- Pick an emoji with \`Enter\` and confirm the launcher closes and the glyph is on the clipboard.` with `- Pick an emoji with \`Enter\` and confirm the launcher closes and the glyph is pasted into the focused field; \`Ctrl+Enter\` copies it only.`

Under `### GNOME clipboard extension (GNOME 50, live session)`, add: `- After upgrading to extension version 3 and logging out and back in, confirm Enter pastes; before the re-login, confirm Enter still copies and Pasta's log shows \`paste skipped\`.`

On macOS, add under `### Clipboard Actions`: `- (macOS) On the first paste without the Accessibility permission, confirm the system prompt appears and the item is copied only; after granting it, confirm Enter pastes.`

- [ ] **Step 3: AGENTS.md**

In `AGENTS.md`, in the `src/platform/` bullet, after the sentence that ends `...driving the launcher's capture banner.`, add: `Pasting on Enter lives in \`platform/linux/paste.rs\` (GNOME through the extension's \`Paste\` method, X11 through XTest in \`paste/x11.rs\`) and \`platform/macos/paste.rs\` (CGEvent); other Wayland desktops copy only.`

- [ ] **Step 4: Full verification**

Run: `cargo fmt --all -- --check && RUSTFLAGS="-D warnings" cargo test 2>&1 | tail -3 && RUSTFLAGS="-D warnings" cargo build --release 2>&1 | tail -2 && cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning'`
Expected: everything passes; clippy count equals `main`'s.
Run (live Pasta stopped first): `gnome-extension/tests/run-all.sh`
Expected: every scenario `PASS`. Restart the live Pasta afterwards.

- [ ] **Step 5: Commit**

```bash
git add docs/linux-platform-notes.md SMOKE_TEST_CHECKLIST.md AGENTS.md
git commit -m "docs: document pasting on Enter" -m "- Linux platform notes: which sessions paste, the GNOME and X11 mechanisms and the xclip ordering finding
- smoke checklist cases for paste, copy-only, terminals, secrets, emoji, parameter fill and macOS permission
- AGENTS.md points at the paste modules"
```

- [ ] **Step 6: Hand the remaining checks to the maintainer**

These need a human and cannot be done by the executor; report them as the end-of-run handoff, together with what Steps 4 and Task 3 verified:

1. Install the build (`scripts/install-linux-app.sh`), log out and back in so GNOME loads extension version 3, then walk the new smoke-checklist cases on GNOME 50 Wayland — especially Ptyxis, Ghostty and GNOME Terminal, which confirm the terminal app IDs.
2. On an X11 session (any non-GNOME desktop under Xorg): Enter pastes into a text field and a terminal; an image pastes as an image.
3. On a Mac: the Accessibility prompt flow and Enter/Cmd+Enter, after CI's macOS build passes.
