# GNOME Clipboard Extension Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove or disprove, with measurements, that a GNOME Shell extension can feed Pasta clipboard changes (and accept clipboard writes from Pasta) over session D-Bus with mutual peer verification, before any of it is built into the real app.

**Architecture:** A throwaway GNOME Shell extension (`spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/`) watches `Meta.Selection` `owner-changed` and pushes each change to a throwaway Pasta-side service (`examples/gnome_bridge_spike.rs`), which answers with pipe write ends for the payloads it wants and reads them on its own threads. The reverse direction (Pasta → shell) goes through a `SetClipboard` method the extension exports. Everything runs inside a headless, fully isolated nested GNOME Shell driven by shell scripts; nothing touches the live desktop session.

**Tech Stack:** GJS ESM extension for GNOME Shell 50; Rust 2024 + `zbus` 5.14 (blocking builder, async interface methods) + `std::io::pipe`; Python 3 + PyGObject (GTK 4, Gio) as test clients; POSIX `sh` harness around `dbus-run-session` + `gnome-shell --headless`.

**Spec:** the decisions recorded in the "Decisions this spike works from" section below (settled in conversation on 2026-10-02 and saved in project memory as `pasta-gnome-wayland-extension-plan`). There is no separate spec file.

## Decisions this spike works from

1. **Channel:** session D-Bus, initiated by the extension on every clipboard change; each side verifies the other's executable before exchanging data. No interface lets an arbitrary client ask for the clipboard.
2. **Scope:** clipboard only (capture, write-back, secret auto-clear). Window behaviour is out of scope.
3. **Fallback:** a message plus an "Enable" button calling `org.gnome.Shell.Extensions.EnableExtension` on user click. (Not built in this spike; Task 1 records the install/enable facts it depends on.)
4. **Delivery:** bundled with Pasta's packages. (Not built in this spike.)

**Plan-level refinement of decision 1 (needs the user's sign-off before execution):** GJS cannot create a pipe — `GLibUnix.open_pipe` exposes its `fds` array as an *in* argument (verified by introspecting the GLibUnix 2.0 typelib), so the extension has no way to mint file descriptors to send. The spike therefore uses **push-then-reply-with-pipes**: the extension calls `Offer(mimetypes)`, Pasta's reply carries one pipe write end per payload it wants, and the extension streams each payload into its pipe. Data still only moves in response to the extension's push, to a verified peer. Side effect: Pasta, not the extension, decides which formats it wants. The alternative is inlining payload bytes (`ay`) in the push; the session bus here allows messages up to 1,000,000,000 bytes (`max_message_size` in `/usr/share/dbus-1/session.conf`), so it is viable but copies every image through `dbus-daemon`.

## Already verified before this plan (do not re-litigate; Task 1 records them)

All on this machine: Ubuntu 26.04.1, GNOME Shell 50.1, gjs 1.88.0, rustc 1.95.0, session bus is `dbus-daemon`.

- A headless nested shell runs fully isolated with `dbus-run-session -- gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800 --wayland-display <name>` plus private `XDG_{CONFIG,DATA,CACHE,STATE}_HOME` and `GSETTINGS_BACKEND=memory`; the live shell (pid unchanged) was unaffected.
- **Extensions are discovered only at shell start-up.** Copying an extension into the user extensions dir of a running shell: `GetExtensionInfo` returns an empty dict and `EnableExtension` returns `false`. The same extension present before start-up: listed, `EnableExtension` returns `true`, state `1`, its `enable()` runs. Consequence for decision 3: after installing or upgrading the bundled extension the user must log out and back in once before "Enable" can work.
- In the nested shell, a GTK 4 client's copy fires `owner-changed` and `Meta.Selection.transfer_async` reads it back: 10 bytes of text, and a 3,173,458-byte PNG in about 20 ms.
- Ownership churns around every real copy: an `owner-changed` with **no** mimetypes, a re-announcement of the **previous** content, then the new content; and when the source app exits, another empty event followed by the same content again (GNOME's clipboard manager taking over).
- Typelib signatures (GNOME 50): `Meta.Selection.transfer_async(selection_type, mimetype, size, output, cancellable, callback)`, `Meta.Selection.get_mimetypes(selection_type)`, `Meta.SelectionType.SELECTION_CLIPBOARD`, `St.Clipboard.set_content(type, mimetype, bytes)`, `Gio.DBusConnection.call_with_unix_fd_list(...)`, `GioUnix.OutputStream.new(fd, close_fd)`. GJS's exported-object dispatcher calls `<Method>Async(params, invocation, fdList)` when defined (from `/org/gnome/gjs/modules/core/overrides/Gio.js` in libgjs).
- zbus 5.14: `#[zbus::interface]` methods accept `#[zbus(header)] Header<'_>` and `#[zbus(connection)] &zbus::Connection`; `zbus::fdo::DBusProxy::get_connection_unix_process_id(BusName)`; `zbus::blocking::fdo::DBusProxy::get_name_owner(BusName)`; `zbus::blocking::connection::Builder::{session, name, serve_at, build}`; `zvariant::OwnedFd: From<std::os::fd::OwnedFd>` and `Serialize`; `zvariant::Fd: From<&T> where T: AsFd`; `zbus::fdo::Error::AccessDenied(String)`.
- `/proc/<pid>/exe` of the live shell is `/usr/bin/gnome-shell`.

## Global Constraints

- Nothing in this spike runs inside the live GNOME session. Every scenario goes through `spike/gnome-clipboard/nested-shell.sh`. An exception inside the live shell can end the whole Wayland session and every app in it.
- Extension code never blocks the compositor: no `*_sync` D-Bus calls, no blocking reads or writes; only async Gio APIs. Reading a `/proc/<pid>/exe` symlink with `GLib.file_read_link` is allowed (a local syscall).
- Neither side logs clipboard *contents*: only mimetype names, byte counts, timings and the first 12 hex characters of a SHA-256.
- Extension UUID is `pasta-clipboard-spike@pasta.launcher`. It is deliberately spike-named: the real UUID becomes part of every user's `enabled-extensions` setting and is effectively permanent, so it is chosen later, not here.
- D-Bus contract (the only shared surface; each side's comments describe its own reasons, never the other side's internals):
  - Pasta owns `com.pasta.Launcher`, object `/com/pasta/Launcher/Clipboard`, interface `com.pasta.Launcher.Clipboard1`, method `Offer(as mimetypes) -> (a{sh} writers)`. Only a caller whose executable is `/usr/bin/gnome-shell` is served; others get `org.freedesktop.DBus.Error.AccessDenied`. The caller writes each payload into the returned fd for that mimetype and closes it.
  - The shell side owns `com.pasta.Launcher.ShellBridge`, object `/com/pasta/Launcher/ShellBridge`, interface `com.pasta.Launcher.ShellBridge1`, method `SetClipboard(s mimetype, h data)`. Only a caller whose executable's basename is `pasta-launcher` is served. It reads `data` to EOF, then sets the clipboard.
  - An executable path ending in ` (deleted)` (binary replaced by an upgrade while running) is compared with that suffix stripped.
- Rust: `cargo fmt --all -- --check`, `cargo clippy --all-targets --no-deps` with no new warnings versus `main`, and `RUSTFLAGS="-D warnings" cargo test` must pass. The example must still compile on macOS (CI builds there), so all Linux-only code sits behind `#[cfg(target_os = "linux")]`.
- Markdown files: one line per paragraph, never hard-wrapped.
- Commits use Conventional Commits (`chore(spike): …`, `test(spike): …`, `docs(spike): …`). Executed under subagent-driven-development in an isolated worktree on branch `chore/gnome-clipboard-spike`. Never push.

---

## File Structure

| Path | Responsibility |
|---|---|
| `spike/gnome-clipboard/nested-shell.sh` | Builds the isolated environment and launches `dbus-run-session`. |
| `spike/gnome-clipboard/nested-inner.sh` | Inside the private session: starts the headless shell, waits for it, enables the spike extension, runs one scenario. |
| `spike/gnome-clipboard/lib.sh` | Shared scenario helpers (`wait_for_line`, `refute_line`, `start_pasta`). |
| `spike/gnome-clipboard/clip_tool.py` | Test clients: set clipboard from a GTK app, make test PNGs, impostor services, non-Pasta caller. |
| `spike/gnome-clipboard/scenario-*.sh` | One scenario per behaviour under test; each prints `PASS <name>` or `FAIL …` and exits non-zero on failure. |
| `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/metadata.json` | Extension metadata. |
| `…/extension.js` | Wiring: `owner-changed` → log → offer; owns the `ShellBridge` lifetime. |
| `…/peer.js` | Who owns a bus name / who is calling, by executable path. |
| `…/offer.js` | Push a change to Pasta and stream payloads into the returned pipes. |
| `…/bridge.js` | The exported `SetClipboard` method. |
| `examples/gnome_bridge_spike.rs` | Pasta-side spike: `serve` (Offer) and `write` (SetClipboard client), with unit tests for the pure helpers. |
| `Cargo.toml` | `[[example]]` entry enabling the example's unit tests. |
| `docs/gnome-extension-spike-findings.md` | Results, measurements, and implications. The deliverable a human reads. |

---

### Task 1: Isolated nested-shell harness and baseline watcher

**Files:**
- Create: `spike/gnome-clipboard/nested-shell.sh`
- Create: `spike/gnome-clipboard/nested-inner.sh`
- Create: `spike/gnome-clipboard/lib.sh`
- Create: `spike/gnome-clipboard/clip_tool.py`
- Create: `spike/gnome-clipboard/scenario-watch.sh`
- Create: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/metadata.json`
- Create: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/extension.js`
- Create: `docs/gnome-extension-spike-findings.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `nested-shell.sh <scenario> [args…]` runs a scenario with `NEST` (scratch dir), `HERE` (this directory), `UUID`, `SPIKE_LOG` (shell log), `WAYLAND_DISPLAY`, `GDK_BACKEND=wayland` exported. `lib.sh` functions `wait_for_line <file> <pattern> <timeout-s>`, `refute_line <file> <pattern>`. `clip_tool.py set-text <text>`. Extension logs lines prefixed `PASTA-SPIKE ` (appearing in `SPIKE_LOG` as `GNOME Shell-Message: <time>: PASTA-SPIKE …`), including `PASTA-SPIKE enabled` and `PASTA-SPIKE owner-changed source=<String(source)> mimetypes=<JSON array>`.

- [ ] **Step 1: Write the outer harness**

`spike/gnome-clipboard/nested-shell.sh`:

```sh
#!/bin/sh
# Runs one spike scenario inside a throwaway, headless GNOME Shell.
#
# Isolation: a private D-Bus session (dbus-run-session), a private Wayland
# display, XDG config/data/cache/state under $NEST, and the in-memory
# GSettings backend. Nothing here can reach the live desktop session or its
# settings. Never load the spike extension into the live shell: an exception
# there can end the whole Wayland session.
#
# Usage: nested-shell.sh <scenario-script> [args...]
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
UUID=pasta-clipboard-spike@pasta.launcher
NEST=${NEST:-$(mktemp -d "${TMPDIR:-/tmp}/pasta-spike.XXXXXX")}
mkdir -p "$NEST/config" "$NEST/cache" "$NEST/state" "$NEST/bin" "$NEST/data/gnome-shell/extensions"
# GNOME Shell discovers extensions only at start-up, so the extension must be
# in place before the shell launches.
rm -rf "$NEST/data/gnome-shell/extensions/$UUID"
cp -r "$HERE/$UUID" "$NEST/data/gnome-shell/extensions/"
export XDG_CONFIG_HOME="$NEST/config" XDG_DATA_HOME="$NEST/data" \
    XDG_CACHE_HOME="$NEST/cache" XDG_STATE_HOME="$NEST/state" \
    GSETTINGS_BACKEND=memory NEST HERE UUID
echo "nest: $NEST"
exec dbus-run-session -- "$HERE/nested-inner.sh" "$@"
```

- [ ] **Step 2: Write the inner harness**

`spike/gnome-clipboard/nested-inner.sh`:

```sh
#!/bin/sh
# Runs inside the private D-Bus session created by nested-shell.sh.
set -eu
DISPLAY_NAME=pasta-spike-$$
export SPIKE_LOG="$NEST/shell.log"
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800 \
    --wayland-display "$DISPLAY_NAME" >"$SPIKE_LOG" 2>&1 &
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

export WAYLAND_DISPLAY="$DISPLAY_NAME" GDK_BACKEND=wayland
"$@"
```

- [ ] **Step 3: Write the shared scenario helpers**

`spike/gnome-clipboard/lib.sh`:

```sh
# Shared helpers for scenario scripts. Sourced, not executed.

# wait_for_line <file> <grep-pattern> <timeout-seconds>
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

# refute_line <file> <grep-pattern>
refute_line() {
    if [ -f "$1" ] && grep -aqE -- "$2" "$1"; then
        echo "FAIL unexpected line in $1: $(grep -aE -- "$2" "$1" | head -1)"
        return 1
    fi
    return 0
}
```

- [ ] **Step 4: Write the GTK clipboard client**

`spike/gnome-clipboard/clip_tool.py` (later tasks add subcommands to the `COMMANDS` table; keep that shape):

```python
"""Test clients for the GNOME clipboard spike. Only ever run inside nested-shell.sh."""
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

    app = Gtk.Application(application_id="dev.pasta.SpikeClip", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run(None)


def set_text(text):
    hold_clipboard(lambda clipboard: clipboard.set(text))


COMMANDS = {
    "set-text": set_text,
}

if __name__ == "__main__":
    command, *args = sys.argv[1:]
    COMMANDS[command](*args)
```

- [ ] **Step 5: Write the baseline extension**

`spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/metadata.json`:

```json
{
  "uuid": "pasta-clipboard-spike@pasta.launcher",
  "name": "Pasta clipboard spike",
  "description": "Throwaway spike. Not for installation in a live session.",
  "shell-version": ["50"]
}
```

`spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/extension.js`:

```js
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

function log(message) {
    console.log(`PASTA-SPIKE ${message}`);
}

export default class PastaClipboardSpike extends Extension {
    enable() {
        this._selection = global.display.get_selection();
        this._ownerChangedId = this._selection.connect('owner-changed', (selection, type, source) => {
            if (type !== Meta.SelectionType.SELECTION_CLIPBOARD)
                return;
            const mimetypes = selection.get_mimetypes(type);
            // String(source) names the concrete source type; recorded so the
            // findings can say which events are replays by the clipboard manager.
            log(`owner-changed source=${String(source)} mimetypes=${JSON.stringify(mimetypes)}`);
        });
        log('enabled');
    }

    disable() {
        this._selection.disconnect(this._ownerChangedId);
        this._selection = null;
        log('disabled');
    }
}
```

- [ ] **Step 6: Write the scenario**

`spike/gnome-clipboard/scenario-watch.sh`:

```sh
#!/bin/sh
# A Wayland client's copy reaches the extension.
set -eu
. "$HERE/lib.sh"
wait_for_line "$SPIKE_LOG" "PASTA-SPIKE enabled" 10
python3 "$HERE/clip_tool.py" set-text "spike text"
wait_for_line "$SPIKE_LOG" 'PASTA-SPIKE owner-changed .*mimetypes=\[.*"text/plain' 10
grep -a "PASTA-SPIKE owner-changed" "$SPIKE_LOG" | sed 's/^.*PASTA-SPIKE/PASTA-SPIKE/'
echo "PASS scenario-watch"
```

- [ ] **Step 7: Make scripts executable and run the scenario**

Run:

```bash
chmod +x spike/gnome-clipboard/*.sh
spike/gnome-clipboard/nested-shell.sh "$PWD/spike/gnome-clipboard/scenario-watch.sh"
```

Expected: the last line is `PASS scenario-watch`, preceded by several `PASTA-SPIKE owner-changed source=… mimetypes=…` lines (some with `[]`). Also confirm the live shell is unaffected: `pgrep -x gnome-shell` prints the same pid before and after.

- [ ] **Step 8: Seed the findings document**

`docs/gnome-extension-spike-findings.md` — write it with these sections and fill in the Task 1 results now (one line per paragraph; no hard wraps):

```markdown
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

(Fill in from "Already verified before this plan": start-up-only discovery, `EnableExtension` returning `false` for a hot-installed extension and `true` when present at start-up, and the resulting log-out requirement after install or upgrade.)

### Event shape

(Paste the `owner-changed` lines from scenario-watch, including the `source=` strings, and state which events are empty, which re-announce previous content, and which follow the source app exiting.)

### Large payloads (question 1)

### Peer verification (question 2)

### Write-back

## Implications for the real implementation

## Open items
```

- [ ] **Step 9: Commit**

```bash
git add spike/gnome-clipboard docs/gnome-extension-spike-findings.md
git commit -m "chore(spike): add isolated nested GNOME Shell harness and clipboard watcher"
```

---

### Task 2: Pasta-side `Offer` service with caller verification

**Files:**
- Create: `examples/gnome_bridge_spike.rs`
- Modify: `Cargo.toml` (append an `[[example]]` table after the `[dependencies]`/target tables)
- Modify: `spike/gnome-clipboard/lib.sh` (add `start_pasta`)
- Create: `spike/gnome-clipboard/scenario-reject-caller.sh`

**Interfaces:**
- Consumes: Task 1 harness and helpers.
- Produces: binary `gnome_bridge_spike serve`, owning `com.pasta.Launcher` and serving `com.pasta.Launcher.Clipboard1.Offer(as) -> a{sh}` at `/com/pasta/Launcher/Clipboard`. Stderr log lines prefixed `PASTA-SPIKE-RS `: `serving com.pasta.Launcher`, `rejected Offer from <exe>`, `Offer mimetypes=[…] wanted=[…]`, `RECEIVED <mime> <n> bytes in <ms> ms sha256=<12 hex>[ TRUNCATED]`. Pure helpers `is_shell_executable(&Path) -> bool`, `select_wanted_mimes(&[String]) -> Vec<String>`, `read_capped(impl Read, u64) -> io::Result<(Vec<u8>, bool)>`, `short_sha256(&[u8]) -> String`. Shell helper `start_pasta` copies the built example to `$NEST/bin/pasta-launcher` and starts `serve` with stderr to `$NEST/pasta.log`.

- [ ] **Step 1: Register the example's tests**

Append to `Cargo.toml`:

```toml
# Throwaway spike (see docs/gnome-extension-spike-findings.md). `test = true`
# so `cargo test` runs its unit tests.
[[example]]
name = "gnome_bridge_spike"
test = true
```

- [ ] **Step 2: Write the failing tests with stub helpers**

Create `examples/gnome_bridge_spike.rs`:

```rust
//! Spike: Pasta's side of a clipboard bridge to GNOME Shell. Throwaway; see
//! docs/gnome-extension-spike-findings.md.
//!
//!     cargo build --example gnome_bridge_spike
//!     gnome_bridge_spike serve
//!     gnome_bridge_spike write <mimetype> <file>

#[cfg(target_os = "linux")]
mod spike {
    use std::io::Read;
    use std::path::Path;

    /// The only executable allowed to call `Offer` or to own the ShellBridge name.
    pub(crate) const SHELL_EXE: &str = "/usr/bin/gnome-shell";

    pub(crate) fn is_shell_executable(_exe: &Path) -> bool {
        unimplemented!()
    }

    pub(crate) fn select_wanted_mimes(_offered: &[String]) -> Vec<String> {
        unimplemented!()
    }

    pub(crate) fn read_capped(_reader: impl Read, _cap: u64) -> std::io::Result<(Vec<u8>, bool)> {
        unimplemented!()
    }

    pub(crate) fn short_sha256(_bytes: &[u8]) -> String {
        unimplemented!()
    }

    pub(crate) fn main() {}

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Cursor;
        use std::path::PathBuf;

        fn strings(items: &[&str]) -> Vec<String> {
            items.iter().map(|s| (*s).to_owned()).collect()
        }

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
            assert!(is_shell_executable(&PathBuf::from("/usr/bin/gnome-shell (deleted)")));
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
            assert_eq!(select_wanted_mimes(&strings(&["text/plain"])), strings(&["text/plain"]));
        }

        #[test]
        fn wants_nothing_from_unknown_formats() {
            assert!(select_wanted_mimes(&strings(&["application/x-custom", "TARGETS"])).is_empty());
        }

        #[test]
        fn reads_everything_under_the_cap() {
            let (bytes, truncated) = read_capped(Cursor::new(b"hello".to_vec()), 5).unwrap();
            assert_eq!(bytes, b"hello");
            assert!(!truncated);
        }

        #[test]
        fn cuts_off_and_reports_payloads_over_the_cap() {
            let (bytes, truncated) = read_capped(Cursor::new(b"hello!".to_vec()), 5).unwrap();
            assert_eq!(bytes, b"hello");
            assert!(truncated);
        }

        #[test]
        fn short_sha256_is_the_first_12_hex_chars() {
            // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
            assert_eq!(short_sha256(b"abc"), "ba7816bf8f01");
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    spike::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --example gnome_bridge_spike`
Expected: 8 tests run, all FAIL with `not implemented`.

- [ ] **Step 4: Implement the pure helpers**

Replace the four stubs in `examples/gnome_bridge_spike.rs` with:

```rust
    /// Payloads larger than this are cut off and reported as truncated instead
    /// of being held in memory.
    pub(crate) const MAX_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;

    pub(crate) fn is_shell_executable(exe: &Path) -> bool {
        let path = exe.to_string_lossy();
        path.strip_suffix(" (deleted)").unwrap_or(&path) == SHELL_EXE
    }

    /// The payloads worth pulling from an offer: the first image, plain text
    /// (UTF-8 preferred), and file-manager file references. Every other
    /// format is recorded by name only.
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

    /// Reads `reader` to EOF, keeping at most `cap` bytes. The flag says
    /// whether anything past `cap` was discarded.
    pub(crate) fn read_capped(reader: impl Read, cap: u64) -> std::io::Result<(Vec<u8>, bool)> {
        let mut bytes = Vec::new();
        reader.take(cap + 1).read_to_end(&mut bytes)?;
        let truncated = bytes.len() as u64 > cap;
        bytes.truncate(cap as usize);
        Ok((bytes, truncated))
    }

    pub(crate) fn short_sha256(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))[..12].to_owned()
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --example gnome_bridge_spike`
Expected: 8 passed.

- [ ] **Step 6: Implement `serve`**

In `examples/gnome_bridge_spike.rs`, inside `mod spike`, add these imports at the top of the module (merge with the existing `use` lines):

```rust
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Instant;
```

and replace `pub(crate) fn main() {}` with:

```rust
    const PASTA_NAME: &str = "com.pasta.Launcher";
    const CLIPBOARD_PATH: &str = "/com/pasta/Launcher/Clipboard";

    fn log(message: &str) {
        eprintln!("PASTA-SPIKE-RS {message}");
    }

    async fn executable_of(
        conn: &zbus::Connection,
        name: zbus::names::BusName<'_>,
    ) -> zbus::fdo::Result<PathBuf> {
        let pid = zbus::fdo::DBusProxy::new(conn)
            .await?
            .get_connection_unix_process_id(name)
            .await?;
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .map_err(|err| zbus::fdo::Error::Failed(format!("reading /proc/{pid}/exe: {err}")))
    }

    /// Reads one payload to EOF on its own thread. There is deliberately no
    /// deadline: the spike records whether writers always close their end.
    fn spawn_payload_reader(mime: String, reader: std::io::PipeReader, offered_at: Instant) {
        std::thread::Builder::new()
            .name("spike-payload-reader".to_owned())
            .spawn(move || match read_capped(reader, MAX_PAYLOAD_BYTES) {
                Ok((bytes, truncated)) => log(&format!(
                    "RECEIVED {mime} {} bytes in {} ms sha256={}{}",
                    bytes.len(),
                    offered_at.elapsed().as_millis(),
                    short_sha256(&bytes),
                    if truncated { " TRUNCATED" } else { "" }
                )),
                Err(err) => log(&format!("read of {mime} failed: {err}")),
            })
            .expect("spawn payload reader thread");
    }

    struct Clipboard;

    #[zbus::interface(name = "com.pasta.Launcher.Clipboard1")]
    impl Clipboard {
        /// Answers a clipboard change with one pipe write end per wanted payload.
        async fn offer(
            &self,
            mimetypes: Vec<String>,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] conn: &zbus::Connection,
        ) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::OwnedFd>> {
            let offered_at = Instant::now();
            let caller = header
                .sender()
                .ok_or_else(|| zbus::fdo::Error::AccessDenied("message has no sender".to_owned()))?
                .to_owned();
            let exe = executable_of(conn, caller.into()).await?;
            if !is_shell_executable(&exe) {
                log(&format!("rejected Offer from {}", exe.display()));
                return Err(zbus::fdo::Error::AccessDenied(format!(
                    "{} is not {SHELL_EXE}",
                    exe.display()
                )));
            }

            let wanted = select_wanted_mimes(&mimetypes);
            log(&format!("Offer mimetypes={mimetypes:?} wanted={wanted:?}"));
            let mut writers = HashMap::new();
            for mime in wanted {
                let (reader, writer) = std::io::pipe()
                    .map_err(|err| zbus::fdo::Error::Failed(format!("pipe: {err}")))?;
                spawn_payload_reader(mime.clone(), reader, offered_at);
                writers.insert(mime, std::os::fd::OwnedFd::from(writer).into());
            }
            Ok(writers)
        }
    }

    fn serve() -> Result<(), Box<dyn std::error::Error>> {
        let _conn = zbus::blocking::connection::Builder::session()?
            .name(PASTA_NAME)?
            .serve_at(CLIPBOARD_PATH, Clipboard)?
            .build()?;
        log(&format!("serving {PASTA_NAME}"));
        loop {
            std::thread::park();
        }
    }

    pub(crate) fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let result = match args.as_slice() {
            [command] if command == "serve" => serve(),
            _ => Err("usage: gnome_bridge_spike serve | write <mimetype> <file>".into()),
        };
        if let Err(err) = result {
            log(&format!("error: {err}"));
            std::process::exit(1);
        }
    }
```

- [ ] **Step 7: Build and lint**

Run:

```bash
cargo build --example gnome_bridge_spike
cargo clippy --all-targets --no-deps 2>&1 | grep -c 'examples/gnome_bridge_spike.rs'
cargo fmt --all -- --check
```

Expected: build succeeds; the clippy count is `0` (the spike adds no code outside this example, so any new warning would point into it); fmt reports nothing. Fix anything before moving on.

- [ ] **Step 8: Add `start_pasta` and the rejection scenario**

Append to `spike/gnome-clipboard/lib.sh`:

```sh
# start_pasta: run the spike service under the executable name the extension
# trusts. Stderr goes to $NEST/pasta.log.
REPO_ROOT=$(cd "$HERE/../.." && pwd)
start_pasta() {
    cp "$REPO_ROOT/target/debug/examples/gnome_bridge_spike" "$NEST/bin/pasta-launcher"
    "$NEST/bin/pasta-launcher" serve 2>>"$NEST/pasta.log" &
    PASTA_PID=$!
    trap 'kill "$PASTA_PID" 2>/dev/null || true' EXIT
    wait_for_line "$NEST/pasta.log" "serving com.pasta.Launcher" 10
}
```

`spike/gnome-clipboard/scenario-reject-caller.sh`:

```sh
#!/bin/sh
# Pasta refuses Offer from anything that is not /usr/bin/gnome-shell.
set -eu
. "$HERE/lib.sh"
start_pasta
if out=$(busctl --user call com.pasta.Launcher /com/pasta/Launcher/Clipboard \
        com.pasta.Launcher.Clipboard1 Offer as 1 text/plain 2>&1); then
    echo "FAIL busctl Offer was accepted: $out"
    exit 1
fi
echo "busctl: $out"
case "$out" in
    *"is not /usr/bin/gnome-shell"*) ;;
    *) echo "FAIL unexpected error text"; exit 1 ;;
esac
wait_for_line "$NEST/pasta.log" "rejected Offer from /usr/bin/busctl" 5
echo "PASS scenario-reject-caller"
```

- [ ] **Step 9: Run it**

Run:

```bash
chmod +x spike/gnome-clipboard/*.sh
spike/gnome-clipboard/nested-shell.sh "$PWD/spike/gnome-clipboard/scenario-reject-caller.sh"
```

Expected: `busctl: Call failed: /usr/bin/busctl is not /usr/bin/gnome-shell` then `PASS scenario-reject-caller`.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml examples/gnome_bridge_spike.rs spike/gnome-clipboard
git commit -m "chore(spike): add Pasta-side Offer service with caller verification"
```

---

### Task 3: Extension pushes changes and streams payloads into Pasta's pipes

**Files:**
- Create: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/peer.js`
- Create: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/offer.js`
- Modify: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/extension.js` (the `owner-changed` handler and imports)
- Modify: `spike/gnome-clipboard/clip_tool.py` (add `make-png`, `set-file`)
- Create: `spike/gnome-clipboard/scenario-offer.sh`
- Modify: `docs/gnome-extension-spike-findings.md` ("Large payloads" section)

**Interfaces:**
- Consumes: Task 2's `Offer` contract and `start_pasta`; Task 1's extension `log` helper.
- Produces: `peer.js` exports `executableOf(busName) -> Promise<string>`, `ownerOf(wellKnownName) -> Promise<{unique, exe}>`, `exeBasename(path) -> string` (strips ` (deleted)`). `offer.js` exports `offerToPasta(selection, mimetypes, log) -> Promise<void>`, logging `offered <n> payload(s) in <ms> ms` or `refusing to offer: com.pasta.Launcher is owned by <exe>`. `clip_tool.py make-png <path> <width> <height> <noise_every_n_rows>` and `set-file <mimetype> <path>`.

- [ ] **Step 1: Write `peer.js`**

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

- [ ] **Step 2: Write `offer.js`**

```js
// Pushes a clipboard change to Pasta and streams the payloads it asks for into
// the pipe write ends its reply carries.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {exeBasename, ownerOf} from './peer.js';

const PASTA_NAME = 'com.pasta.Launcher';
const PASTA_PATH = '/com/pasta/Launcher/Clipboard';
const PASTA_IFACE = 'com.pasta.Launcher.Clipboard1';
const PASTA_EXE = 'pasta-launcher';

function transferInto(selection, mimetype, fd) {
    return new Promise((resolve, reject) => {
        const stream = GioUnix.OutputStream.new(fd, true);
        selection.transfer_async(Meta.SelectionType.SELECTION_CLIPBOARD, mimetype, -1, stream, null,
            (source, result) => {
                try {
                    source.transfer_finish(result);
                    resolve();
                } catch (e) {
                    reject(e);
                } finally {
                    // Closing is what tells the reader the payload is complete.
                    stream.close(null);
                }
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

export async function offerToPasta(selection, mimetypes, log) {
    let owner;
    try {
        owner = await ownerOf(PASTA_NAME);
    } catch (e) {
        log(`no ${PASTA_NAME} on the bus; not offering (${e.message})`);
        return;
    }
    if (exeBasename(owner.exe) !== PASTA_EXE) {
        log(`refusing to offer: ${PASTA_NAME} is owned by ${owner.exe}`);
        return;
    }

    const started = GLib.get_monotonic_time();
    // Address the verified unique name, not the well-known one, so a name
    // change between the check and the call cannot redirect the payload.
    const [reply, fdList] = await callOffer(owner.unique, mimetypes);
    const wanted = reply.deepUnpack()[0];
    // steal_fds() hands ownership to us; with get() the list would keep a
    // duplicate of every write end open and the reader would never see EOF.
    const fds = fdList ? fdList.steal_fds() : [];
    await Promise.all(Object.entries(wanted).map(
        ([mimetype, index]) => transferInto(selection, mimetype, fds[index])));
    log(`offered ${Object.keys(wanted).length} payload(s) in ${(GLib.get_monotonic_time() - started) / 1000} ms`);
}
```

- [ ] **Step 3: Wire it into `extension.js`**

In `extension.js`, add the import below the existing ones:

```js
import {offerToPasta} from './offer.js';
```

and replace this line inside the `owner-changed` handler:

```js
            log(`owner-changed source=${String(source)} mimetypes=${JSON.stringify(mimetypes)}`);
```

with:

```js
            log(`owner-changed source=${String(source)} mimetypes=${JSON.stringify(mimetypes)}`);
            if (mimetypes.length === 0)
                return;
            offerToPasta(selection, mimetypes, log).catch(e => log(`offer failed: ${e.message}`));
```

- [ ] **Step 4: Add the PNG and file clients to `clip_tool.py`**

Add above `COMMANDS`:

```python
def make_png(path, width, height, noise_every_n_rows):
    """Write a PNG whose size is controlled by how many rows are random noise."""
    import os

    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf

    width, height, every = int(width), int(height), int(noise_every_n_rows)
    rowstride = width * 3
    pixels = bytearray(rowstride * height)
    for y in range(height):
        row = os.urandom(rowstride) if y % every == 0 else bytes([y % 256, 90, 200]) * width
        pixels[y * rowstride:(y + 1) * rowstride] = row
    pixbuf = GdkPixbuf.Pixbuf.new_from_bytes(
        GLib.Bytes.new(bytes(pixels)), GdkPixbuf.Colorspace.RGB, False, 8, width, height, rowstride)
    pixbuf.savev(path, "png", [], [])


def set_file(mimetype, path):
    data = open(path, "rb").read()
    hold_clipboard(lambda clipboard: clipboard.set_content(
        Gdk.ContentProvider.new_for_bytes(mimetype, GLib.Bytes.new(data))))
```

and extend the table:

```python
COMMANDS = {
    "set-text": set_text,
    "make-png": make_png,
    "set-file": set_file,
}
```

- [ ] **Step 5: Write the scenario**

`spike/gnome-clipboard/scenario-offer.sh`:

```sh
#!/bin/sh
# Text, a ~3 MB PNG and a ~15 MB incompressible PNG reach Pasta intact.
set -eu
. "$HERE/lib.sh"
start_pasta

python3 "$HERE/clip_tool.py" set-text "spike text"
wait_for_line "$NEST/pasta.log" "RECEIVED text/plain;charset=utf-8 10 bytes .*sha256=$(printf 'spike text' | sha256sum | cut -c1-12)" 10

python3 "$HERE/clip_tool.py" make-png "$NEST/mid.png" 2880 1800 5
python3 "$HERE/clip_tool.py" make-png "$NEST/noise.png" 2880 1800 1
for png in mid noise; do
    size=$(stat -c %s "$NEST/$png.png")
    sha=$(sha256sum "$NEST/$png.png" | cut -c1-12)
    python3 "$HERE/clip_tool.py" set-file image/png "$NEST/$png.png"
    wait_for_line "$NEST/pasta.log" "RECEIVED image/png $size bytes in [0-9]+ ms sha256=$sha\$" 15
done

refute_line "$NEST/pasta.log" "TRUNCATED|failed"
echo "--- pasta.log"; cat "$NEST/pasta.log"
echo "--- extension"; grep -a "PASTA-SPIKE" "$SPIKE_LOG" | sed 's/^.*PASTA-SPIKE/PASTA-SPIKE/'
echo "PASS scenario-offer"
```

- [ ] **Step 6: Run it**

Run:

```bash
cargo build --example gnome_bridge_spike
chmod +x spike/gnome-clipboard/*.sh
spike/gnome-clipboard/nested-shell.sh "$PWD/spike/gnome-clipboard/scenario-offer.sh"
```

Expected: `PASS scenario-offer`, with `RECEIVED` lines for text and both PNGs, no `TRUNCATED` or `failed`. If a `RECEIVED` line never appears, check `pasta.log` for an `Offer` line without a matching `RECEIVED`: that means a write end stayed open somewhere (see the `steal_fds` comment) — record it as a finding rather than adding a timeout.

- [ ] **Step 7: Record the measurements**

In `docs/gnome-extension-spike-findings.md` → "Large payloads", record for each payload: bytes, ms from `Offer` to `RECEIVED`, the extension's `offered … in … ms`, and the count of `RECEIVED` lines per payload (replays from the event churn show up as repeats). Compare with the Xwayland baseline measured earlier in the conversation (3.2 MB in about 40 ms, 15.6 MB in about 145 ms via `xclip`).

- [ ] **Step 8: Commit**

```bash
git add spike/gnome-clipboard docs/gnome-extension-spike-findings.md
git commit -m "chore(spike): push clipboard changes to Pasta and stream payloads over pipes"
```

---

### Task 4: The extension refuses an impostor Pasta

**Files:**
- Modify: `spike/gnome-clipboard/clip_tool.py` (add `impostor`)
- Create: `spike/gnome-clipboard/scenario-impostor-pasta.sh`
- Modify: `docs/gnome-extension-spike-findings.md` ("Peer verification" section)

**Interfaces:**
- Consumes: Task 3's `offerToPasta` refusal log line.
- Produces: `clip_tool.py impostor <pasta|bridge> <seconds>`, which owns the corresponding well-known name, prints `IMPOSTOR owns <name>`, and prints `IMPOSTOR got <Method>` for any call it receives. Task 5 uses the `bridge` kind.

- [ ] **Step 1: Add the impostor to `clip_tool.py`**

Add above `COMMANDS`:

```python
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
```

and add `"impostor": impostor,` to `COMMANDS`.

- [ ] **Step 2: Write the scenario**

`spike/gnome-clipboard/scenario-impostor-pasta.sh`:

```sh
#!/bin/sh
# With a non-Pasta process holding com.pasta.Launcher, the extension sends it nothing.
set -eu
. "$HERE/lib.sh"
python3 "$HERE/clip_tool.py" impostor pasta 12 >"$NEST/impostor.log" 2>&1 &
wait_for_line "$NEST/impostor.log" "IMPOSTOR owns com.pasta.Launcher" 10
python3 "$HERE/clip_tool.py" set-text "for pasta only"
wait_for_line "$SPIKE_LOG" "PASTA-SPIKE refusing to offer: com.pasta.Launcher is owned by /usr/bin/python3" 10
refute_line "$NEST/impostor.log" "IMPOSTOR got"
echo "PASS scenario-impostor-pasta"
```

- [ ] **Step 3: Run it**

Run: `spike/gnome-clipboard/nested-shell.sh "$PWD/spike/gnome-clipboard/scenario-impostor-pasta.sh"`
Expected: `PASS scenario-impostor-pasta`.

- [ ] **Step 4: Record the result**

In "Peer verification", record both directions so far (Task 2: Pasta rejects `busctl`; Task 4: the extension refuses a Python impostor), and state the limit plainly: the extension's check is a basename match, so any process the user runs that is *named* `pasta-launcher` passes it.

- [ ] **Step 5: Commit**

```bash
git add spike/gnome-clipboard docs/gnome-extension-spike-findings.md
git commit -m "test(spike): extension refuses to offer clipboard data to an impostor"
```

---

### Task 5: Write-back through `SetClipboard`, verified in both directions

**Files:**
- Create: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/bridge.js`
- Modify: `spike/gnome-clipboard/pasta-clipboard-spike@pasta.launcher/extension.js` (create and destroy the bridge)
- Modify: `examples/gnome_bridge_spike.rs` (add `write`)
- Modify: `spike/gnome-clipboard/clip_tool.py` (add `poke-bridge`)
- Create: `spike/gnome-clipboard/scenario-write.sh`
- Modify: `docs/gnome-extension-spike-findings.md` ("Write-back", "Peer verification")

**Interfaces:**
- Consumes: `peer.js` (`executableOf`, `exeBasename`), Task 3's offer path (to observe the write echoing back), Task 4's `impostor bridge`.
- Produces: `bridge.js` exports `class ShellBridge { constructor(log); destroy(); }`, logging `SetClipboard <mime> <n> bytes sha256=<12 hex>` or `rejected SetClipboard from <exe>`. `gnome_bridge_spike write <mimetype> <file>` logging `WROTE <mime> <n> bytes sha256=<12 hex>` or `refusing to write: com.pasta.Launcher.ShellBridge is owned by <exe>`. `clip_tool.py poke-bridge` printing `POKE accepted` or `POKE rejected: <message>`.

- [ ] **Step 1: Write `bridge.js`**

```js
// Accepts clipboard writes from Pasta over D-Bus and applies them with St.Clipboard.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {executableOf, exeBasename} from './peer.js';

const BRIDGE_NAME = 'com.pasta.Launcher.ShellBridge';
const BRIDGE_PATH = '/com/pasta/Launcher/ShellBridge';
const ALLOWED_CALLER_EXE = 'pasta-launcher';
const BRIDGE_XML = `<node>
  <interface name="com.pasta.Launcher.ShellBridge1">
    <method name="SetClipboard">
      <arg type="s" name="mimetype" direction="in"/>
      <arg type="h" name="data" direction="in"/>
    </method>
  </interface>
</node>`;

function readAll(fd) {
    return new Promise((resolve, reject) => {
        const input = GioUnix.InputStream.new(fd, true);
        const sink = Gio.MemoryOutputStream.new_resizable();
        sink.splice_async(input,
            Gio.OutputStreamSpliceFlags.CLOSE_SOURCE | Gio.OutputStreamSpliceFlags.CLOSE_TARGET,
            GLib.PRIORITY_DEFAULT, null,
            (stream, result) => {
                try {
                    stream.splice_finish(result);
                    resolve(stream.steal_as_bytes());
                } catch (e) {
                    reject(e);
                }
            });
    });
}

export class ShellBridge {
    constructor(log) {
        this._log = log;
        this._exported = Gio.DBusExportedObject.wrapJSObject(BRIDGE_XML, this);
        this._exported.export(Gio.DBus.session, BRIDGE_PATH);
        this._nameId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BRIDGE_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    async SetClipboardAsync([mimetype, handle], invocation, fdList) {
        // Take ownership of the incoming fds first so they are closed whatever happens next.
        const fds = fdList ? fdList.steal_fds() : [];
        const exe = await executableOf(invocation.get_sender());
        if (exeBasename(exe) !== ALLOWED_CALLER_EXE) {
            fds.forEach(fd => GLib.close(fd));
            this._log(`rejected SetClipboard from ${exe}`);
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                `${exe} is not ${ALLOWED_CALLER_EXE}`);
            return;
        }
        const bytes = await readAll(fds[handle]);
        St.Clipboard.get_default().set_content(St.ClipboardType.CLIPBOARD, mimetype, bytes);
        const sha = GLib.compute_checksum_for_bytes(GLib.ChecksumType.SHA256, bytes).slice(0, 12);
        this._log(`SetClipboard ${mimetype} ${bytes.get_size()} bytes sha256=${sha}`);
        invocation.return_value(null);
    }

    destroy() {
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
    }
}
```

- [ ] **Step 2: Create and destroy the bridge in `extension.js`**

Add the import:

```js
import {ShellBridge} from './bridge.js';
```

In `enable()`, immediately before `log('enabled');`, add:

```js
        this._bridge = new ShellBridge(log);
```

In `disable()`, as its first line, add:

```js
        this._bridge.destroy();
        this._bridge = null;
```

- [ ] **Step 3: Add `write` to the Rust spike**

In `examples/gnome_bridge_spike.rs`, inside `mod spike`, add `use std::io::Write;` to the imports, add these items after `serve`:

```rust
    const BRIDGE_NAME: &str = "com.pasta.Launcher.ShellBridge";
    const BRIDGE_PATH: &str = "/com/pasta/Launcher/ShellBridge";
    const BRIDGE_IFACE: &str = "com.pasta.Launcher.ShellBridge1";

    /// Sends `file` to the clipboard through the shell's bridge, after checking
    /// that the bridge name is held by the installed gnome-shell.
    fn write(mime: &str, file: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = std::fs::read(file)?;
        let sha = short_sha256(&bytes);
        let len = bytes.len();

        let conn = zbus::blocking::Connection::session()?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&conn)?;
        let owner = dbus.get_name_owner(zbus::names::BusName::try_from(BRIDGE_NAME)?)?;
        let pid = dbus.get_connection_unix_process_id((&owner).into())?;
        let exe = std::fs::read_link(format!("/proc/{pid}/exe"))?;
        if !is_shell_executable(&exe) {
            log(&format!("refusing to write: {BRIDGE_NAME} is owned by {}", exe.display()));
            return Err(format!("{BRIDGE_NAME} is not owned by {SHELL_EXE}").into());
        }

        let (reader, mut writer) = std::io::pipe()?;
        let feeder = std::thread::spawn(move || writer.write_all(&bytes));
        let reply = conn.call_method(
            Some(owner.as_str()),
            BRIDGE_PATH,
            Some(BRIDGE_IFACE),
            "SetClipboard",
            &(mime, zbus::zvariant::Fd::from(&reader)),
        );
        // Drop our read end before joining: if the call failed without the
        // peer reading, the feeder then gets EPIPE instead of blocking forever.
        drop(reader);
        let fed = feeder.join().map_err(|_| "feeder thread panicked")?;
        reply?;
        fed?;
        log(&format!("WROTE {mime} {len} bytes sha256={sha}"));
        Ok(())
    }
```

and extend the `match` in `main`:

```rust
            [command] if command == "serve" => serve(),
            [command, mime, file] if command == "write" => write(mime, Path::new(file)),
```

- [ ] **Step 4: Build, test and lint**

Run:

```bash
cargo build --example gnome_bridge_spike
RUSTFLAGS="-D warnings" cargo test --example gnome_bridge_spike
cargo fmt --all -- --check
```

Expected: build succeeds, 8 tests pass, fmt reports nothing. Then run `cargo clippy --all-targets --no-deps 2>&1 | grep -c 'examples/gnome_bridge_spike.rs'` and expect `0`.

- [ ] **Step 5: Add the non-Pasta caller to `clip_tool.py`**

Add above `COMMANDS`:

```python
def poke_bridge():
    """Call SetClipboard as a process that is not pasta-launcher."""
    import os

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
```

and add `"poke-bridge": poke_bridge,` to `COMMANDS`.

- [ ] **Step 6: Write the scenario**

`spike/gnome-clipboard/scenario-write.sh`:

```sh
#!/bin/sh
# Pasta can write text and an image through the bridge; the write echoes back
# through Offer with the same hash; non-Pasta callers and impostor bridges are refused.
set -eu
. "$HERE/lib.sh"
start_pasta

printf 'written by pasta' >"$NEST/text.txt"
python3 "$HERE/clip_tool.py" make-png "$NEST/mid.png" 2880 1800 5
for case in "text/plain;charset=utf-8 text.txt" "image/png mid.png"; do
    set -- $case
    sha=$(sha256sum "$NEST/$2" | cut -c1-12)
    "$NEST/bin/pasta-launcher" write "$1" "$NEST/$2" 2>>"$NEST/pasta.log"
    wait_for_line "$NEST/pasta.log" "WROTE $1 .*sha256=$sha" 5
    wait_for_line "$SPIKE_LOG" "PASTA-SPIKE SetClipboard $1 .*sha256=$sha" 5
    # The shell now owns the clipboard with our bytes; owner-changed fires and
    # the extension offers them straight back.
    wait_for_line "$NEST/pasta.log" "RECEIVED $1 .*sha256=$sha" 10
done

poke=$(python3 "$HERE/clip_tool.py" poke-bridge)
echo "$poke"
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected"; exit 1 ;;
esac
wait_for_line "$SPIKE_LOG" "PASTA-SPIKE rejected SetClipboard from /usr/bin/python3" 5

# Free the bridge name, let an impostor take it, and confirm Pasta will not write to it.
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
python3 "$HERE/clip_tool.py" impostor bridge 10 >"$NEST/impostor.log" 2>&1 &
wait_for_line "$NEST/impostor.log" "IMPOSTOR owns com.pasta.Launcher.ShellBridge" 10
if "$NEST/bin/pasta-launcher" write "text/plain;charset=utf-8" "$NEST/text.txt" 2>>"$NEST/pasta.log"; then
    echo "FAIL write to an impostor bridge succeeded"
    exit 1
fi
wait_for_line "$NEST/pasta.log" "refusing to write: com.pasta.Launcher.ShellBridge is owned by /usr/bin/python3" 5
refute_line "$NEST/impostor.log" "IMPOSTOR got"
echo "PASS scenario-write"
```

- [ ] **Step 7: Run it**

Run:

```bash
chmod +x spike/gnome-clipboard/*.sh
spike/gnome-clipboard/nested-shell.sh "$PWD/spike/gnome-clipboard/scenario-write.sh"
```

Expected: `POKE rejected: GDBus.Error:org.freedesktop.DBus.Error.AccessDenied: /usr/bin/python3… is not pasta-launcher`, then `PASS scenario-write`.

- [ ] **Step 8: Record the results**

In "Write-back": record that text and image writes round-trip with identical hashes, and the write latency. In "Peer verification": add the two Task 5 refusals, and note that `SHELL_EXE` is hard-coded to `/usr/bin/gnome-shell` (correct for Debian/Ubuntu, Fedora and Arch packaging; not for e.g. NixOS store paths).

- [ ] **Step 9: Commit**

```bash
git add examples/gnome_bridge_spike.rs spike/gnome-clipboard docs/gnome-extension-spike-findings.md
git commit -m "chore(spike): write clipboard through a verified shell bridge and back"
```

---

### Task 6: Full regression run and findings write-up

**Files:**
- Create: `spike/gnome-clipboard/run-all.sh`
- Modify: `docs/gnome-extension-spike-findings.md` ("Event shape", "Implications for the real implementation", "Open items")

**Interfaces:**
- Consumes: every scenario from Tasks 1–5.
- Produces: `run-all.sh` runs each scenario in its own fresh nested shell and exits non-zero if any fails. The completed findings document.

- [ ] **Step 1: Write the runner**

`spike/gnome-clipboard/run-all.sh`:

```sh
#!/bin/sh
# Runs every scenario, each in its own fresh nested shell.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
cargo build --example gnome_bridge_spike --manifest-path "$HERE/../../Cargo.toml" || exit 1
failed=0
for scenario in watch reject-caller offer impostor-pasta write; do
    if "$HERE/nested-shell.sh" "$HERE/scenario-$scenario.sh"; then
        :
    else
        echo "FAILED scenario-$scenario"
        failed=1
    fi
done
exit "$failed"
```

- [ ] **Step 2: Run everything**

Run:

```bash
chmod +x spike/gnome-clipboard/run-all.sh
spike/gnome-clipboard/run-all.sh 2>&1 | tee /tmp/pasta-spike-run.txt | grep -E '^(PASS|FAIL|FAILED)'
RUSTFLAGS="-D warnings" cargo test
```

Expected: five `PASS` lines, no `FAIL`; the full `cargo test` suite still passes.

- [ ] **Step 3: Complete the findings**

Fill in the remaining sections of `docs/gnome-extension-spike-findings.md` from the run output:

- "Event shape": the observed `source=` strings, which events are empty, which are replays, and how many `RECEIVED` lines one copy produced.
- "Implications for the real implementation", at minimum: (a) replays and empty events need handling, either by dropping empty events in the extension and relying on Pasta's existing dedup, or by filtering by source type — list both with what each costs; (b) whether the payload reader needs a deadline, based on whether any `Offer` went without a `RECEIVED`; (c) the log-out requirement after install or upgrade and what the "Enable" button must say when `GetExtensionInfo` returns an empty dict; (d) the limits of executable-name verification found in Tasks 4–5; (e) the plan-level change that Pasta, not the extension, chooses formats.
- "Open items": anything a scenario could not establish. At least: behaviour in the live session with real apps (Nautilus, KeePassXC, a screenshot tool) is untested, because the spike never loads the extension into the live shell.

Write facts as measured; mark anything inferred as inferred. Do not decide go/no-go in the document; list what the results imply and leave the decision to the maintainer.

- [ ] **Step 4: Commit**

```bash
git add spike/gnome-clipboard/run-all.sh docs/gnome-extension-spike-findings.md
git commit -m "docs(spike): record GNOME clipboard extension spike findings"
```
