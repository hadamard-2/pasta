# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Pasta is a native, Spotlight-style clipboard manager launcher written in Rust with [GPUI](https://gpui.rs) (no Electron/webview). It supports macOS (Apple Silicon) and Linux (Wayland-first, X11 fallback). Binary name: `pasta-launcher`; the Linux `.deb`/`.rpm` package name is `pasta`.

## Commands

```bash
cargo build --release        # release build (used by install scripts and packaging)
cargo build                  # dev build (opt-level 1; heavy deps like rusqlite/syntect/fastembed/sha2/aes-gcm are opt-level 3 even in dev)
cargo test                   # run the full test suite (unit tests live inline in `#[cfg(test)] mod tests` blocks, no separate tests/ dir)
cargo test <test_name>       # run a single test by name substring
cargo fmt --all -- --check   # formatting check (CI-gated)
cargo clippy --all-targets --no-deps   # lint (CI runs this without -D warnings; ~33 pre-existing cosmetic warnings are tolerated, but don't add new ones)
cargo audit                  # advisory scan (CI job, informational/non-blocking)
```

Linux system build deps (Debian/Ubuntu): `libxkbcommon-dev libfontconfig1-dev libwayland-dev libdbus-1-dev libssl-dev libsecret-1-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-x11-dev libudev-dev`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy, `cargo test` and `cargo build --release` on both macOS and Ubuntu, with `RUSTFLAGS="-D warnings"` for the test/build steps (but not for clippy). Match that locally before considering a change CI-clean.

To smoke-test a UI change manually there's a checklist at `SMOKE_TEST_CHECKLIST.md` — it's the closest thing to a manual QA pass for launcher/tray/search/secrets/editor flows since there's no UI test harness.

## Architecture

### Module layout and shared-state pattern

`src/main.rs` is the entry point *and* the shared prelude: it defines cross-cutting constants (window sizes, timing constants), the `TransformAction` enum + keyboard-shortcut routing, GPUI `actions!` for text-input editing, and the top-level `fn main()` (two `#[cfg(target_os = ...)]` variants — macOS and Linux boot very differently, see below). Every other module does `use crate::*;` to pull these in rather than importing narrowly — that's the established convention here, not an oversight.

- `src/storage.rs` — the persistence and search core. SQLite (via `rusqlite`, bundled) storing AES-256-GCM–encrypted clipboard content, with the encryption key held in the OS keychain (`keyring`) and an ephemeral in-memory fallback (`CryptoBox::ephemeral()`) when the keychain is unavailable — see `ClipboardStorage::bootstrap` vs `bootstrap_fallback`. Also owns the search-query mini-language (`parse_search_query`: `:b <query>` for bowls, `:e <query>` for bowl export, bare `:<query>` for tag-only, otherwise default full-text/semantic), the "Pasta Bowl" tagged-collection model and its YAML export/import, and three search tiers (`SearchExecution::{Fast, Semantic, Neural}` — see below).
- `src/neural_embed.rs` — wraps `fastembed`/`ort` (ONNX Runtime) for on-device embeddings (all-MiniLM-L6-v2). Only initialized when Pasta Brain is enabled (see below).
- `src/transforms/mod.rs` — pure string-transform functions (shell-quote, JSON encode/decode/pretty/minify, URL encode/decode, base64, JWT decode, epoch decode, SHA-256, cert PEM info, QR code) triggered by one-key shortcuts routed through `transform_action_for_shortcut` in `main.rs`.
- `src/emoji.rs` — emoji search/picker data and ranking.
- `src/app/` — the GPUI application layer:
  - `state.rs` — `LauncherView` (the single large view struct holding all UI/editor/search state) and the background search worker (`start_search_worker`, a dedicated thread draining an mpsc channel, coalescing to the latest pending query, and posting `SearchResponse`s back over a `futures` unbounded channel).
  - `actions.rs` — `LauncherView` methods: query handling, search dispatch/debouncing, item actions (copy, delete, reveal secret, tag/bowl/parameter editors, transforms).
  - `query_input.rs` — the custom text-input widget (GPUI has no built-in one); handles selection, IME marked-text, cursor movement.
  - `view.rs` — the `Render` impl / widget tree: results list, detail/preview pane, action bar.
  - `runtime.rs` — wires GPUI globals (`LauncherState`, `AutoClearState`, etc.), spawns the hotkey listener, clipboard watcher, launcher show/hide transition loop, and menu-command dispatch loop.
- `src/platform/` — `mod.rs` re-exports either `linux::*` or `macos::*` under one namespace so shared code just does `use crate::platform::*` and gets the right implementation. macOS and Linux are genuinely separate implementations (tray/menu, hotkey capture, window management, launch-at-login, secret auth), not one codepath with small platform ifs.
- `src/ui/` — palette/theming (`palette.rs`, see `docs/DESIGN_SYSTEM.md`), language detection for syntax highlighting (`language.rs`), `syntect`-based highlighting (`syntax.rs`), and preview text formatting (`preview.rs`).

### Search tiers (`SearchExecution`)

Three progressively more expensive tiers, dispatched with increasing debounce delay as query length/confidence grows (`SEARCH_SEMANTIC_DELAY_MS`, `SEARCH_NEURAL_DELAY_MS`, `SEARCH_NEURAL_MIN_QUERY_CHARS` in `app/actions.rs`): `Fast` (SQLite full-text/fuzzy), `Semantic` (lightweight feature-hash embedding, always available), `Neural` (on-device ONNX embedding via Pasta Brain, opt-in). Query generations are tracked with an `AtomicU64` so a fast-typed newer query invalidates and discards in-flight slower search results rather than racing them onto screen.

### Pasta Brain (neural search) is opt-in and lazy

Disabled by default (`default_pasta_brain_enabled`). The ONNX model (~90MB) is only downloaded and the embedder only initialized (`spawn_neural_init`) when the user has explicitly enabled it — either at startup (if already enabled) or from the tray toggle. Do not make neural init unconditional; it was previously a cause of fan spin-up on every launch. Persisted UI settings (`~/.config/PastaClipboard/ui-style.json` — actual path differs per platform via `dirs::config_dir`) override code-level defaults, so changing a default constant does not affect existing installs.

### Platform divergence: macOS vs Linux `main()`

The two `fn main()` bodies in `main.rs` share the storage-bootstrap and key-binding setup but differ sharply after that — macOS activates as an accessory app and hides immediately after setup (menu-bar-driven); Linux does a single-instance `flock` check first (`acquire_single_instance_lock`), and if another instance holds it, forwards a "show" request over a Unix socket and exits rather than starting a second process. The Linux launcher window itself is destroyed and recreated on every show/hide cycle (not just hidden) — see `docs/linux-platform-notes.md` for why and what that implies for window-creation-time state.

**Read `docs/linux-platform-notes.md` before touching Linux windowing, the global hotkey, blur, or single-instance behavior.** It documents hard-won, non-obvious X11/GPUI/Mutter interactions (window centering races, missing `WM_CLASS`/skip-taskbar handling, KDE-Wayland-only blur, `/dev/input`-based hotkey capture) that are easy to misdiagnose as bugs without that context. Two verified rules from it worth repeating here: never have Pasta set desktop-wide GNOME settings itself (e.g. `center-new-windows`) — document opt-in workarounds instead, and `pkill -f pasta-launcher` will kill the invoking shell too (its pattern matches the full command line) — use `pkill -x pasta-launcher`.

### Secrets

Clipboard items classified as secrets are AES-256-GCM encrypted at rest and masked in the UI until revealed. Reveal and clear-history are gated behind OS auth: Touch ID/keychain on macOS, polkit (password, optionally Howdy face auth) on Linux (`src/platform/linux/polkit.rs`).

## Repository/branching note

`Cargo.toml` lists `repository = "https://github.com/hadamard-2/pasta"` while the README badges point at `yafetgetachew/pasta` — both are the same project under different historical remotes; don't treat the mismatch as something to "fix" without checking with the maintainer first.
