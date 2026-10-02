# GNOME Clipboard Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On GNOME 50, Pasta captures clipboard history, writes to the clipboard and captures the clipboard present at start-up through a bundled GNOME Shell extension (`clipboard@pasta.launcher`), with an in-launcher banner and Enable button when the extension is not usable.

**Architecture:** The extension pushes every clipboard change to Pasta over session D-Bus (`Offer`); Pasta replies with pipe write ends for the formats it wants, reads them on a worker thread, publishes a snapshot and bumps a change counter. Linux picks one clipboard path at start-up (Wayland data-control, GNOME extension, or X11), and the existing `clipboard_change_count` / `read_clipboard_*` / `write_clipboard_*` functions route by it, so the watcher, storage and secret handling stay unchanged. Writes go to the extension's `SetClipboard`; a status watcher maps the extension's GNOME state to the launcher banner.

**Tech Stack:** Rust 2024, GPUI 0.2.2, `zbus` 5.14 (blocking API + `#[zbus::interface]`), `nix` 0.23 (`poll`), `std::io::pipe`; GJS ESM extension for GNOME Shell 50; POSIX `sh` + Python 3/PyGObject test harness around `dbus-run-session` and headless `gnome-shell`.

**Spec:** `docs/superpowers/specs/2026-10-02-gnome-clipboard-extension-design.md`

## Global Constraints

- Extension UUID is exactly `clipboard@pasta.launcher`; `metadata.json` declares `"shell-version": ["50"]` and an integer `"version"` starting at `1`.
- D-Bus contract (verbatim from the spec): Pasta owns `com.pasta.Launcher`, object `/com/pasta/Launcher/Clipboard`, interface `com.pasta.Launcher.Clipboard1`, method `Offer(as mimetypes) -> (a{sh} writers)`, served only to a caller whose `/proc/<pid>/exe` is `/usr/bin/gnome-shell` (any ` (deleted)` suffix stripped), others get `org.freedesktop.DBus.Error.AccessDenied`. The extension owns `com.pasta.Launcher.ShellBridge`, object `/com/pasta/Launcher/ShellBridge`, interface `com.pasta.Launcher.ShellBridge1`, method `SetClipboard(s mimetype, h data)`, served only to a caller whose executable basename (` (deleted)` stripped) is `pasta-launcher`.
- Payload cap is 32 MiB (`32 * 1024 * 1024` bytes) in both directions; each payload read on Pasta's side has a 5-second deadline.
- Wanted formats: the first `image/*`; `text/plain;charset=utf-8`, else `text/plain`; `text/uri-list` and `x-special/gnome-copied-files` when present.
- Logs never contain clipboard contents — only mimetype names, byte counts, executable paths and reasons. Pasta logs with `eprintln!`; the extension with `console.warn`/`console.log` prefixed `pasta-clipboard:`.
- The extension never blocks the compositor: no `*_sync` D-Bus calls and no blocking stream I/O; `GLib.file_read_link` on `/proc/<pid>/exe` is the only synchronous call allowed.
- Nothing in this plan loads the extension into the live GNOME session or calls `org.gnome.Shell.Extensions` on the live session bus. Extension behaviour is exercised only through the isolated harness (`gnome-extension/tests/nested-shell.sh`). Never use `pkill -f` (it matches the invoking shell); use pids.
- Unit tests live inline in `#[cfg(test)] mod tests` (repo convention). CI parity: `cargo fmt --all -- --check`, `cargo clippy --all-targets --no-deps` with no new warnings versus `main`, `RUSTFLAGS="-D warnings" cargo test`, `cargo build --release`. Everything Linux-only stays behind `#[cfg(target_os = "linux")]`; anything shared with macOS must compile warning-free there.
- Comments on each side of the D-Bus boundary justify that side's own choices; never describe the other side's internals.
- Markdown files: one line per paragraph, never hard-wrapped.
- Conventional Commits. Work on branch `feat/gnome-clipboard-extension` in an isolated worktree. Never push.

## Rulings this plan makes against the spec text (for the maintainer to confirm)

1. **Enable click runs on a background thread without suppressing blur auto-hide.** The spec says to suppress auto-hide during the call; `EnableExtension` opens no window and does not take focus, so there is nothing to suppress, and a blocking D-Bus call on the UI thread would freeze the launcher if the shell is slow. Cost if wrong: if some GNOME build shows a dialog on enable, the launcher could auto-hide behind it.
2. **When history is empty, the Enable button joins the existing empty-history notice instead of a separate banner.** The banner appears above a non-empty list. Showing both would print the same reason twice. Cost if wrong: purely visual.
3. **No code change to `main()`'s start-up clipboard read.** On the GNOME path the snapshot is empty at start-up, so `read_clipboard_file_image()` and `read_clipboard_snapshot()` return `None` and the read is already a no-op; the extension's start-up offer reaches the watcher through the counter. Cost if wrong: none observable.

---

## File Structure

| Path | Responsibility |
|---|---|
| `src/platform/linux/gnome_bridge_client.rs` | `std` + `zbus` only. Shared identity check (`SHELL_EXE`, `is_shell_executable`), `MAX_PAYLOAD_BYTES`, and the `SetClipboard` client `set_clipboard`. Compiled verbatim by `examples/gnome_bridge_write.rs`. |
| `src/platform/linux/gnome_bridge.rs` | Pasta's `Offer` service: format selection, deadline-bounded pipe reads, snapshot store + change counter, service start-up. |
| `src/platform/linux/gnome_extension_status.rs` | Classify the extension's GNOME state, map it to banner text/action, watch for state changes, call `EnableExtension`. |
| `src/platform/linux/mod.rs` | Clipboard path selection and routing; self-write echo window; GNOME notice store; `read_clipboard_image`; capture-fix action entry points. |
| `src/main.rs` | `CaptureFixAction` enum and `MenuCommand::ClipboardCaptureStatusChanged`. |
| `src/app/runtime.rs` | Watcher prefers `read_clipboard_image`; handle `ClipboardCaptureStatusChanged`. |
| `src/app/view.rs` | Capture banner above a non-empty list; Enable button in banner and empty notice. |
| `src/platform/macos/clipboard.rs`, `src/platform/macos/mod.rs` | No-op `clipboard_capture_fix_action` / `run_capture_fix_action`. |
| `examples/gnome_bridge_write.rs` | Test-only `SetClipboard` caller built from the real client file. |
| `gnome-extension/clipboard@pasta.launcher/*` | `metadata.json`, `extension.js`, `peer.js`, `fd.js`, `offer.js`, `pasta-watch.js`, `bridge.js`. |
| `gnome-extension/tests/*` | Isolated harness, `clip_tool.py`, scenarios, `run-all.sh`. |
| `Cargo.toml`, `packaging/linux/pasta.spec`, `scripts/install-linux-app.sh`, `scripts/uninstall-linux-app.sh` | Packaging of the extension; removal of the spike example entry. |
| `docs/linux-platform-notes.md`, `README.md`, `AGENTS.md`, `SMOKE_TEST_CHECKLIST.md` | Documentation. |
| Deleted: `spike/gnome-clipboard/`, `examples/gnome_bridge_spike.rs` | Spike code (its findings doc and plan stay). |

---

### Task 1: Self-write echoes stay suppressed for the whole window

**Files:**
- Modify: `src/platform/linux/mod.rs` (`should_ignore_self_clipboard_write`, tests module)

**Interfaces:**
- Consumes: `SelfClipboardWriteState { pending: Option<PendingSelfClipboardWrite { due_at: Instant, expected_hash: String }> }` (defined in `src/app/runtime.rs`), `clipboard_bytes_hash(&[u8]) -> String`.
- Produces: private `fn is_self_write_echo(due_at: Instant, expected_hash: &str, now: Instant, hash: &str) -> bool`. `should_ignore_self_clipboard_write` keeps its signature.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/platform/linux/mod.rs`:

```rust
    #[test]
    fn a_matching_echo_inside_the_window_is_ours() {
        let now = Instant::now();
        let due = now + std::time::Duration::from_secs(5);
        assert!(is_self_write_echo(due, "abc", now, "abc"));
    }

    #[test]
    fn every_echo_inside_the_window_is_ours_not_just_the_first() {
        let now = Instant::now();
        let due = now + std::time::Duration::from_secs(5);
        assert!(is_self_write_echo(due, "abc", now, "abc"));
        assert!(is_self_write_echo(
            due,
            "abc",
            now + std::time::Duration::from_secs(1),
            "abc"
        ));
    }

    #[test]
    fn different_content_is_not_an_echo() {
        let now = Instant::now();
        let due = now + std::time::Duration::from_secs(5);
        assert!(!is_self_write_echo(due, "abc", now, "def"));
    }

    #[test]
    fn an_echo_after_the_window_is_not_ours() {
        let now = Instant::now();
        assert!(!is_self_write_echo(
            now,
            "abc",
            now + std::time::Duration::from_millis(1),
            "abc"
        ));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --bin pasta-launcher echo`
Expected: compile error `cannot find function \`is_self_write_echo\` in this scope`.

- [ ] **Step 3: Implement**

Replace the body of `should_ignore_self_clipboard_write` in `src/platform/linux/mod.rs` and add the helper directly above it:

```rust
/// Whether clipboard content hashing to `hash` is an echo of Pasta's own
/// pending write. A match does not consume the pending entry: one write can
/// come back several times (each write path and each clipboard replay fires
/// its own change), and every one of those echoes is ours until it expires.
fn is_self_write_echo(due_at: Instant, expected_hash: &str, now: Instant, hash: &str) -> bool {
    now <= due_at && hash == expected_hash
}

/// Returns true if we should ignore this clipboard write because we
/// ourselves just wrote it.
pub(crate) fn should_ignore_self_clipboard_write(cx: &mut App, bytes: &[u8]) -> bool {
    let pending = cx
        .try_global::<SelfClipboardWriteState>()
        .and_then(|state| state.pending.clone());
    let Some(pending) = pending else { return false };

    let now = Instant::now();
    if now > pending.due_at {
        cx.global_mut::<SelfClipboardWriteState>().pending = None;
        return false;
    }

    is_self_write_echo(
        pending.due_at,
        &pending.expected_hash,
        now,
        &clipboard_bytes_hash(bytes),
    )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin pasta-launcher echo`
Expected: the four new tests pass.

- [ ] **Step 5: Full checks and commit**

Run: `cargo fmt --all -- --check && RUSTFLAGS="-D warnings" cargo test`
Expected: clean, all tests pass.

```bash
git add src/platform/linux/mod.rs
git commit -m "fix(linux): keep self-write echoes suppressed for the whole window"
```

---

### Task 2: Shared identity check, format selection and bounded payload reads

**Files:**
- Create: `src/platform/linux/gnome_bridge_client.rs`
- Create: `src/platform/linux/gnome_bridge.rs`
- Modify: `src/platform/linux/mod.rs` (module declarations)

**Interfaces:**
- Consumes: nothing.
- Produces (in `gnome_bridge_client.rs`): `pub(crate) const SHELL_EXE: &str = "/usr/bin/gnome-shell"`, `pub(crate) const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024`, `pub(crate) fn is_shell_executable(exe: &Path) -> bool`. (In `gnome_bridge.rs`): `pub(crate) const PAYLOAD_DEADLINE: Duration` (5 s), `pub(crate) fn select_wanted_mimes(offered: &[String]) -> Vec<String>`, `pub(crate) enum PayloadError { TooLarge, DeadlineExceeded, Io(String) }`, `pub(crate) fn read_payload(fd: OwnedFd, cap: usize, deadline: Duration) -> Result<Vec<u8>, PayloadError>`.

- [ ] **Step 1: Create the client file with the identity check and its tests (stubbed)**

`src/platform/linux/gnome_bridge_client.rs`:

```rust
//! Pasta's client for the GNOME Shell clipboard bridge, plus the identity check
//! the bridge service shares. Depends only on `std` and `zbus` so that
//! `examples/gnome_bridge_write.rs` can compile this exact file.

use std::path::Path;

/// The only executable Pasta accepts as the GNOME Shell end of the bridge.
pub(crate) const SHELL_EXE: &str = "/usr/bin/gnome-shell";

/// Largest clipboard payload Pasta sends or accepts over the bridge.
pub(crate) const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

/// Whether `exe` (a `/proc/<pid>/exe` target) is the installed gnome-shell.
pub(crate) fn is_shell_executable(_exe: &Path) -> bool {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn only_the_installed_gnome_shell_is_trusted() {
        assert!(is_shell_executable(Path::new("/usr/bin/gnome-shell")));
        assert!(!is_shell_executable(Path::new("/usr/bin/python3.14")));
        assert!(!is_shell_executable(Path::new("/tmp/gnome-shell")));
    }

    #[test]
    fn a_shell_replaced_by_an_upgrade_is_still_trusted() {
        // /proc/<pid>/exe gains this suffix when the binary on disk is
        // replaced while the process keeps running.
        assert!(is_shell_executable(&PathBuf::from(
            "/usr/bin/gnome-shell (deleted)"
        )));
    }
}
```

- [ ] **Step 2: Create `gnome_bridge.rs` with stubs and tests**

`src/platform/linux/gnome_bridge.rs`:

```rust
//! Pasta's side of the GNOME Shell clipboard bridge: answers `Offer` with pipe
//! write ends for the formats it wants, reads them, and publishes the result as
//! the current clipboard snapshot.

use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

/// How long one payload may take to arrive before Pasta gives up on it.
pub(crate) const PAYLOAD_DEADLINE: Duration = Duration::from_secs(5);

/// The offered formats worth reading: the first image, plain text (UTF-8
/// preferred), and file-manager file references. Every other format is kept
/// by name only (it still drives concealed/transient detection).
pub(crate) fn select_wanted_mimes(_offered: &[String]) -> Vec<String> {
    unimplemented!()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PayloadError {
    TooLarge,
    DeadlineExceeded,
    Io(String),
}

/// Reads `fd` to EOF, giving up when it exceeds `cap` bytes or does not reach
/// EOF within `deadline`.
pub(crate) fn read_payload(
    _fd: OwnedFd,
    _cap: usize,
    _deadline: Duration,
) -> Result<Vec<u8>, PayloadError> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn wants_first_image_utf8_text_and_file_references() {
        let offered = strings(&[
            "image/webp",
            "image/png",
            "text/plain",
            "text/plain;charset=utf-8",
            "text/uri-list",
            "x-special/gnome-copied-files",
            "x-kde-passwordManagerHint",
        ]);
        assert_eq!(
            select_wanted_mimes(&offered),
            strings(&[
                "image/webp",
                "text/plain;charset=utf-8",
                "text/uri-list",
                "x-special/gnome-copied-files",
            ])
        );
    }

    #[test]
    fn falls_back_to_bare_text_plain() {
        assert_eq!(
            select_wanted_mimes(&strings(&["text/plain"])),
            strings(&["text/plain"])
        );
    }

    #[test]
    fn wants_nothing_from_unknown_formats() {
        assert!(select_wanted_mimes(&strings(&["application/x-custom", "TARGETS"])).is_empty());
    }

    #[test]
    fn reads_a_payload_to_eof() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let feeder = std::thread::spawn(move || writer.write_all(b"hello"));
        let bytes = read_payload(OwnedFd::from(reader), 1024, Duration::from_secs(2));
        feeder.join().unwrap().unwrap();
        assert_eq!(bytes, Ok(b"hello".to_vec()));
    }

    #[test]
    fn rejects_a_payload_over_the_cap() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let feeder = std::thread::spawn(move || {
            let _ = writer.write_all(b"0123456789");
        });
        let result = read_payload(OwnedFd::from(reader), 8, Duration::from_secs(2));
        feeder.join().unwrap();
        assert_eq!(result, Err(PayloadError::TooLarge));
    }

    #[test]
    fn gives_up_on_a_writer_that_never_finishes() {
        // Stands in for a source app that stalls mid-transfer: the write end
        // stays open and nothing arrives.
        let (reader, writer) = std::io::pipe().unwrap();
        let started = Instant::now();
        let result = read_payload(OwnedFd::from(reader), 1024, Duration::from_millis(150));
        assert_eq!(result, Err(PayloadError::DeadlineExceeded));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(writer);
    }
}
```

- [ ] **Step 3: Declare the modules**

In `src/platform/linux/mod.rs`, next to `mod global_shortcuts;` and `mod polkit;`, add:

```rust
mod gnome_bridge;
mod gnome_bridge_client;
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --bin pasta-launcher gnome_bridge`
Expected: 8 tests run, all FAIL with `not implemented`.

- [ ] **Step 5: Implement the identity check**

In `gnome_bridge_client.rs` replace the stub:

```rust
/// Whether `exe` (a `/proc/<pid>/exe` target) is the installed gnome-shell.
/// The kernel appends " (deleted)" when the binary on disk was replaced while
/// the process kept running, which an upgrade does.
pub(crate) fn is_shell_executable(exe: &Path) -> bool {
    let path = exe.to_string_lossy();
    path.strip_suffix(" (deleted)").unwrap_or(&path) == SHELL_EXE
}
```

- [ ] **Step 6: Implement format selection and payload reads**

In `gnome_bridge.rs` replace both stubs:

```rust
pub(crate) fn select_wanted_mimes(offered: &[String]) -> Vec<String> {
    let mut wanted = Vec::new();
    if let Some(image) = offered.iter().find(|mime| mime.starts_with("image/")) {
        wanted.push(image.clone());
    }
    let text = offered
        .iter()
        .find(|mime| *mime == "text/plain;charset=utf-8")
        .or_else(|| offered.iter().find(|mime| *mime == "text/plain"));
    if let Some(text) = text {
        wanted.push(text.clone());
    }
    wanted.extend(
        offered
            .iter()
            .filter(|mime| *mime == "text/uri-list" || *mime == "x-special/gnome-copied-files")
            .cloned(),
    );
    wanted
}

pub(crate) fn read_payload(
    fd: OwnedFd,
    cap: usize,
    deadline: Duration,
) -> Result<Vec<u8>, PayloadError> {
    use nix::poll::{PollFd, PollFlags, poll};

    let mut file = std::fs::File::from(fd);
    let give_up_at = Instant::now() + deadline;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let remaining = give_up_at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PayloadError::DeadlineExceeded);
        }
        let mut fds = [PollFd::new(file.as_raw_fd(), PollFlags::POLLIN)];
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
        match poll(&mut fds, timeout_ms) {
            Ok(0) => return Err(PayloadError::DeadlineExceeded),
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(err) => return Err(PayloadError::Io(err.to_string())),
        }
        match file.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => {
                if bytes.len() + read > cap {
                    return Err(PayloadError::TooLarge);
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(PayloadError::Io(err.to_string())),
        }
    }
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --bin pasta-launcher gnome_bridge`
Expected: 8 passed. Unused-item warnings for items not yet called from outside tests are expected at this point (they are used by Task 3); `RUSTFLAGS="-D warnings"` is applied at the end of Task 3.

- [ ] **Step 8: Commit**

```bash
git add src/platform/linux/gnome_bridge.rs src/platform/linux/gnome_bridge_client.rs src/platform/linux/mod.rs
git commit -m "feat(linux): add GNOME bridge identity check, format selection and bounded reads"
```

---

### Task 3: `Offer` service, snapshot store, `SetClipboard` client and the write test example

**Files:**
- Modify: `src/platform/linux/gnome_bridge.rs`
- Modify: `src/platform/linux/gnome_bridge_client.rs`
- Create: `examples/gnome_bridge_write.rs`

**Interfaces:**
- Consumes: Task 2's `SHELL_EXE`, `MAX_PAYLOAD_BYTES`, `is_shell_executable`, `select_wanted_mimes`, `read_payload`, `PAYLOAD_DEADLINE`, `PayloadError`.
- Produces (in `gnome_bridge.rs`): `pub(crate) struct GnomeClipboardSnapshot { pub(crate) mimetypes: Vec<String>, pub(crate) payloads: HashMap<String, Vec<u8>> }` with `text(&self) -> Option<String>`, `image(&self) -> Option<(Vec<u8>, String)>`, `bytes(&self, mime: &str) -> Option<Vec<u8>>`; `pub(crate) struct SnapshotStore` with `snapshot(&self) -> Option<GnomeClipboardSnapshot>`, `change_count(&self) -> i64`; `pub(crate) static STORE: SnapshotStore`; `pub(crate) fn ensure_service() -> Result<(), String>`. Log line `info: GNOME clipboard snapshot published: mimetypes=[…] payloads=[<mime>=<bytes>, …]` (payload entries sorted) and `warning: refused GNOME clipboard Offer from <exe>`. (In `gnome_bridge_client.rs`): `pub(crate) fn set_clipboard(mimetype: &str, bytes: Vec<u8>) -> Result<(), String>`. Binary `gnome_bridge_write <mimetype> <file>` printing `WROTE <mimetype> <bytes>` or `refused: <reason>` (exit 1).

- [ ] **Step 1: Write the failing snapshot tests**

Append to the `tests` module in `gnome_bridge.rs`:

```rust
    fn snapshot(mimes: &[&str], payloads: &[(&str, &[u8])]) -> GnomeClipboardSnapshot {
        GnomeClipboardSnapshot {
            mimetypes: strings(mimes),
            payloads: payloads
                .iter()
                .map(|(mime, bytes)| ((*mime).to_owned(), bytes.to_vec()))
                .collect(),
        }
    }

    #[test]
    fn snapshot_text_prefers_utf8() {
        let snap = snapshot(
            &["text/plain", "text/plain;charset=utf-8"],
            &[("text/plain;charset=utf-8", "héllo".as_bytes())],
        );
        assert_eq!(snap.text().as_deref(), Some("héllo"));
    }

    #[test]
    fn snapshot_text_falls_back_to_bare_text_plain() {
        let snap = snapshot(&["text/plain"], &[("text/plain", b"plain")]);
        assert_eq!(snap.text().as_deref(), Some("plain"));
    }

    #[test]
    fn snapshot_image_is_the_first_offered_image_that_arrived() {
        let snap = snapshot(
            &["image/png", "text/plain"],
            &[("image/png", b"\x89PNG"), ("text/plain", b"caption")],
        );
        assert_eq!(snap.image(), Some((b"\x89PNG".to_vec(), "image/png".to_owned())));
    }

    #[test]
    fn snapshot_without_image_payload_has_no_image() {
        let snap = snapshot(&["image/png"], &[]);
        assert_eq!(snap.image(), None);
    }

    #[test]
    fn publishing_replaces_the_snapshot_and_bumps_the_counter() {
        // A local store, not the process-global one, so this test does not
        // depend on the order other tests run in.
        let store = SnapshotStore::new();
        assert_eq!(store.change_count(), 0);
        assert_eq!(store.snapshot(), None);
        store.publish(snapshot(&["text/plain"], &[("text/plain", b"a")]));
        store.publish(snapshot(&["text/plain"], &[("text/plain", b"b")]));
        assert_eq!(store.change_count(), 2);
        assert_eq!(store.snapshot().and_then(|s| s.text()).as_deref(), Some("b"));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --bin pasta-launcher gnome_bridge 2>&1 | grep -E "error\[E04(12|25|33)\]" | head -3`
Expected: compile errors for the missing `GnomeClipboardSnapshot` / `SnapshotStore`.

- [ ] **Step 3: Implement the snapshot store**

In `gnome_bridge.rs`, extend the imports:

```rust
use std::collections::HashMap;
use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
```

and add:

```rust
/// The most recent clipboard contents the shell offered, as far as Pasta read
/// them: every offered format name, plus the payloads it asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GnomeClipboardSnapshot {
    pub(crate) mimetypes: Vec<String>,
    pub(crate) payloads: HashMap<String, Vec<u8>>,
}

impl GnomeClipboardSnapshot {
    pub(crate) fn text(&self) -> Option<String> {
        ["text/plain;charset=utf-8", "text/plain"]
            .iter()
            .find_map(|mime| self.payloads.get(*mime))
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
    }

    pub(crate) fn image(&self) -> Option<(Vec<u8>, String)> {
        self.mimetypes
            .iter()
            .filter(|mime| mime.starts_with("image/"))
            .find_map(|mime| self.payloads.get(mime).map(|bytes| (bytes.clone(), mime.clone())))
    }

    pub(crate) fn bytes(&self, mime: &str) -> Option<Vec<u8>> {
        self.payloads.get(mime).cloned()
    }
}

/// Latest snapshot plus a counter the clipboard watcher compares, mirroring
/// the change counters of the other clipboard paths.
pub(crate) struct SnapshotStore {
    snapshot: Mutex<Option<GnomeClipboardSnapshot>>,
    change_count: AtomicI64,
}

impl SnapshotStore {
    pub(crate) const fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
            change_count: AtomicI64::new(0),
        }
    }

    pub(crate) fn snapshot(&self) -> Option<GnomeClipboardSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn change_count(&self) -> i64 {
        self.change_count.load(Ordering::Acquire)
    }

    fn publish(&self, snapshot: GnomeClipboardSnapshot) {
        *self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(snapshot);
        self.change_count.fetch_add(1, Ordering::AcqRel);
    }
}

pub(crate) static STORE: SnapshotStore = SnapshotStore::new();
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin pasta-launcher gnome_bridge`
Expected: 13 passed.

- [ ] **Step 5: Implement the `Offer` service**

Add to `gnome_bridge.rs`:

```rust
use super::gnome_bridge_client::{MAX_PAYLOAD_BYTES, SHELL_EXE, is_shell_executable};

const PASTA_NAME: &str = "com.pasta.Launcher";
const CLIPBOARD_PATH: &str = "/com/pasta/Launcher/Clipboard";

/// Reads every wanted payload of one offer on its own thread, then publishes
/// the snapshot once all of them have finished or been given up on.
fn collect_offer(mimetypes: Vec<String>, readers: Vec<(String, OwnedFd)>) {
    let spawned = std::thread::Builder::new()
        .name("pasta-gnome-offer".to_owned())
        .spawn(move || {
            let mut payloads = HashMap::new();
            for (mime, fd) in readers {
                match read_payload(fd, MAX_PAYLOAD_BYTES, PAYLOAD_DEADLINE) {
                    Ok(bytes) => {
                        payloads.insert(mime, bytes);
                    }
                    Err(err) => {
                        eprintln!("warning: GNOME clipboard payload {mime} dropped: {err:?}");
                    }
                }
            }
            let mut summary: Vec<String> = payloads
                .iter()
                .map(|(mime, bytes)| format!("{mime}={}", bytes.len()))
                .collect();
            summary.sort();
            eprintln!(
                "info: GNOME clipboard snapshot published: mimetypes={mimetypes:?} payloads=[{}]",
                summary.join(", ")
            );
            STORE.publish(GnomeClipboardSnapshot {
                mimetypes,
                payloads,
            });
        });
    if let Err(err) = spawned {
        eprintln!("warning: could not start GNOME clipboard reader thread: {err}");
    }
}

async fn executable_of(
    conn: &zbus::Connection,
    name: zbus::names::BusName<'_>,
) -> zbus::fdo::Result<std::path::PathBuf> {
    let pid = zbus::fdo::DBusProxy::new(conn)
        .await?
        .get_connection_unix_process_id(name)
        .await?;
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .map_err(|err| zbus::fdo::Error::Failed(format!("reading /proc/{pid}/exe: {err}")))
}

struct ClipboardService;

#[zbus::interface(name = "com.pasta.Launcher.Clipboard1")]
impl ClipboardService {
    /// Answers a clipboard change with one pipe write end per wanted format.
    async fn offer(
        &self,
        mimetypes: Vec<String>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::OwnedFd>> {
        let caller = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::AccessDenied("message has no sender".to_owned()))?
            .to_owned();
        let exe = executable_of(conn, caller.into()).await?;
        if !is_shell_executable(&exe) {
            eprintln!("warning: refused GNOME clipboard Offer from {}", exe.display());
            return Err(zbus::fdo::Error::AccessDenied(format!(
                "{} is not {SHELL_EXE}",
                exe.display()
            )));
        }

        let mut writers = HashMap::new();
        let mut readers = Vec::new();
        for mime in select_wanted_mimes(&mimetypes) {
            let (reader, writer) = std::io::pipe()
                .map_err(|err| zbus::fdo::Error::Failed(format!("pipe: {err}")))?;
            readers.push((mime.clone(), OwnedFd::from(reader)));
            writers.insert(mime, OwnedFd::from(writer).into());
        }
        collect_offer(mimetypes, readers);
        Ok(writers)
    }
}

/// Registers Pasta's clipboard service on the session bus, once. The
/// connection lives in a static so the service stays up for the process
/// lifetime; zbus runs it on its own executor thread.
pub(crate) fn ensure_service() -> Result<(), String> {
    static SERVICE: std::sync::OnceLock<Result<zbus::blocking::Connection, String>> =
        std::sync::OnceLock::new();
    SERVICE
        .get_or_init(|| {
            let built = zbus::blocking::connection::Builder::session()
                .and_then(|builder| builder.name(PASTA_NAME))
                .and_then(|builder| builder.serve_at(CLIPBOARD_PATH, ClipboardService))
                .and_then(|builder| builder.build())
                .map_err(|err| err.to_string());
            if let Err(err) = &built {
                eprintln!("warning: could not register {PASTA_NAME} on the session bus: {err}");
            }
            built
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
}
```

- [ ] **Step 6: Implement the `SetClipboard` client**

Append to `gnome_bridge_client.rs` (above its `tests` module):

```rust
const BRIDGE_NAME: &str = "com.pasta.Launcher.ShellBridge";
const BRIDGE_PATH: &str = "/com/pasta/Launcher/ShellBridge";
const BRIDGE_IFACE: &str = "com.pasta.Launcher.ShellBridge1";

/// Puts `bytes` on the clipboard as `mimetype` through the shell's bridge,
/// after checking that the bridge name is held by the installed gnome-shell.
/// Blocks until the shell has read the payload; call it off the UI thread.
pub(crate) fn set_clipboard(mimetype: &str, bytes: Vec<u8>) -> Result<(), String> {
    use std::io::Write;

    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "{} bytes exceeds the {MAX_PAYLOAD_BYTES}-byte limit",
            bytes.len()
        ));
    }
    let conn = zbus::blocking::Connection::session().map_err(|err| err.to_string())?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(&conn).map_err(|err| err.to_string())?;
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

    let (reader, mut writer) = std::io::pipe().map_err(|err| err.to_string())?;
    let feeder = std::thread::spawn(move || writer.write_all(&bytes));
    let reply = conn.call_method(
        Some(owner.as_str()),
        BRIDGE_PATH,
        Some(BRIDGE_IFACE),
        "SetClipboard",
        &(mimetype, zbus::zvariant::Fd::from(&reader)),
    );
    // Drop our read end before joining: if the shell refused without reading,
    // the feeder then gets EPIPE instead of blocking forever.
    drop(reader);
    let fed = feeder
        .join()
        .map_err(|_| "clipboard feeder thread panicked".to_owned())?;
    reply.map_err(|err| err.to_string())?;
    fed.map_err(|err| err.to_string())
}
```

- [ ] **Step 7: Create the write test example**

`examples/gnome_bridge_write.rs`:

```rust
//! Test-only: sends a file to the clipboard through the GNOME extension bridge
//! using Pasta's real client code. The isolated harness in gnome-extension/tests
//! runs it under the name `pasta-launcher`, the only caller the bridge serves.
//!
//!     cargo build --example gnome_bridge_write
//!     gnome_bridge_write <mimetype> <file>

#[cfg(target_os = "linux")]
#[path = "../src/platform/linux/gnome_bridge_client.rs"]
mod gnome_bridge_client;

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mimetype, file] = args.as_slice() else {
        eprintln!("usage: gnome_bridge_write <mimetype> <file>");
        std::process::exit(2);
    };
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("refused: reading {file}: {err}");
            std::process::exit(1);
        }
    };
    let len = bytes.len();
    match gnome_bridge_client::set_clipboard(mimetype, bytes) {
        Ok(()) => println!("WROTE {mimetype} {len}"),
        Err(err) => {
            eprintln!("refused: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
```

- [ ] **Step 8: Build, test, lint**

Items in `gnome_bridge.rs` and `set_clipboard` are not called from the app until Task 4, so mark them for now: add `#![allow(dead_code)]` as the first line of `gnome_bridge.rs` with the comment `// Wired into clipboard routing by the next commit; remove then.` and the same for `gnome_bridge_client.rs`. (Task 4 removes both.)

Run:

```bash
cargo build --example gnome_bridge_write
RUSTFLAGS="-D warnings" cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --no-deps 2>&1 | grep -cE "gnome_bridge(_client)?\.rs|gnome_bridge_write\.rs"
```

Expected: build succeeds; all tests pass; fmt clean; clippy count `0`.

- [ ] **Step 9: Commit**

```bash
git add src/platform/linux/gnome_bridge.rs src/platform/linux/gnome_bridge_client.rs examples/gnome_bridge_write.rs
git commit -m "feat(linux): add GNOME clipboard Offer service and SetClipboard client"
```

---

### Task 4: Choose the clipboard path once and route reads, writes and the watcher

**Files:**
- Modify: `src/platform/linux/mod.rs`
- Modify: `src/app/runtime.rs` (Linux `spawn_clipboard_watcher`)
- Modify: `src/platform/linux/gnome_bridge.rs`, `src/platform/linux/gnome_bridge_client.rs` (remove the temporary `#![allow(dead_code)]`)

**Interfaces:**
- Consumes: Task 3's `gnome_bridge::{STORE, ensure_service, GnomeClipboardSnapshot}`, `gnome_bridge_client::set_clipboard`.
- Produces: private `enum ClipboardPath { Wayland, GnomeExtension, X11 }`, `fn choose_clipboard_path(wayland: bool, data_control: bool, current_desktop: Option<&str>) -> ClipboardPath`, `fn clipboard_path() -> ClipboardPath`; `pub(crate) fn read_clipboard_image() -> Option<(Vec<u8>, String)>` (GNOME path only; `None` elsewhere).

- [ ] **Step 1: Write the failing path-selection tests**

Add to the tests module in `src/platform/linux/mod.rs`:

```rust
    #[test]
    fn x11_sessions_use_the_x11_path() {
        assert_eq!(
            choose_clipboard_path(false, false, Some("ubuntu:GNOME")),
            ClipboardPath::X11
        );
    }

    #[test]
    fn wayland_with_data_control_uses_it_even_on_gnome() {
        assert_eq!(
            choose_clipboard_path(true, true, Some("ubuntu:GNOME")),
            ClipboardPath::Wayland
        );
    }

    #[test]
    fn gnome_without_data_control_uses_the_extension() {
        assert_eq!(
            choose_clipboard_path(true, false, Some("ubuntu:GNOME")),
            ClipboardPath::GnomeExtension
        );
        assert_eq!(
            choose_clipboard_path(true, false, Some("GNOME")),
            ClipboardPath::GnomeExtension
        );
    }

    #[test]
    fn other_desktops_without_data_control_keep_the_wayland_path() {
        // The Wayland monitor then reports the missing protocol, as today.
        assert_eq!(
            choose_clipboard_path(true, false, Some("KDE")),
            ClipboardPath::Wayland
        );
        assert_eq!(choose_clipboard_path(true, false, None), ClipboardPath::Wayland);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --bin pasta-launcher clipboard_path 2>&1 | grep -E "cannot find" | head -2`
Expected: `cannot find function \`choose_clipboard_path\`` / `cannot find type \`ClipboardPath\``.

- [ ] **Step 3: Implement path selection**

Add to `src/platform/linux/mod.rs` near `is_wayland_session`:

```rust
/// Which mechanism Pasta uses to see and set the clipboard, decided once at
/// start-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardPath {
    /// Wayland data-control protocol (or its missing-protocol report).
    Wayland,
    /// GNOME Shell extension over D-Bus; GNOME implements no data-control.
    GnomeExtension,
    /// X11 selections via XFIXES and xclip/xsel.
    X11,
}

fn choose_clipboard_path(
    wayland: bool,
    data_control: bool,
    current_desktop: Option<&str>,
) -> ClipboardPath {
    if !wayland {
        return ClipboardPath::X11;
    }
    let gnome = current_desktop.is_some_and(|desktop| {
        desktop
            .split(':')
            .any(|part| part.eq_ignore_ascii_case("GNOME"))
    });
    if !data_control && gnome {
        ClipboardPath::GnomeExtension
    } else {
        ClipboardPath::Wayland
    }
}

fn clipboard_path() -> ClipboardPath {
    static PATH: OnceLock<ClipboardPath> = OnceLock::new();
    *PATH.get_or_init(|| {
        let wayland = is_wayland_session();
        choose_clipboard_path(
            wayland,
            wayland && wayland_data_control_available(),
            std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        )
    })
}

/// Whether the compositor advertises ext- or wlr-data-control.
fn wayland_data_control_available() -> bool {
    let Ok(conn) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _queue)) = registry_queue_init::<WaylandClipboardMonitorState>(&conn) else {
        return false;
    };
    globals.contents().with_list(|list| {
        list.iter().any(|global| {
            global.interface == ExtDataControlManagerV1::interface().name
                || global.interface == ZwlrDataControlManagerV1::interface().name
        })
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin pasta-launcher clipboard_path`
Expected: 4 passed.

- [ ] **Step 5: Route the clipboard functions**

In `src/platform/linux/mod.rs` make these replacements.

`clipboard_change_count`:

```rust
pub(crate) fn clipboard_change_count() -> i64 {
    match clipboard_path() {
        ClipboardPath::GnomeExtension => {
            if let Err(err) = gnome_bridge::ensure_service() {
                set_clipboard_capture_blocked(format!(
                    "Pasta could not register its clipboard service on D-Bus: {err}"
                ));
            }
            gnome_bridge::STORE.change_count()
        }
        ClipboardPath::Wayland => {
            ensure_wayland_clipboard_monitor();
            WAYLAND_CLIPBOARD_CHANGE_COUNT.load(Ordering::Acquire)
        }
        ClipboardPath::X11 => {
            ensure_x11_clipboard_monitor();
            if X11_CLIPBOARD_MONITOR_READY.load(Ordering::Acquire) {
                return X11_CLIPBOARD_CHANGE_COUNT.load(Ordering::Acquire);
            }
            polling_clipboard_change_count()
        }
    }
}
```

`read_clipboard_text` — add as the first statement:

```rust
    if clipboard_path() == ClipboardPath::GnomeExtension {
        return gnome_bridge::STORE.snapshot().and_then(|snapshot| snapshot.text());
    }
```

`read_clipboard_bytes` — add as the first statement:

```rust
    if clipboard_path() == ClipboardPath::GnomeExtension {
        return gnome_bridge::STORE
            .snapshot()
            .and_then(|snapshot| snapshot.bytes(mime_type));
    }
```

`read_clipboard_mime_types` — add as the first statement:

```rust
    if clipboard_path() == ClipboardPath::GnomeExtension {
        return gnome_bridge::STORE
            .snapshot()
            .map(|snapshot| snapshot.mimetypes)
            .unwrap_or_default();
    }
```

`write_clipboard_text` — add as the first statement:

```rust
    if clipboard_path() == ClipboardPath::GnomeExtension {
        write_through_gnome_bridge("text/plain;charset=utf-8", value.as_bytes().to_vec());
        return;
    }
```

`write_clipboard_image_bytes` — add as the first statement:

```rust
    if clipboard_path() == ClipboardPath::GnomeExtension {
        write_through_gnome_bridge(mime_type, bytes.to_vec());
        return;
    }
```

and add these two functions next to `read_clipboard_file_image`:

```rust
/// Image payload of the current clipboard, on paths where Pasta holds the
/// bytes itself. Other paths read images through GPUI and return `None` here.
pub(crate) fn read_clipboard_image() -> Option<(Vec<u8>, String)> {
    if clipboard_path() != ClipboardPath::GnomeExtension {
        return None;
    }
    gnome_bridge::STORE.snapshot().and_then(|snapshot| snapshot.image())
}

/// The bridge call waits for the shell to read the whole payload, so it runs
/// off the UI thread.
fn write_through_gnome_bridge(mimetype: &str, bytes: Vec<u8>) {
    let mimetype = mimetype.to_owned();
    let spawned = std::thread::Builder::new()
        .name("pasta-gnome-write".to_owned())
        .spawn(move || {
            if let Err(err) = gnome_bridge_client::set_clipboard(&mimetype, bytes) {
                eprintln!("warning: GNOME clipboard write of {mimetype} failed: {err}");
            }
        });
    if let Err(err) = spawned {
        eprintln!("warning: could not start GNOME clipboard writer thread: {err}");
    }
}
```

Remove the temporary `#![allow(dead_code)]` lines (and their comments) from `gnome_bridge.rs` and `gnome_bridge_client.rs`.

- [ ] **Step 6: Let the watcher take images from the platform first**

In `src/app/runtime.rs`, inside the Linux `spawn_clipboard_watcher`, replace the block from `let clipboard_image = cx` through the `let clipboard_image = match clipboard_image { … };` that falls back to `read_clipboard_file_image()` with:

```rust
                // Paths that hold the clipboard bytes themselves hand the image
                // over directly; the others read it through GPUI.
                let clipboard_image = match read_clipboard_image() {
                    Some(image) => Some(image),
                    None => cx
                        .update(|cx| {
                            cx.read_from_clipboard().and_then(|item| {
                                item.into_entries().find_map(|entry| match entry {
                                    ClipboardEntry::Image(image) => Some(image),
                                    _ => None,
                                })
                            })
                        })
                        .ok()
                        .flatten()
                        .map(|image| (image.bytes.clone(), image.format.mime_type().to_owned())),
                };

                // A file manager copying an image puts only a file *reference*
                // on the clipboard, so fall back to reading the referenced
                // file rather than storing its path as text. That fallback
                // shells out to `xclip`, so it runs on the background executor
                // — a slow or stalled selection owner must not block the
                // foreground executor that drives the UI, hotkey, and quit.
                let clipboard_image = match clipboard_image {
                    Some(image) => Some(image),
                    None => {
                        cx.background_executor()
                            .spawn(async { read_clipboard_file_image() })
                            .await
                    }
                };
```

The rest of the watcher (the `if let Some((bytes, mime_type)) = clipboard_image` branch and the text branch) is unchanged.

- [ ] **Step 7: Build, test, lint**

Run:

```bash
RUSTFLAGS="-D warnings" cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning'
git worktree add /tmp/pasta-main-clippy main
(cd /tmp/pasta-main-clippy && cargo clippy --all-targets --no-deps 2>&1 | grep -c '^warning')
git worktree remove --force /tmp/pasta-main-clippy
```

Expected: tests pass, fmt clean, and the two clippy warning counts are equal (this branch adds none). Later tasks refer to this as "the clippy comparison against `main`".

- [ ] **Step 8: Commit**

```bash
git add src/platform/linux src/app/runtime.rs
git commit -m "feat(linux): route clipboard through the GNOME extension when GNOME lacks data-control"
```

---

### Task 5: Extension status, the GNOME notice store and the Enable action

**Files:**
- Create: `src/platform/linux/gnome_extension_status.rs`
- Modify: `src/platform/linux/mod.rs` (module, notice store, `clipboard_capture_unavailable_reason`, `probe_clipboard_capture`, new action fns)
- Modify: `src/main.rs` (`CaptureFixAction`, `MenuCommand::ClipboardCaptureStatusChanged`)
- Modify: `src/app/runtime.rs` (`handle_menu_command`)
- Modify: `src/platform/macos/clipboard.rs`, `src/platform/macos/mod.rs`

**Interfaces:**
- Consumes: `clipboard_path()`, `ClipboardPath` (Task 4); `MENU_COMMAND_TX`, `LauncherState`.
- Produces: in `src/main.rs`: `pub(crate) enum CaptureFixAction { EnableGnomeExtension }` (Clone, Copy, Debug, PartialEq, Eq) and `MenuCommand::ClipboardCaptureStatusChanged`. In Linux and macOS platform modules: `pub(crate) fn clipboard_capture_fix_action() -> Option<CaptureFixAction>`, `pub(crate) fn run_capture_fix_action(action: CaptureFixAction)`. In `gnome_extension_status.rs`: `EXTENSION_UUID`, `LoadedExtension`, `ExtensionStatus`, `classify(...)`, `notice_for(&ExtensionStatus) -> Option<(String, Option<CaptureFixAction>)>`, `spawn_status_watcher(on_change)`, `enable_extension() -> Result<bool, String>`.

- [ ] **Step 1: Add the shared types in `src/main.rs`**

Next to `enum MenuCommand`, add:

```rust
/// A one-click fix the launcher can offer when clipboard capture is
/// unavailable. Only the Linux GNOME path produces one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum CaptureFixAction {
    EnableGnomeExtension,
}
```

and add a variant to `MenuCommand`:

```rust
    /// The reason clipboard capture is unavailable (or its fix) changed.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    ClipboardCaptureStatusChanged,
```

- [ ] **Step 2: Write the status module with stubbed logic and its tests**

`src/platform/linux/gnome_extension_status.rs`:

```rust
//! Whether Pasta's GNOME Shell extension is usable, and what the launcher
//! should tell the user when it is not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::OwnedValue;

use crate::CaptureFixAction;

pub(crate) const EXTENSION_UUID: &str = "clipboard@pasta.launcher";

const SHELL_BUS: &str = "org.gnome.Shell";
const SHELL_PATH: &str = "/org/gnome/Shell";
const EXTENSIONS_INTERFACE: &str = "org.gnome.Shell.Extensions";

// GNOME 50 ExtensionState values (misc/extensionUtils.js).
const STATE_INACTIVE: f64 = 2.0;
const STATE_ERROR: f64 = 3.0;
const STATE_OUT_OF_DATE: f64 = 4.0;
const STATE_INITIALIZED: f64 = 6.0;

/// What GNOME reports for a loaded extension.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LoadedExtension {
    pub(crate) state: f64,
    pub(crate) version: Option<f64>,
    pub(crate) path: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExtensionStatus {
    ExtensionsDisabled,
    NeedsLogout,
    NotInstalled,
    UpdatePending,
    Failed(String),
    OutOfDate,
    Disabled,
    Active,
}

pub(crate) fn classify(
    _user_extensions_enabled: bool,
    _loaded: Option<&LoadedExtension>,
    _installed_on_disk: bool,
    _on_disk_version: Option<f64>,
    _first_error: Option<&str>,
) -> ExtensionStatus {
    unimplemented!()
}

pub(crate) fn notice_for(_status: &ExtensionStatus) -> Option<(String, Option<CaptureFixAction>)> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(state: f64, version: Option<f64>) -> LoadedExtension {
        LoadedExtension {
            state,
            version,
            path: None,
        }
    }

    #[test]
    fn extensions_switched_off_wins_over_everything() {
        let active = loaded(1.0, Some(1.0));
        assert_eq!(
            classify(false, Some(&active), true, Some(1.0), None),
            ExtensionStatus::ExtensionsDisabled
        );
    }

    #[test]
    fn unknown_to_gnome_but_on_disk_needs_a_logout() {
        assert_eq!(
            classify(true, None, true, None, None),
            ExtensionStatus::NeedsLogout
        );
    }

    #[test]
    fn unknown_to_gnome_and_absent_is_not_installed() {
        assert_eq!(
            classify(true, None, false, None, None),
            ExtensionStatus::NotInstalled
        );
    }

    #[test]
    fn a_different_on_disk_version_is_a_pending_update() {
        let old = loaded(1.0, Some(1.0));
        assert_eq!(
            classify(true, Some(&old), true, Some(2.0), None),
            ExtensionStatus::UpdatePending
        );
    }

    #[test]
    fn error_state_carries_gnomes_first_error() {
        let broken = loaded(STATE_ERROR, Some(1.0));
        assert_eq!(
            classify(true, Some(&broken), true, Some(1.0), Some("SyntaxError: x")),
            ExtensionStatus::Failed("SyntaxError: x".to_owned())
        );
        assert_eq!(
            classify(true, Some(&broken), true, Some(1.0), None),
            ExtensionStatus::Failed("unknown error".to_owned())
        );
    }

    #[test]
    fn out_of_date_state_is_reported() {
        let stale = loaded(STATE_OUT_OF_DATE, Some(1.0));
        assert_eq!(
            classify(true, Some(&stale), true, Some(1.0), None),
            ExtensionStatus::OutOfDate
        );
    }

    #[test]
    fn inactive_and_initialized_are_disabled() {
        for state in [STATE_INACTIVE, STATE_INITIALIZED] {
            let off = loaded(state, Some(1.0));
            assert_eq!(
                classify(true, Some(&off), true, Some(1.0), None),
                ExtensionStatus::Disabled
            );
        }
    }

    #[test]
    fn active_and_transitional_states_are_active() {
        for state in [1.0, 5.0, 7.0, 8.0] {
            let on = loaded(state, Some(1.0));
            assert_eq!(
                classify(true, Some(&on), true, Some(1.0), None),
                ExtensionStatus::Active
            );
        }
    }

    #[test]
    fn only_a_disabled_extension_offers_the_enable_action() {
        assert_eq!(
            notice_for(&ExtensionStatus::Disabled),
            Some((
                "Pasta needs its GNOME extension to see your clipboard.".to_owned(),
                Some(CaptureFixAction::EnableGnomeExtension)
            ))
        );
        for status in [
            ExtensionStatus::ExtensionsDisabled,
            ExtensionStatus::NeedsLogout,
            ExtensionStatus::NotInstalled,
            ExtensionStatus::UpdatePending,
            ExtensionStatus::Failed("x".to_owned()),
            ExtensionStatus::OutOfDate,
        ] {
            let (_, action) = notice_for(&status).expect("a notice");
            assert_eq!(action, None, "{status:?}");
        }
    }

    #[test]
    fn notice_texts_match_the_spec() {
        let text = |status| notice_for(&status).map(|(text, _)| text);
        assert_eq!(
            text(ExtensionStatus::ExtensionsDisabled).as_deref(),
            Some("Extensions are turned off in GNOME. Turn them on in the Extensions app so Pasta can see your clipboard.")
        );
        assert_eq!(
            text(ExtensionStatus::NeedsLogout).as_deref(),
            Some("Log out and back in to finish installing Pasta's GNOME extension.")
        );
        assert_eq!(
            text(ExtensionStatus::NotInstalled).as_deref(),
            Some("Pasta's GNOME extension isn't installed. Reinstall Pasta to add it.")
        );
        assert_eq!(
            text(ExtensionStatus::UpdatePending).as_deref(),
            Some("Log out and back in to finish updating Pasta's GNOME extension.")
        );
        assert_eq!(
            text(ExtensionStatus::Failed("boom".to_owned())).as_deref(),
            Some("Pasta's GNOME extension failed to load: boom.")
        );
        assert_eq!(
            text(ExtensionStatus::OutOfDate).as_deref(),
            Some("This GNOME version isn't supported by Pasta's extension yet.")
        );
        assert_eq!(text(ExtensionStatus::Active), None);
    }
}
```

In `src/platform/linux/mod.rs` add `mod gnome_extension_status;` next to the other module declarations.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --bin pasta-launcher gnome_extension_status`
Expected: 10 tests run, all FAIL with `not implemented`.

- [ ] **Step 4: Implement classification and notices**

Replace both stubs:

```rust
pub(crate) fn classify(
    user_extensions_enabled: bool,
    loaded: Option<&LoadedExtension>,
    installed_on_disk: bool,
    on_disk_version: Option<f64>,
    first_error: Option<&str>,
) -> ExtensionStatus {
    if !user_extensions_enabled {
        return ExtensionStatus::ExtensionsDisabled;
    }
    let Some(loaded) = loaded else {
        return if installed_on_disk {
            ExtensionStatus::NeedsLogout
        } else {
            ExtensionStatus::NotInstalled
        };
    };
    if let (Some(running), Some(on_disk)) = (loaded.version, on_disk_version)
        && running != on_disk
    {
        return ExtensionStatus::UpdatePending;
    }
    if loaded.state == STATE_ERROR {
        return ExtensionStatus::Failed(first_error.unwrap_or("unknown error").to_owned());
    }
    if loaded.state == STATE_OUT_OF_DATE {
        return ExtensionStatus::OutOfDate;
    }
    if loaded.state == STATE_INACTIVE || loaded.state == STATE_INITIALIZED {
        return ExtensionStatus::Disabled;
    }
    ExtensionStatus::Active
}

pub(crate) fn notice_for(status: &ExtensionStatus) -> Option<(String, Option<CaptureFixAction>)> {
    let text = match status {
        ExtensionStatus::Active => return None,
        ExtensionStatus::Disabled => {
            return Some((
                "Pasta needs its GNOME extension to see your clipboard.".to_owned(),
                Some(CaptureFixAction::EnableGnomeExtension),
            ));
        }
        ExtensionStatus::ExtensionsDisabled => "Extensions are turned off in GNOME. Turn them on in the Extensions app so Pasta can see your clipboard.".to_owned(),
        ExtensionStatus::NeedsLogout => {
            "Log out and back in to finish installing Pasta's GNOME extension.".to_owned()
        }
        ExtensionStatus::NotInstalled => {
            "Pasta's GNOME extension isn't installed. Reinstall Pasta to add it.".to_owned()
        }
        ExtensionStatus::UpdatePending => {
            "Log out and back in to finish updating Pasta's GNOME extension.".to_owned()
        }
        ExtensionStatus::Failed(error) => {
            format!("Pasta's GNOME extension failed to load: {error}.")
        }
        ExtensionStatus::OutOfDate => {
            "This GNOME version isn't supported by Pasta's extension yet.".to_owned()
        }
    };
    Some((text, None))
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --bin pasta-launcher gnome_extension_status`
Expected: 10 passed.

- [ ] **Step 6: Implement the D-Bus side: query, watcher, enable**

Append to `gnome_extension_status.rs` (above `tests`):

```rust
fn extension_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/gnome-shell/extensions").join(EXTENSION_UUID)];
    if let Some(data) = dirs::data_dir() {
        dirs.push(data.join("gnome-shell/extensions").join(EXTENSION_UUID));
    }
    dirs
}

fn read_metadata_version(extension_dir: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(extension_dir.join("metadata.json")).ok()?;
    let metadata: serde_json::Value = serde_json::from_str(&text).ok()?;
    metadata.get("version")?.as_f64()
}

fn parse_loaded(info: &HashMap<String, OwnedValue>) -> Option<LoadedExtension> {
    let state = info.get("state")?.downcast_ref::<f64>().ok()?;
    Some(LoadedExtension {
        state,
        version: info.get("version").and_then(|value| value.downcast_ref::<f64>().ok()),
        path: info
            .get("path")
            .and_then(|value| value.downcast_ref::<&str>().ok())
            .map(str::to_owned),
    })
}

fn shell_extensions(conn: &Connection) -> Result<Proxy<'static>, String> {
    Proxy::new(conn, SHELL_BUS, SHELL_PATH, EXTENSIONS_INTERFACE).map_err(|err| err.to_string())
}

fn query_status(conn: &Connection) -> Result<ExtensionStatus, String> {
    let shell = shell_extensions(conn)?;
    let enabled: bool = shell
        .get_property("UserExtensionsEnabled")
        .map_err(|err| err.to_string())?;
    let info: HashMap<String, OwnedValue> = shell
        .call("GetExtensionInfo", &(EXTENSION_UUID,))
        .map_err(|err| err.to_string())?;
    let loaded = parse_loaded(&info);
    let errors: Vec<String> = shell
        .call("GetExtensionErrors", &(EXTENSION_UUID,))
        .unwrap_or_default();
    let on_disk_version = loaded
        .as_ref()
        .and_then(|loaded| loaded.path.as_deref())
        .and_then(|path| read_metadata_version(Path::new(path)));
    let installed_on_disk = extension_dirs().iter().any(|dir| dir.is_dir());
    Ok(classify(
        enabled,
        loaded.as_ref(),
        installed_on_disk,
        on_disk_version,
        errors.first().map(String::as_str),
    ))
}

/// Reports the current notice, then re-checks whenever GNOME Shell signals
/// anything on its object (extension state changes, the global extensions
/// switch), reporting only when the notice changes. Runs for the process
/// lifetime on its own thread.
pub(crate) fn spawn_status_watcher(
    on_change: impl Fn(Option<(String, Option<CaptureFixAction>)>) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("pasta-gnome-extension-status".to_owned())
        .spawn(move || {
            let conn = match Connection::session() {
                Ok(conn) => conn,
                Err(err) => {
                    on_change(Some((
                        format!("Pasta could not check its GNOME extension: {err}"),
                        None,
                    )));
                    return;
                }
            };
            let messages = MatchRule::builder()
                .msg_type(MessageType::Signal)
                .sender(SHELL_BUS)
                .and_then(|builder| builder.path(SHELL_PATH))
                .map(|builder| builder.build())
                .and_then(|rule| MessageIterator::for_match_rule(rule, &conn, Some(16)));
            let mut messages = match messages {
                Ok(messages) => Some(messages),
                Err(err) => {
                    eprintln!("warning: cannot follow GNOME extension state changes: {err}");
                    None
                }
            };

            let mut last: Option<Option<(String, Option<CaptureFixAction>)>> = None;
            loop {
                let notice = match query_status(&conn) {
                    Ok(status) => notice_for(&status),
                    Err(err) => Some((
                        format!("Pasta could not check its GNOME extension: {err}"),
                        None,
                    )),
                };
                if last.as_ref() != Some(&notice) {
                    on_change(notice.clone());
                    last = Some(notice);
                }
                match messages.as_mut().and_then(|messages| messages.next()) {
                    Some(_) => continue,
                    None => return,
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("warning: could not start GNOME extension status thread: {err}");
    }
}

/// Asks GNOME Shell to enable the extension. Only ever called from a user's
/// click on the launcher's Enable button.
pub(crate) fn enable_extension() -> Result<bool, String> {
    let conn = Connection::session().map_err(|err| err.to_string())?;
    shell_extensions(&conn)?
        .call("EnableExtension", &(EXTENSION_UUID,))
        .map_err(|err| err.to_string())
}
```

- [ ] **Step 7: Wire the notice store and actions in `src/platform/linux/mod.rs`**

Add `CaptureFixAction` to the `use crate::{ … }` list. Add next to `clipboard_capture_block`:

```rust
/// The GNOME extension's current notice. Unlike the first-reason-wins block
/// above, it is replaced as the extension's state changes and cleared once
/// the extension is active.
static GNOME_CAPTURE_NOTICE: Mutex<Option<(String, Option<CaptureFixAction>)>> = Mutex::new(None);

fn set_gnome_capture_notice(notice: Option<(String, Option<CaptureFixAction>)>) {
    *GNOME_CAPTURE_NOTICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = notice;
    if let Some(tx) = MENU_COMMAND_TX.get() {
        let _ = tx.send(MenuCommand::ClipboardCaptureStatusChanged);
    }
}

/// The one-click fix for the current notice, if it has one.
pub(crate) fn clipboard_capture_fix_action() -> Option<CaptureFixAction> {
    GNOME_CAPTURE_NOTICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(|(_, action)| *action)
}

/// Runs a fix the user clicked. Off the UI thread: it is a D-Bus round trip
/// to the shell, and the resulting state change arrives through the status
/// watcher.
pub(crate) fn run_capture_fix_action(action: CaptureFixAction) {
    match action {
        CaptureFixAction::EnableGnomeExtension => {
            let spawned = std::thread::Builder::new()
                .name("pasta-gnome-enable".to_owned())
                .spawn(|| match gnome_extension_status::enable_extension() {
                    Ok(true) => {}
                    Ok(false) => eprintln!("warning: GNOME Shell declined to enable the extension"),
                    Err(err) => eprintln!("warning: enabling the GNOME extension failed: {err}"),
                });
            if let Err(err) = spawned {
                eprintln!("warning: could not start GNOME enable thread: {err}");
            }
        }
    }
}
```

Replace `clipboard_capture_unavailable_reason`:

```rust
pub(crate) fn clipboard_capture_unavailable_reason() -> Option<String> {
    clipboard_capture_block()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .or_else(|| {
            GNOME_CAPTURE_NOTICE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .map(|(text, _)| text.clone())
        })
}
```

Replace `probe_clipboard_capture`:

```rust
pub(crate) fn probe_clipboard_capture() {
    match clipboard_path() {
        ClipboardPath::Wayland => {}
        ClipboardPath::GnomeExtension => {
            gnome_extension_status::spawn_status_watcher(set_gnome_capture_notice);
        }
        ClipboardPath::X11 => {
            if let Some(reason) =
                x11_capture_block_reason(command_exists("xclip"), command_exists("xsel"))
            {
                eprintln!("warning: {reason}");
                set_clipboard_capture_blocked(reason);
            }
        }
    }
}
```

- [ ] **Step 8: Refresh the launcher on status changes**

In `src/app/runtime.rs` `handle_menu_command`, add an arm:

```rust
        MenuCommand::ClipboardCaptureStatusChanged => {
            if let Some(window) = cx
                .try_global::<LauncherState>()
                .and_then(|state| state.window)
            {
                let _ = window.update(cx, |_view, _window, cx| cx.notify());
            }
        }
```

- [ ] **Step 9: macOS no-ops**

In `src/platform/macos/clipboard.rs`, after `probe_clipboard_capture`, add:

```rust
/// No-op counterpart to the Linux capture-fix actions.
pub(crate) fn clipboard_capture_fix_action() -> Option<crate::CaptureFixAction> {
    None
}

/// No-op counterpart to the Linux capture-fix actions.
pub(crate) fn run_capture_fix_action(_action: crate::CaptureFixAction) {}
```

and add `clipboard_capture_fix_action` and `run_capture_fix_action` to the `pub(crate) use clipboard::{ … }` list in `src/platform/macos/mod.rs`.

- [ ] **Step 10: Build, test, lint**

Run: `RUSTFLAGS="-D warnings" cargo test && cargo fmt --all -- --check`, then the clippy comparison against `main` from Task 4 Step 7. Expected: all pass; counts equal. (`clipboard_capture_fix_action`/`run_capture_fix_action` are consumed in Task 6; if `-D warnings` reports them unused on Linux, add `#[allow(dead_code)] // used by the launcher banner (next commit)` to both and remove it in Task 6.)

- [ ] **Step 11: Commit**

```bash
git add src/main.rs src/app/runtime.rs src/platform
git commit -m "feat(linux): report GNOME extension status and offer an Enable action"
```

---

### Task 6: Capture banner and Enable button in the launcher

**Files:**
- Modify: `src/app/view.rs`

**Interfaces:**
- Consumes: `clipboard_capture_unavailable_reason() -> Option<String>`, `clipboard_capture_fix_action() -> Option<CaptureFixAction>`, `run_capture_fix_action(CaptureFixAction)`, `Palette` (Copy; fields `muted_text`, `keycap_bg`, `keycap_text`).
- Produces: methods `render_capture_notice(&self, reason: &str, palette: Palette, cx: &mut Context<Self>) -> AnyElement` and `render_capture_fix_button(action: CaptureFixAction, palette: Palette, cx: &mut Context<Self>) -> AnyElement` on `LauncherView`.

There is no UI test harness in this repo; the behaviour behind the banner (which text, whether there is a button) is unit-tested in Task 5. This task is verified by build/lint and the manual checks in Step 4.

- [ ] **Step 1: Add the two render helpers**

In `src/app/view.rs`, inside `impl LauncherView` (next to `render_preview_pane`), add:

```rust
    /// One-line notice above a non-empty history explaining why nothing new is
    /// being captured, with its fix when there is one.
    fn render_capture_notice(
        &self,
        reason: &str,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut notice = div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px(px(10.0))
            .py(px(6.0))
            .text_xs()
            .text_color(palette.muted_text)
            .child(div().flex_1().child(reason.to_owned()));
        if let Some(action) = clipboard_capture_fix_action() {
            notice = notice.child(Self::render_capture_fix_button(action, palette, cx));
        }
        notice.into_any_element()
    }

    fn render_capture_fix_button(
        action: CaptureFixAction,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = match action {
            CaptureFixAction::EnableGnomeExtension => "Enable",
        };
        div()
            .id("capture-fix-action")
            .text_xs()
            .text_color(palette.keycap_text)
            .bg(palette.keycap_bg)
            .rounded(px(4.0))
            .px(px(8.0))
            .py(px(2.0))
            .cursor_pointer()
            .on_click(cx.listener(move |_view, _event, _window, cx| {
                run_capture_fix_action(action);
                cx.notify();
            }))
            .child(label)
            .into_any_element()
    }
```

- [ ] **Step 2: Add the button to the empty-history notice**

In the `let results = if self.items.is_empty() {` branch, replace:

```rust
            if let Some(reason) = capture_blocked {
                empty_state =
                    empty_state.child(div().max_w(px(420.0)).text_xs().text_center().child(reason));
            }
```

with:

```rust
            if let Some(reason) = capture_blocked {
                empty_state =
                    empty_state.child(div().max_w(px(420.0)).text_xs().text_center().child(reason));
                if let Some(action) = clipboard_capture_fix_action() {
                    empty_state = empty_state
                        .child(Self::render_capture_fix_button(action, palette, cx));
                }
            }
```

- [ ] **Step 3: Show the banner above a non-empty list**

Immediately after the statement that ends the `let results = if self.items.is_empty() { … };` chain (the line `};` just before `let mut panel = div()`), insert:

```rust
        // History exists but capture is down: without this the list looks
        // normal and the user never learns why nothing new appears in it.
        let results = match (
            self.items.is_empty(),
            clipboard_capture_unavailable_reason(),
        ) {
            (false, Some(reason)) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.render_capture_notice(&reason, palette, cx))
                .child(div().flex_1().min_h(px(0.0)).child(results))
                .into_any_element(),
            _ => results,
        };
```

Remove any temporary `#[allow(dead_code)]` added in Task 5 Step 10.

- [ ] **Step 4: Build, lint, and check by hand**

Run: `RUSTFLAGS="-D warnings" cargo test && cargo fmt --all -- --check`, plus the clippy comparison against `main`. Expected: all pass; counts equal.

Visual check (not in the live session's clipboard path — this only renders the launcher): run `cargo run` with `XDG_CURRENT_DESKTOP=GNOME` on this GNOME 50 machine while the extension is not installed, open the launcher, and confirm the banner reads "Pasta's GNOME extension isn't installed. Reinstall Pasta to add it." above existing history, with no button. Close Pasta afterwards (tray → Quit or by pid).

- [ ] **Step 5: Commit**

```bash
git add src/app/view.rs src/platform
git commit -m "feat(ui): show clipboard-capture banner with Enable button"
```

---

### Task 7: The GNOME Shell extension

**Files:**
- Create: `gnome-extension/clipboard@pasta.launcher/metadata.json`
- Create: `gnome-extension/clipboard@pasta.launcher/peer.js`
- Create: `gnome-extension/clipboard@pasta.launcher/fd.js`
- Create: `gnome-extension/clipboard@pasta.launcher/offer.js`
- Create: `gnome-extension/clipboard@pasta.launcher/pasta-watch.js`
- Create: `gnome-extension/clipboard@pasta.launcher/bridge.js`
- Create: `gnome-extension/clipboard@pasta.launcher/extension.js`

**Interfaces:**
- Consumes: the D-Bus contract in Global Constraints.
- Produces: an extension that pushes `Offer` on every clipboard change with formats, offers once when `com.pasta.Launcher` appears, and serves `SetClipboard`. Warnings logged with prefix `pasta-clipboard:`, including `pasta-clipboard: not offering: com.pasta.Launcher is owned by <exe>` and `pasta-clipboard: rejected SetClipboard from <exe>`.

The extension has no standalone unit-test runner; Task 8's harness exercises it end to end. This task's check is that GNOME loads it cleanly in the isolated shell (Step 8).

- [ ] **Step 1: `metadata.json`**

```json
{
  "uuid": "clipboard@pasta.launcher",
  "name": "Pasta clipboard",
  "description": "Lets the Pasta clipboard manager see and set the clipboard on GNOME.",
  "shell-version": ["50"],
  "version": 1
}
```

- [ ] **Step 2: `peer.js`**

```js
// Identifies the process behind a D-Bus name by its executable path, without
// blocking the compositor: every bus call here is asynchronous.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

function callBus(method, args, replyType) {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call(
            'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
            method, args, new GLib.VariantType(replyType),
            Gio.DBusCallFlags.NONE, 2000, null,
            (connection, result) => {
                try {
                    resolve(connection.call_finish(result).deepUnpack()[0]);
                } catch (e) {
                    reject(e);
                }
            });
    });
}

/** Absolute executable path of the process that owns `busName` (unique or well-known). */
export async function executableOf(busName) {
    const pid = await callBus('GetConnectionUnixProcessID', new GLib.Variant('(s)', [busName]), '(u)');
    return GLib.file_read_link(`/proc/${pid}/exe`);
}

/** Current unique owner of a well-known name and that owner's executable. */
export async function ownerOf(wellKnownName) {
    const unique = await callBus('GetNameOwner', new GLib.Variant('(s)', [wellKnownName]), '(s)');
    return {unique, exe: await executableOf(unique)};
}

/** Basename of an executable path; a binary replaced on disk while running reads back with " (deleted)". */
export function exeBasename(path) {
    return GLib.path_get_basename(path.replace(/ \(deleted\)$/, ''));
}
```

- [ ] **Step 3: `fd.js`**

```js
// File descriptors received over D-Bus belong to this process once stolen
// from their list; anything not handed to a stream must be closed here.
import GLib from 'gi://GLib';

export function closeQuietly(fd) {
    try {
        GLib.close(fd);
    } catch (e) {
        console.warn(`pasta-clipboard: closing fd ${fd} failed: ${e.message}`);
    }
}

/** Whether `index` names an entry of `fds` that has not been used yet. */
export function isUsableIndex(index, fds, used) {
    return Number.isInteger(index) && index >= 0 && index < fds.length && !used.has(index);
}
```

- [ ] **Step 4: `offer.js`**

```js
// Pushes a clipboard change to Pasta and streams the payloads it asks for into
// the pipe write ends its reply carries.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {closeQuietly, isUsableIndex} from './fd.js';
import {exeBasename, ownerOf} from './peer.js';

const PASTA_NAME = 'com.pasta.Launcher';
const PASTA_PATH = '/com/pasta/Launcher/Clipboard';
const PASTA_IFACE = 'com.pasta.Launcher.Clipboard1';
const PASTA_EXE = 'pasta-launcher';

function transferInto(selection, mimetype, fd) {
    return new Promise(resolve => {
        const stream = GioUnix.OutputStream.new(fd, true);
        selection.transfer_async(Meta.SelectionType.SELECTION_CLIPBOARD, mimetype, -1, stream, null,
            (source, result) => {
                try {
                    source.transfer_finish(result);
                } catch (e) {
                    console.warn(`pasta-clipboard: transfer of ${mimetype} failed: ${e.message}`);
                }
                try {
                    // Closing is what tells the reader the payload is complete.
                    stream.close(null);
                } catch (e) {
                    console.warn(`pasta-clipboard: closing ${mimetype} stream failed: ${e.message}`);
                }
                resolve();
            });
    });
}

function callOffer(destination, mimetypes) {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call_with_unix_fd_list(
            destination, PASTA_PATH, PASTA_IFACE, 'Offer',
            new GLib.Variant('(as)', [mimetypes]), new GLib.VariantType('(a{sh})'),
            Gio.DBusCallFlags.NONE, 2000, null, null,
            (connection, result) => {
                try {
                    resolve(connection.call_with_unix_fd_list_finish(result));
                } catch (e) {
                    reject(e);
                }
            });
    });
}

/** Offers the current clipboard (listing `mimetypes`) to Pasta, if Pasta is running and genuine. */
export async function offerToPasta(selection, mimetypes) {
    let owner;
    try {
        owner = await ownerOf(PASTA_NAME);
    } catch {
        return; // Pasta is not running; nothing to offer to.
    }
    if (exeBasename(owner.exe) !== PASTA_EXE) {
        console.warn(`pasta-clipboard: not offering: ${PASTA_NAME} is owned by ${owner.exe}`);
        return;
    }

    // Address the verified unique name, not the well-known one, so an owner
    // change between the check and the call cannot redirect the payload.
    const [reply, fdList] = await callOffer(owner.unique, mimetypes);
    // steal_fds() hands ownership to us; with get() the list would keep a
    // duplicate of every write end open and the reader would never see EOF.
    const fds = fdList ? fdList.steal_fds() : [];
    const wanted = reply.deepUnpack()[0];
    const used = new Set();
    const transfers = [];
    for (const [mimetype, index] of Object.entries(wanted)) {
        if (!isUsableIndex(index, fds, used)) {
            console.warn(`pasta-clipboard: ignoring bad handle ${index} for ${mimetype}`);
            continue;
        }
        used.add(index);
        transfers.push(transferInto(selection, mimetype, fds[index]));
    }
    fds.forEach((fd, index) => {
        if (!used.has(index))
            closeQuietly(fd);
    });
    await Promise.all(transfers);
}
```

- [ ] **Step 5: `pasta-watch.js`**

```js
// Offers the clipboard as it stands whenever Pasta appears on the bus, so a
// copy made before Pasta started is not lost.
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';

import {offerToPasta} from './offer.js';

export class PastaWatch {
    constructor(selection) {
        // The appeared callback also fires right away when the name is
        // already owned, which covers enabling while Pasta is running.
        this._watchId = Gio.bus_watch_name_on_connection(
            Gio.DBus.session, 'com.pasta.Launcher', Gio.BusNameWatcherFlags.NONE,
            () => this._offerCurrent(selection), null);
    }

    _offerCurrent(selection) {
        const mimetypes = selection.get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD);
        if (mimetypes.length === 0)
            return;
        offerToPasta(selection, mimetypes)
            .catch(e => console.warn(`pasta-clipboard: start-up offer failed: ${e.message}`));
    }

    destroy() {
        Gio.bus_unwatch_name(this._watchId);
    }
}
```

- [ ] **Step 6: `bridge.js`**

```js
// Accepts clipboard writes from Pasta over D-Bus and applies them with St.Clipboard.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {closeQuietly, isUsableIndex} from './fd.js';
import {executableOf, exeBasename} from './peer.js';

const BRIDGE_NAME = 'com.pasta.Launcher.ShellBridge';
const BRIDGE_PATH = '/com/pasta/Launcher/ShellBridge';
const ALLOWED_CALLER_EXE = 'pasta-launcher';
const MAX_BYTES = 32 * 1024 * 1024;
const BRIDGE_XML = `<node>
  <interface name="com.pasta.Launcher.ShellBridge1">
    <method name="SetClipboard">
      <arg type="s" name="mimetype" direction="in"/>
      <arg type="h" name="data" direction="in"/>
    </method>
  </interface>
</node>`;

function joinChunks(chunks, total) {
    const out = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
        out.set(chunk, offset);
        offset += chunk.length;
    }
    return new GLib.Bytes(out);
}

/** Reads `fd` to EOF, refusing payloads over MAX_BYTES. */
function readAll(fd) {
    const input = GioUnix.InputStream.new(fd, true);
    const chunks = [];
    let total = 0;
    return new Promise((resolve, reject) => {
        const next = () => input.read_bytes_async(64 * 1024, GLib.PRIORITY_DEFAULT, null, (stream, result) => {
            let bytes;
            try {
                bytes = stream.read_bytes_finish(result);
            } catch (e) {
                input.close(null);
                reject(e);
                return;
            }
            const size = bytes.get_size();
            if (size === 0) {
                input.close(null);
                resolve(joinChunks(chunks, total));
                return;
            }
            total += size;
            if (total > MAX_BYTES) {
                input.close(null);
                reject(new Error(`payload exceeds ${MAX_BYTES} bytes`));
                return;
            }
            chunks.push(bytes.toArray());
            next();
        });
        next();
    });
}

export class ShellBridge {
    constructor() {
        this._exported = Gio.DBusExportedObject.wrapJSObject(BRIDGE_XML, this);
        this._exported.export(Gio.DBus.session, BRIDGE_PATH);
        this._nameId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BRIDGE_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    async SetClipboardAsync([mimetype, handle], invocation, fdList) {
        // Take ownership of every incoming fd first so none outlives this call.
        const fds = fdList ? fdList.steal_fds() : [];
        const closeAllExcept = keep => fds.forEach((fd, index) => {
            if (index !== keep)
                closeQuietly(fd);
        });

        let exe;
        try {
            exe = await executableOf(invocation.get_sender());
        } catch (e) {
            closeAllExcept(-1);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `could not identify caller: ${e.message}`);
            return;
        }
        if (exeBasename(exe) !== ALLOWED_CALLER_EXE) {
            closeAllExcept(-1);
            console.warn(`pasta-clipboard: rejected SetClipboard from ${exe}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `${exe} is not ${ALLOWED_CALLER_EXE}`);
            return;
        }
        if (!isUsableIndex(handle, fds, new Set())) {
            closeAllExcept(-1);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.InvalidArgs',
                `no file descriptor at index ${handle}`);
            return;
        }
        closeAllExcept(handle);

        let bytes;
        try {
            bytes = await readAll(fds[handle]);
        } catch (e) {
            console.warn(`pasta-clipboard: SetClipboard ${mimetype} read failed: ${e.message}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', e.message);
            return;
        }
        St.Clipboard.get_default().set_content(St.ClipboardType.CLIPBOARD, mimetype, bytes);
        invocation.return_value(null);
    }

    destroy() {
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
    }
}
```

- [ ] **Step 7: `extension.js`**

```js
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {ShellBridge} from './bridge.js';
import {offerToPasta} from './offer.js';
import {PastaWatch} from './pasta-watch.js';

export default class PastaClipboardExtension extends Extension {
    enable() {
        this._selection = global.display.get_selection();
        // The bridge comes first: if it throws, nothing else has been
        // connected yet for disable() to miss.
        this._bridge = new ShellBridge();
        this._ownerChangedId = this._selection.connect('owner-changed', (selection, type) => {
            if (type !== Meta.SelectionType.SELECTION_CLIPBOARD)
                return;
            const mimetypes = selection.get_mimetypes(type);
            // A change with no formats is the clipboard being cleared between
            // owners; there is nothing to offer.
            if (mimetypes.length === 0)
                return;
            offerToPasta(selection, mimetypes)
                .catch(e => console.warn(`pasta-clipboard: offer failed: ${e.message}`));
        });
        this._pastaWatch = new PastaWatch(this._selection);
    }

    disable() {
        this._pastaWatch?.destroy();
        this._pastaWatch = null;
        if (this._ownerChangedId) {
            this._selection.disconnect(this._ownerChangedId);
            this._ownerChangedId = 0;
        }
        this._bridge?.destroy();
        this._bridge = null;
        this._selection = null;
    }
}
```

- [ ] **Step 8: Check that GNOME loads it, in an isolated shell**

This uses the spike's harness (still present until Task 8) with a scenario that only checks loading. Create `/tmp/pasta-load-check.sh` (outside the repo):

```sh
#!/bin/sh
set -eu
info=$(busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions GetExtensionInfo s clipboard@pasta.launcher)
echo "$info" | tr ' ' '\n' | grep -A2 '"state"' | tr '\n' ' '; echo
errors=$(busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions GetExtensionErrors s clipboard@pasta.launcher)
echo "errors: $errors"
[ "$errors" = "as 0" ] || exit 1
grep -a "pasta-clipboard\|JS ERROR" "$SPIKE_LOG" || true
echo "PASS load-check"
```

Run it with the spike harness pointed at the real extension:

The spike harness installs whatever extension directory sits next to it under the name in its `UUID=` line, so point a temporary copy at the real extension:

```bash
chmod +x /tmp/pasta-load-check.sh
rm -rf /tmp/pasta-load && mkdir -p /tmp/pasta-load
cp spike/gnome-clipboard/nested-shell.sh spike/gnome-clipboard/nested-inner.sh /tmp/pasta-load/
cp -r gnome-extension/clipboard@pasta.launcher /tmp/pasta-load/
sed -i 's/^UUID=.*/UUID=clipboard@pasta.launcher/' /tmp/pasta-load/nested-shell.sh
/tmp/pasta-load/nested-shell.sh /tmp/pasta-load-check.sh
rm -rf /tmp/pasta-load /tmp/pasta-load-check.sh
```

Expected: `"state" d 1` (active), `errors: as 0`, no `JS ERROR` lines, `PASS load-check`. Confirm `pgrep -x gnome-shell` prints the same pid before and after.

- [ ] **Step 9: Commit**

```bash
git add gnome-extension/clipboard@pasta.launcher
git commit -m "feat(gnome): add clipboard@pasta.launcher GNOME Shell extension"
```

---

### Task 8: Promote the isolated harness, test end to end, delete the spike

**Files:**
- Create: `gnome-extension/tests/nested-shell.sh`, `nested-inner.sh`, `lib.sh`, `clip_tool.py`, `run-all.sh`
- Create: `gnome-extension/tests/scenario-capture.sh`, `scenario-startup.sh`, `scenario-write.sh`, `scenario-refusals.sh`
- Delete: `spike/gnome-clipboard/` (whole directory), `examples/gnome_bridge_spike.rs`
- Modify: `Cargo.toml` (remove the `[[example]] gnome_bridge_spike` table and its comment)

**Interfaces:**
- Consumes: Task 3's `gnome_bridge_write` example and Pasta log lines (`info: GNOME clipboard snapshot published: … payloads=[<mime>=<bytes>, …]`, `warning: refused GNOME clipboard Offer from <exe>`); Task 7's extension and its warnings.
- Produces: `gnome-extension/tests/run-all.sh` — builds `pasta-launcher` and `gnome_bridge_write`, runs every scenario in a fresh isolated shell, exits non-zero on any failure, and refuses to start while a live `pasta-launcher` is running.

- [ ] **Step 1: Harness entry point**

`gnome-extension/tests/nested-shell.sh`:

```sh
#!/bin/sh
# Runs one scenario inside a throwaway, headless GNOME Shell with the real
# clipboard@pasta.launcher extension installed.
#
# Isolation: a private D-Bus session (dbus-run-session), a private Wayland
# display, XDG config/data/cache/state under $NEST, and the in-memory
# GSettings backend. The runtime directory is shared with the desktop
# (gnome-shell will not start without it), which is why a live Pasta must not
# be running: its single-instance lock would capture the test launch.
#
# Usage: nested-shell.sh <scenario-script> [args...]
set -eu
if pgrep -x pasta-launcher >/dev/null 2>&1; then
    echo "FAIL a pasta-launcher is running; quit it before running the GNOME extension tests"
    exit 1
fi
HERE=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$HERE/../.." && pwd)
UUID=clipboard@pasta.launcher
NEST=${NEST:-$(mktemp -d "${TMPDIR:-/tmp}/pasta-gnome-test.XXXXXX")}
mkdir -p "$NEST/config" "$NEST/cache" "$NEST/state" "$NEST/writer" "$NEST/data/gnome-shell/extensions"
# GNOME Shell discovers extensions only at start-up.
rm -rf "$NEST/data/gnome-shell/extensions/$UUID"
cp -r "$REPO_ROOT/gnome-extension/$UUID" "$NEST/data/gnome-shell/extensions/"
export XDG_CONFIG_HOME="$NEST/config" XDG_DATA_HOME="$NEST/data" \
    XDG_CACHE_HOME="$NEST/cache" XDG_STATE_HOME="$NEST/state" \
    GSETTINGS_BACKEND=memory NEST HERE REPO_ROOT UUID
echo "nest: $NEST"
exec dbus-run-session -- "$HERE/nested-inner.sh" "$@"
```

`gnome-extension/tests/nested-inner.sh`:

```sh
#!/bin/sh
# Runs inside the private D-Bus session created by nested-shell.sh.
set -eu
DISPLAY_NAME=pasta-gnome-test-$$
export SHELL_LOG="$NEST/shell.log"
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800 \
    --wayland-display "$DISPLAY_NAME" >"$SHELL_LOG" 2>&1 &
SHELL_PID=$!
trap 'kill "$SHELL_PID" 2>/dev/null; wait "$SHELL_PID" 2>/dev/null || true' EXIT

ready=no
for _ in $(seq 1 80); do
    if busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions ListExtensions >/dev/null 2>&1; then
        ready=yes
        break
    fi
    sleep 0.25
done
[ "$ready" = yes ] || { echo "FAIL nested shell never exposed org.gnome.Shell.Extensions"; exit 1; }

enabled=$(busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s "$UUID")
[ "$enabled" = "b true" ] || { echo "FAIL EnableExtension returned: $enabled"; exit 1; }

export WAYLAND_DISPLAY="$DISPLAY_NAME" GDK_BACKEND=wayland XDG_CURRENT_DESKTOP=GNOME
"$@"
```

- [ ] **Step 2: Shared helpers**

`gnome-extension/tests/lib.sh`:

```sh
# Shared helpers for scenario scripts. Sourced, not executed.

# wait_for_line <file> <ERE> <timeout-seconds>
wait_for_line() {
    deadline=$(( $(date +%s) + $3 ))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if [ -f "$1" ] && grep -aqE -- "$2" "$1"; then
            return 0
        fi
        sleep 0.2
    done
    echo "FAIL waited $3s in $1 for: $2"
    return 1
}

# refute_line <file> <ERE>
refute_line() {
    if [ -f "$1" ] && grep -aqE -- "$2" "$1"; then
        echo "FAIL unexpected line in $1: $(grep -aE -- "$2" "$1" | head -1)"
        return 1
    fi
    return 0
}

# start_pasta: run the real pasta-launcher inside the nested session; its
# stderr goes to $NEST/pasta.log.
start_pasta() {
    "$REPO_ROOT/target/debug/pasta-launcher" 2>>"$NEST/pasta.log" &
    PASTA_PID=$!
    trap 'kill "$PASTA_PID" 2>/dev/null || true' EXIT
    wait_for_line "$NEST/pasta.log" "Linux launcher window created" 30
}

# write_as_pasta <mimetype> <file>: send through the bridge using Pasta's real
# client, under the executable name the bridge serves.
write_as_pasta() {
    cp "$REPO_ROOT/target/debug/examples/gnome_bridge_write" "$NEST/writer/pasta-launcher"
    "$NEST/writer/pasta-launcher" "$1" "$2"
}
```

- [ ] **Step 3: Test clients**

`gnome-extension/tests/clip_tool.py`:

```python
"""Test clients for the GNOME extension harness. Only ever run inside nested-shell.sh."""
import os
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk  # noqa: E402


def hold_clipboard(fill, hold_seconds=3.0):
    """Open a window (so the app has a seat serial), call fill(clipboard), keep serving, quit."""

    def activate(app):
        window = Gtk.ApplicationWindow(application=app)
        window.present()
        clipboard = window.get_display().get_clipboard()
        GLib.timeout_add(500, lambda: (fill(clipboard), False)[1])
        GLib.timeout_add(int(hold_seconds * 1000), lambda: (app.quit(), False)[1])

    app = Gtk.Application(application_id="dev.pasta.GnomeTestClip", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def provider(mimetype, data):
    return Gdk.ContentProvider.new_for_bytes(mimetype, GLib.Bytes.new(data))


def set_text(text):
    hold_clipboard(lambda clipboard: clipboard.set(text))


def set_file(mimetype, path):
    data = open(path, "rb").read()
    hold_clipboard(lambda clipboard: clipboard.set_content(provider(mimetype, data)))


def set_file_reference(path):
    """Copy a file the way Nautilus does: a reference, not the bytes."""
    uri = "file://" + os.path.abspath(path)
    hold_clipboard(lambda clipboard: clipboard.set_content(Gdk.ContentProvider.new_union([
        provider("x-special/gnome-copied-files", ("copy\n" + uri).encode()),
        provider("text/uri-list", (uri + "\r\n").encode()),
        provider("text/plain;charset=utf-8", uri.encode()),
    ])))


def make_png(path, width, height):
    """Write a PNG with some noise rows so its size is non-trivial."""
    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf

    width, height = int(width), int(height)
    rowstride = width * 3
    pixels = bytearray(rowstride * height)
    for y in range(height):
        row = os.urandom(rowstride) if y % 5 == 0 else bytes([y % 256, 90, 200]) * width
        pixels[y * rowstride:(y + 1) * rowstride] = row
    GdkPixbuf.Pixbuf.new_from_bytes(
        GLib.Bytes.new(bytes(pixels)), GdkPixbuf.Colorspace.RGB, False, 8, width, height, rowstride
    ).savev(path, "png", [], [])


IMPOSTORS = {
    "pasta": (
        "com.pasta.Launcher",
        "/com/pasta/Launcher/Clipboard",
        '<node><interface name="com.pasta.Launcher.Clipboard1">'
        '<method name="Offer"><arg type="as" direction="in"/><arg type="a{sh}" direction="out"/></method>'
        "</interface></node>",
    ),
    "bridge": (
        "com.pasta.Launcher.ShellBridge",
        "/com/pasta/Launcher/ShellBridge",
        '<node><interface name="com.pasta.Launcher.ShellBridge1">'
        '<method name="SetClipboard"><arg type="s" direction="in"/><arg type="h" direction="in"/></method>'
        "</interface></node>",
    ),
}


def impostor(kind, seconds):
    """Own a name the real peers trust, answer every call, and report it."""
    name, path, xml = IMPOSTORS[kind]
    interface = Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0]
    loop = GLib.MainLoop()

    def on_call(_conn, _sender, _path, _iface, method, _params, invocation):
        print(f"IMPOSTOR got {method}", flush=True)
        if method == "Offer":
            invocation.return_value(GLib.Variant("(a{sh})", ({},)))
        else:
            invocation.return_value(None)

    def on_bus(connection, _name):
        connection.register_object(path, interface, on_call, None, None)

    Gio.bus_own_name(
        Gio.BusType.SESSION, name, Gio.BusNameOwnerFlags.NONE,
        on_bus, lambda *_: print(f"IMPOSTOR owns {name}", flush=True), None)
    GLib.timeout_add_seconds(int(seconds), loop.quit)
    loop.run()


def poke_bridge():
    """Call SetClipboard as a process that is not pasta-launcher."""
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    read_end, write_end = os.pipe()
    os.write(write_end, b"poke")
    os.close(write_end)
    fds = Gio.UnixFDList.new_from_array([read_end])
    try:
        connection.call_with_unix_fd_list_sync(
            "com.pasta.Launcher.ShellBridge", "/com/pasta/Launcher/ShellBridge",
            "com.pasta.Launcher.ShellBridge1", "SetClipboard",
            GLib.Variant("(sh)", ("text/plain;charset=utf-8", 0)), None,
            Gio.DBusCallFlags.NONE, 5000, fds, None)
        print("POKE accepted", flush=True)
    except GLib.Error as error:
        print(f"POKE rejected: {error.message}", flush=True)


COMMANDS = {
    "set-text": set_text,
    "set-file": set_file,
    "set-file-reference": set_file_reference,
    "make-png": make_png,
    "impostor": impostor,
    "poke-bridge": poke_bridge,
}

if __name__ == "__main__":
    command, *args = sys.argv[1:]
    COMMANDS[command](*args)
```

- [ ] **Step 4: Scenarios**

`gnome-extension/tests/scenario-capture.sh`:

```sh
#!/bin/sh
# Text, an image and a file-manager reference reach Pasta's snapshot.
set -eu
. "$HERE/lib.sh"
start_pasta

python3 "$HERE/clip_tool.py" set-text "harness text"
wait_for_line "$NEST/pasta.log" 'snapshot published: .*payloads=\[.*text/plain;charset=utf-8=12' 15

python3 "$HERE/clip_tool.py" make-png "$NEST/image.png" 1440 900
size=$(stat -c %s "$NEST/image.png")
python3 "$HERE/clip_tool.py" set-file image/png "$NEST/image.png"
wait_for_line "$NEST/pasta.log" "snapshot published: .*payloads=\[image/png=$size" 15

python3 "$HERE/clip_tool.py" set-file-reference "$NEST/image.png"
wait_for_line "$NEST/pasta.log" 'snapshot published: .*x-special/gnome-copied-files=' 15

refute_line "$NEST/pasta.log" "payload .* dropped|refused GNOME clipboard Offer"
refute_line "$SHELL_LOG" "pasta-clipboard: (offer failed|transfer of)|JS ERROR"
echo "PASS scenario-capture"
```

`gnome-extension/tests/scenario-startup.sh`:

```sh
#!/bin/sh
# Something copied before Pasta starts is offered once Pasta appears.
set -eu
. "$HERE/lib.sh"
python3 "$HERE/clip_tool.py" set-text "before pasta"
start_pasta
wait_for_line "$NEST/pasta.log" 'snapshot published: .*payloads=\[.*text/plain;charset=utf-8=12' 15
echo "PASS scenario-startup"
```

`gnome-extension/tests/scenario-write.sh`:

```sh
#!/bin/sh
# Pasta's real client writes text and an image through the bridge; the shell
# then owns the clipboard and offers it straight back.
set -eu
. "$HERE/lib.sh"
start_pasta

printf 'written by pasta' >"$NEST/text.txt"
python3 "$HERE/clip_tool.py" make-png "$NEST/write.png" 800 600
for case in "text/plain;charset=utf-8 text.txt" "image/png write.png"; do
    set -- $case
    size=$(stat -c %s "$NEST/$2")
    out=$(write_as_pasta "$1" "$NEST/$2")
    [ "$out" = "WROTE $1 $size" ] || { echo "FAIL write said: $out"; exit 1; }
    wait_for_line "$NEST/pasta.log" "snapshot published: .*$1=$size" 15
done
echo "PASS scenario-write"
```

`gnome-extension/tests/scenario-refusals.sh`:

```sh
#!/bin/sh
# Each side refuses peers that are not who they claim to be.
set -eu
. "$HERE/lib.sh"

# 1. The extension offers nothing to an impostor holding Pasta's name.
python3 "$HERE/clip_tool.py" impostor pasta 10 >"$NEST/impostor-pasta.log" 2>&1 &
IMPOSTOR_PID=$!
wait_for_line "$NEST/impostor-pasta.log" "IMPOSTOR owns com.pasta.Launcher" 10
python3 "$HERE/clip_tool.py" set-text "for pasta only"
wait_for_line "$SHELL_LOG" "pasta-clipboard: not offering: com.pasta.Launcher is owned by /usr/bin/python3" 10
kill "$IMPOSTOR_PID" 2>/dev/null || true
wait "$IMPOSTOR_PID" 2>/dev/null || true
refute_line "$NEST/impostor-pasta.log" "IMPOSTOR got"

# 2. Pasta refuses an Offer from anything but /usr/bin/gnome-shell.
start_pasta
if busctl --user call com.pasta.Launcher /com/pasta/Launcher/Clipboard \
        com.pasta.Launcher.Clipboard1 Offer as 1 text/plain >/dev/null 2>&1; then
    echo "FAIL busctl Offer was accepted"
    exit 1
fi
wait_for_line "$NEST/pasta.log" "refused GNOME clipboard Offer from /usr/bin/busctl" 5

# 3. The bridge refuses a caller that is not pasta-launcher.
poke=$(python3 "$HERE/clip_tool.py" poke-bridge)
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected: $poke"; exit 1 ;;
esac
wait_for_line "$SHELL_LOG" "pasta-clipboard: rejected SetClipboard from /usr/bin/python3" 5

# 4. Pasta's client refuses to write to an impostor bridge.
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
python3 "$HERE/clip_tool.py" impostor bridge 10 >"$NEST/impostor-bridge.log" 2>&1 &
wait_for_line "$NEST/impostor-bridge.log" "IMPOSTOR owns com.pasta.Launcher.ShellBridge" 10
printf 'secret' >"$NEST/secret.txt"
if write_as_pasta "text/plain;charset=utf-8" "$NEST/secret.txt" 2>"$NEST/writer.err"; then
    echo "FAIL write to an impostor bridge succeeded"
    exit 1
fi
grep -q "is owned by /usr/bin/python3" "$NEST/writer.err" || { echo "FAIL unexpected writer error: $(cat "$NEST/writer.err")"; exit 1; }
sleep 1
refute_line "$NEST/impostor-bridge.log" "IMPOSTOR got"
echo "PASS scenario-refusals"
```

`gnome-extension/tests/run-all.sh`:

```sh
#!/bin/sh
# Builds Pasta and the write test client, then runs every scenario, each in
# its own fresh isolated GNOME Shell. Local only: needs GNOME Shell 50.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
cargo build --manifest-path "$HERE/../../Cargo.toml" --bin pasta-launcher --example gnome_bridge_write || exit 1
failed=0
for scenario in capture startup write refusals; do
    if "$HERE/nested-shell.sh" "$HERE/scenario-$scenario.sh"; then
        :
    else
        echo "FAILED scenario-$scenario"
        failed=1
    fi
done
exit "$failed"
```

- [ ] **Step 5: Run everything**

Run:

```bash
chmod +x gnome-extension/tests/*.sh
pgrep -x gnome-shell
gnome-extension/tests/run-all.sh 2>&1 | grep -E '^(PASS|FAIL|FAILED)'
pgrep -x gnome-shell
```

Expected: `PASS scenario-capture`, `PASS scenario-startup`, `PASS scenario-write`, `PASS scenario-refusals`, no `FAIL`; the live `gnome-shell` pid is the same before and after. If a scenario fails, read `$NEST/pasta.log` and `$NEST/shell.log` (the nest path is printed at the start of each scenario) and fix the cause in the code under test, not by loosening the assertion.

- [ ] **Step 6: Delete the spike**

```bash
git rm -r spike/gnome-clipboard examples/gnome_bridge_spike.rs
```

In `Cargo.toml`, delete these lines:

```toml
# Throwaway spike (see docs/gnome-extension-spike-findings.md). `test = true`
# so `cargo test` runs its unit tests.
[[example]]
name = "gnome_bridge_spike"
test = true
```

Run: `RUSTFLAGS="-D warnings" cargo test && cargo fmt --all -- --check`
Expected: passes; the spike's 8 example tests are gone from the output.

- [ ] **Step 7: Commit**

```bash
git add gnome-extension/tests Cargo.toml
git commit -m "test(gnome): add isolated end-to-end harness for the GNOME extension and drop the spike"
```

---

### Task 9: Package and install the extension

**Files:**
- Modify: `Cargo.toml` (`[package.metadata.deb] assets`)
- Modify: `packaging/linux/pasta.spec` (`%install`, `%files`)
- Modify: `scripts/install-linux-app.sh`
- Modify: `scripts/uninstall-linux-app.sh`

**Interfaces:**
- Consumes: the seven files in `gnome-extension/clipboard@pasta.launcher/`.
- Produces: system installs at `/usr/share/gnome-shell/extensions/clipboard@pasta.launcher/`; user installs at `~/.local/share/gnome-shell/extensions/clipboard@pasta.launcher/`. No script enables, disables or changes any GNOME setting.

- [ ] **Step 1: `.deb` assets**

In `Cargo.toml`, append to the `assets = [ … ]` array of `[package.metadata.deb]`:

```toml
    ["gnome-extension/clipboard@pasta.launcher/metadata.json", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
    ["gnome-extension/clipboard@pasta.launcher/extension.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
    ["gnome-extension/clipboard@pasta.launcher/peer.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
    ["gnome-extension/clipboard@pasta.launcher/fd.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
    ["gnome-extension/clipboard@pasta.launcher/offer.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
    ["gnome-extension/clipboard@pasta.launcher/pasta-watch.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
    ["gnome-extension/clipboard@pasta.launcher/bridge.js", "usr/share/gnome-shell/extensions/clipboard@pasta.launcher/", "644"],
```

- [ ] **Step 2: `.rpm` spec**

In `packaging/linux/pasta.spec`, at the end of `%install` (after the polkit `install` line, before `desktop-file-validate`), add:

```spec
for f in metadata.json extension.js peer.js fd.js offer.js pasta-watch.js bridge.js; do
    install -Dm0644 gnome-extension/clipboard@pasta.launcher/$f \
        %{buildroot}%{_datadir}/gnome-shell/extensions/clipboard@pasta.launcher/$f
done
```

and in `%files`, after the polkit line, add:

```spec
%{_datadir}/gnome-shell/extensions/clipboard@pasta.launcher/
```

- [ ] **Step 3: Install script**

In `scripts/install-linux-app.sh`, add with the other path variables:

```bash
GNOME_EXT_UUID="clipboard@pasta.launcher"
GNOME_EXT_SRC="${ROOT_DIR}/gnome-extension/${GNOME_EXT_UUID}"
GNOME_EXT_DIR="${HOME}/.local/share/gnome-shell/extensions/${GNOME_EXT_UUID}"
```

and insert this block immediately before the final `echo ""` / `echo "Done. Launch Pasta …"` lines:

```bash
# ---------------------------------------------------------------------------
# GNOME Shell extension — how Pasta sees the clipboard on GNOME, whose
# compositor offers no clipboard protocol to background apps. Installed only;
# enabling it is the user's choice (Pasta's launcher offers an Enable button).
# ---------------------------------------------------------------------------
if [[ -d "${GNOME_EXT_SRC}" ]]; then
  rm -rf "${GNOME_EXT_DIR}"
  mkdir -p "${GNOME_EXT_DIR}"
  install -m 0644 "${GNOME_EXT_SRC}"/* "${GNOME_EXT_DIR}/"
  echo "Installed GNOME extension: ${GNOME_EXT_DIR}"
  if [[ "${XDG_CURRENT_DESKTOP:-}" == *GNOME* ]]; then
    echo ""
    echo "GNOME: log out and back in, then click Enable in Pasta's launcher"
    echo "(or turn on \"Pasta clipboard\" in GNOME's Extensions app)."
  fi
else
  echo "warning: ${GNOME_EXT_SRC} not found; Pasta cannot capture the clipboard on GNOME" >&2
fi
```

- [ ] **Step 4: Uninstall script**

In `scripts/uninstall-linux-app.sh`, after `remove_quiet "${POLKIT_USER_DIR}/${BUNDLE_ID}.policy"`, add:

```bash
GNOME_EXT_DIR="${HOME}/.local/share/gnome-shell/extensions/clipboard@pasta.launcher"
if [[ -d "${GNOME_EXT_DIR}" ]]; then
  rm -rf "${GNOME_EXT_DIR}"
  echo "Removed: ${GNOME_EXT_DIR}"
fi
```

- [ ] **Step 5: Verify without touching this machine's install**

The install script builds a release, installs into `$HOME` and may ask for `sudo`; do not run it here. Verify instead:

```bash
bash -n scripts/install-linux-app.sh && bash -n scripts/uninstall-linux-app.sh && echo "syntax ok"
cargo metadata --format-version 1 --no-deps >/dev/null && echo "Cargo.toml ok"
for f in metadata.json extension.js peer.js fd.js offer.js pasta-watch.js bridge.js; do
  grep -q "clipboard@pasta.launcher/$f\"" Cargo.toml || echo "deb asset missing: $f"
  test -f "gnome-extension/clipboard@pasta.launcher/$f" || echo "source missing: $f"
done
ls gnome-extension/clipboard@pasta.launcher | wc -l
```

Expected: `syntax ok`, `Cargo.toml ok`, no `missing` lines, `7`. If `cargo deb` is installed, also run `cargo build --release && cargo deb --no-build && dpkg -c target/debian/*.deb | grep gnome-shell/extensions` and expect the seven files.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml packaging/linux/pasta.spec scripts/install-linux-app.sh scripts/uninstall-linux-app.sh
git commit -m "build(linux): package and install the GNOME clipboard extension"
```

---

### Task 10: Documentation

**Files:**
- Modify: `docs/linux-platform-notes.md`
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `SMOKE_TEST_CHECKLIST.md`

**Interfaces:**
- Consumes: everything above. One line per paragraph; no hard wraps.

- [ ] **Step 1: `docs/linux-platform-notes.md`**

Add a section after "Global hotkey: desktop portal first, evdev fallback":

```markdown
## Clipboard on GNOME: the clipboard@pasta.launcher extension

GNOME 50 has no X11 session and its compositor implements neither `ext-data-control-v1` nor `wlr-data-control-v1`, deliberately: background apps cannot watch the clipboard. Code inside GNOME Shell can, so Pasta ships a small extension (`gnome-extension/clipboard@pasta.launcher/`). At start-up Linux picks one clipboard path (`clipboard_path()` in `src/platform/linux/mod.rs`): Wayland data-control when the compositor has it, the GNOME extension when it does not and `XDG_CURRENT_DESKTOP` contains `GNOME`, X11 without `WAYLAND_DISPLAY`.

The extension pushes every clipboard change to Pasta (`com.pasta.Launcher` → `com.pasta.Launcher.Clipboard1.Offer(as) -> a{sh}`); Pasta answers with pipe write ends for the formats it wants, reads them (32 MiB cap, 5 s deadline per payload) and publishes a snapshot plus a change counter that the normal clipboard watcher consumes (`src/platform/linux/gnome_bridge.rs`). Writes go the other way through `com.pasta.Launcher.ShellBridge1.SetClipboard(s, h)` (`gnome_bridge_client.rs`). Pasta serves only `/usr/bin/gnome-shell`; the extension serves only an executable named `pasta-launcher` — a basename check, so any program named that, run as the user, passes it.

Things worth knowing before touching it:

- **GNOME discovers extensions only when the shell starts.** After installing or upgrading, the user must log out and back in once before it can be enabled; until then `GetExtensionInfo` returns an empty dict and `EnableExtension` returns `false`. `gnome_extension_status.rs` turns this and every other state into the launcher banner.
- **Every copy produces several clipboard events**: the real one, an empty one (dropped by the extension), and a replay from GNOME's memory when the source app exits. Replays are forwarded on purpose; storage dedup moves the item to the top instead of duplicating it.
- **Pasta's own writes come back** as clipboard changes; the self-write check in `should_ignore_self_clipboard_write` stays active for its whole 5-second window for this reason.
- **Never load the extension into your live session to test it** — an exception in GNOME Shell can end the whole Wayland session. Use `gnome-extension/tests/run-all.sh`, which runs a headless GNOME Shell with its own D-Bus session, XDG directories and in-memory settings. Quit any running Pasta first: the harness shares `XDG_RUNTIME_DIR` (gnome-shell will not start without it), so a live instance's single-instance lock would capture the test launch.
- The measurements behind this design are in `docs/gnome-extension-spike-findings.md`.
```

- [ ] **Step 2: `README.md`**

Replace the line:

```markdown
Wayland-first (tested on KDE Plasma and GNOME; requires a compositor that implements `ext-data-control-v1` or `wlr-data-control-v1`). X11 falls back through GPUI's X11 backend.
```

with:

```markdown
Wayland-first. On KDE Plasma, Sway, Hyprland and other compositors that implement `ext-data-control-v1` or `wlr-data-control-v1`, Pasta reads the clipboard directly. On GNOME (tested on GNOME 50), whose compositor offers no such protocol, Pasta installs a small GNOME Shell extension, `clipboard@pasta.launcher`: after installing, log out and back in once, then click **Enable** in Pasta's launcher (or turn on "Pasta clipboard" in GNOME's Extensions app). X11 falls back through GPUI's X11 backend.
```

- [ ] **Step 3: `AGENTS.md`**

In the `src/platform/` bullet, append this sentence at the end of the paragraph:

```markdown
Clipboard access on Linux has three paths chosen once at start-up (`clipboard_path()` in `linux/mod.rs`): Wayland data-control, X11, and on GNOME a bundled GNOME Shell extension (`gnome-extension/clipboard@pasta.launcher/`) that talks to `linux/gnome_bridge.rs` and `linux/gnome_bridge_client.rs` over D-Bus, with `linux/gnome_extension_status.rs` driving the launcher's capture banner. Its end-to-end tests live in `gnome-extension/tests/` and run only locally (see `docs/linux-platform-notes.md`).
```

- [ ] **Step 4: `SMOKE_TEST_CHECKLIST.md`**

Add a section after "Global shortcut registration (Linux)":

```markdown
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
```

- [ ] **Step 5: Check and commit**

Run: `git diff --check`
Expected: no whitespace errors. Then read `git diff` for the four files once and confirm every added prose paragraph is a single line (no manual line breaks inside a paragraph).

```bash
git add docs/linux-platform-notes.md README.md AGENTS.md SMOKE_TEST_CHECKLIST.md
git commit -m "docs: document the GNOME clipboard extension"
```
