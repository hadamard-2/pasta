use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Instant;

use gpui::{App, Styled, Window, WindowHandle};
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};
use ksni::{Icon, ToolTip, Tray};
use notify_rust::{Hint, Notification, Timeout};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use rfd::FileDialog;
use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, event_created_child};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::{
    EVT_DATA_OFFER_OPCODE as EXT_DATA_OFFER_OPCODE, Event as ExtDataControlDeviceEvent,
    ExtDataControlDeviceV1,
};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::ExtDataControlOfferV1;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur::OrgKdeKwinBlur;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::{
    EVT_DATA_OFFER_OPCODE as WLR_DATA_OFFER_OPCODE, Event as ZwlrDataControlDeviceEvent,
    ZwlrDataControlDeviceV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::ZwlrDataControlOfferV1;
mod polkit;

use wl_clipboard_rs::copy::{MimeType as CopyMimeType, Options as CopyOptions, Source};
use wl_clipboard_rs::paste::{
    ClipboardType, MimeType as PasteMimeType, Seat, get_contents, get_mime_types_ordered,
};

use crate::storage::ClipboardStorage;
use crate::{
    ABOUT_WINDOW_HEIGHT, ABOUT_WINDOW_WIDTH, AboutWindowState, AutoClearState, LAUNCHER_HEIGHT,
    LAUNCHER_WIDTH, LauncherExitIntent, LauncherView, MENU_COMMAND_TX, MenuCommand, NEURAL_STATUS,
    NeuralStatus, Palette, SelfClipboardWriteState, UiStyleState, palette_for,
};

// ---------------------------------------------------------------------------
// Clipboard (Phase 1)
// ---------------------------------------------------------------------------

/// Snapshot of a clipboard read.
#[derive(Clone, Debug)]
pub(crate) struct ClipboardSnapshot {
    pub text: String,
    pub is_concealed: bool,
    pub is_transient: bool,
}

#[derive(Default)]
struct ClipboardChangeState {
    next_change_count: i64,
    last_signature: Option<String>,
}

enum ClipboardManager {
    Zwlr(ZwlrDataControlManagerV1),
    Ext(ExtDataControlManagerV1),
}

// Variant payloads are owned solely to keep the underlying Wayland proxies
// alive for the duration of the monitor; they're never read back.
#[allow(dead_code)]
enum ClipboardDevice {
    Zwlr(ZwlrDataControlDeviceV1),
    Ext(ExtDataControlDeviceV1),
}

struct WaylandClipboardMonitorState {
    #[allow(dead_code)]
    devices: Vec<ClipboardDevice>,
}

static CLIPBOARD_CHANGE_STATE: OnceLock<Mutex<ClipboardChangeState>> = OnceLock::new();
static WAYLAND_CLIPBOARD_CHANGE_COUNT: AtomicI64 = AtomicI64::new(0);
static WAYLAND_CLIPBOARD_MONITOR_START: OnceLock<()> = OnceLock::new();

pub(crate) fn clipboard_change_count() -> i64 {
    if is_wayland_session() {
        ensure_wayland_clipboard_monitor();
        return WAYLAND_CLIPBOARD_CHANGE_COUNT.load(Ordering::Acquire);
    }

    polling_clipboard_change_count()
}

fn polling_clipboard_change_count() -> i64 {
    let signature = current_clipboard_signature();
    let state = CLIPBOARD_CHANGE_STATE.get_or_init(|| Mutex::new(ClipboardChangeState::default()));
    let mut guard = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.last_signature != signature {
        guard.next_change_count = guard.next_change_count.wrapping_add(1);
        guard.last_signature = signature;
    }
    guard.next_change_count
}

impl ClipboardManager {
    fn get_data_device(
        &self,
        seat: &WlSeat,
        qh: &wayland_client::QueueHandle<WaylandClipboardMonitorState>,
    ) -> ClipboardDevice {
        match self {
            Self::Zwlr(manager) => ClipboardDevice::Zwlr(manager.get_data_device(seat, qh, ())),
            Self::Ext(manager) => ClipboardDevice::Ext(manager.get_data_device(seat, qh, ())),
        }
    }
}

fn ensure_wayland_clipboard_monitor() {
    WAYLAND_CLIPBOARD_MONITOR_START.get_or_init(|| {
        std::thread::Builder::new()
            .name("pasta-linux-clipboard-monitor".to_owned())
            .spawn(move || {
                if let Err(err) = run_wayland_clipboard_monitor() {
                    eprintln!("warning: failed to start Wayland clipboard monitor: {err}");
                }
            })
            .unwrap_or_else(|err| {
                panic!("failed to spawn Wayland clipboard monitor thread: {err}");
            });
    });
}

fn run_wayland_clipboard_monitor() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|err| err.to_string())?;
    let (globals, mut queue) = registry_queue_init::<WaylandClipboardMonitorState>(&conn)
        .map_err(|err| err.to_string())?;
    let qh = queue.handle();

    let manager = globals
        .bind::<ExtDataControlManagerV1, _, _>(&qh, 1..=1, ())
        .ok()
        .map(ClipboardManager::Ext)
        .or_else(|| {
            globals
                .bind::<ZwlrDataControlManagerV1, _, _>(&qh, 1..=1, ())
                .ok()
                .map(ClipboardManager::Zwlr)
        })
        .ok_or_else(|| "missing ext-data-control / wlr-data-control protocol".to_owned())?;

    let registry = globals.registry();
    let seats: Vec<WlSeat> = globals.contents().with_list(|globals| {
        globals
            .iter()
            .filter(|global| global.interface == WlSeat::interface().name && global.version >= 2)
            .map(|global| registry.bind(global.name, 2, &qh, ()))
            .collect()
    });

    if seats.is_empty() {
        return Err("no Wayland seats available for clipboard monitor".to_owned());
    }

    let mut state = WaylandClipboardMonitorState {
        devices: seats
            .iter()
            .map(|seat| manager.get_data_device(seat, &qh))
            .collect(),
    };

    queue.roundtrip(&mut state).map_err(|err| err.to_string())?;
    loop {
        queue
            .blocking_dispatch(&mut state)
            .map_err(|err| err.to_string())?;
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        _event: <WlSeat as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtDataControlManagerV1,
        _event: <ExtDataControlManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrDataControlManagerV1,
        _event: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlOfferV1, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtDataControlOfferV1,
        _event: <ExtDataControlOfferV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrDataControlOfferV1,
        _event: <ZwlrDataControlOfferV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtDataControlDeviceV1,
        event: <ExtDataControlDeviceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            ExtDataControlDeviceEvent::Selection { .. }
            | ExtDataControlDeviceEvent::PrimarySelection { .. } => {
                WAYLAND_CLIPBOARD_CHANGE_COUNT.fetch_add(1, Ordering::AcqRel);
            }
            _ => {}
        }
    }

    event_created_child!(WaylandClipboardMonitorState, ExtDataControlDeviceV1, [
        EXT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for WaylandClipboardMonitorState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrDataControlDeviceV1,
        event: <ZwlrDataControlDeviceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            ZwlrDataControlDeviceEvent::Selection { .. }
            | ZwlrDataControlDeviceEvent::PrimarySelection { .. } => {
                WAYLAND_CLIPBOARD_CHANGE_COUNT.fetch_add(1, Ordering::AcqRel);
            }
            _ => {}
        }
    }

    event_created_child!(WaylandClipboardMonitorState, ZwlrDataControlDeviceV1, [
        WLR_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

/// SHA-256 hash of the given text, used to de-duplicate clipboard items.
pub(crate) fn clipboard_text_hash(value: &str) -> String {
    clipboard_bytes_hash(value.as_bytes())
}

/// SHA-256 hash of raw bytes, used to de-duplicate clipboard items (text or image).
pub(crate) fn clipboard_bytes_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn read_clipboard_snapshot() -> Option<ClipboardSnapshot> {
    let mime_types = read_clipboard_mime_types();
    let text = read_clipboard_text()?;
    Some(ClipboardSnapshot {
        text,
        is_concealed: clipboard_looks_concealed(&mime_types),
        is_transient: clipboard_looks_transient(&mime_types),
    })
}

/// Returns true if we should ignore this clipboard write because we
/// ourselves just wrote it.
pub(crate) fn should_ignore_self_clipboard_write(cx: &mut App, bytes: &[u8]) -> bool {
    let pending = cx
        .try_global::<SelfClipboardWriteState>()
        .and_then(|state| state.pending.clone());
    let Some(pending) = pending else { return false };

    if Instant::now() > pending.due_at {
        cx.global_mut::<SelfClipboardWriteState>().pending = None;
        return false;
    }

    if clipboard_bytes_hash(bytes) == pending.expected_hash {
        cx.global_mut::<SelfClipboardWriteState>().pending = None;
        return true;
    }

    false
}

/// Process secret auto-clear timer.
pub(crate) fn process_secret_autoclear(cx: &mut App) {
    let pending = cx
        .try_global::<AutoClearState>()
        .and_then(|state| state.pending.clone());
    let Some(pending) = pending else { return };
    if Instant::now() < pending.due_at {
        return;
    }

    let should_clear = read_clipboard_text()
        .map(|current| clipboard_text_hash(&current) == pending.expected_hash)
        .unwrap_or(false);
    if should_clear {
        write_clipboard_text("");
    }

    cx.global_mut::<AutoClearState>().pending = None;
}

/// Parse a comma-separated tag input string into a list of tags.
pub(crate) fn parse_custom_tags_input(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub(crate) fn show_macos_notification(title: &str, body: &str) {
    if Notification::new()
        .summary(title)
        .body(body)
        .appname("Pasta")
        .hint(Hint::Transient(true))
        .timeout(Timeout::Milliseconds(2_500))
        .show()
        .is_err()
    {
        eprintln!("[notification] {title}: {body}");
    }
}

pub(crate) fn write_clipboard_text(value: &str) {
    if is_wayland_session() {
        let options = CopyOptions::new();
        if let Err(err) = options.copy(
            Source::Bytes(value.as_bytes().to_vec().into_boxed_slice()),
            CopyMimeType::Text,
        ) {
            eprintln!("warning: failed to copy to Wayland clipboard: {err}");
        }
        return;
    }

    if command_exists("xclip") {
        if let Err(err) = write_via_command("xclip", &["-selection", "clipboard"], value) {
            eprintln!("warning: failed to copy to clipboard with xclip: {err}");
        }
        return;
    }

    if command_exists("xsel") {
        if let Err(err) = write_via_command("xsel", &["--clipboard", "--input"], value) {
            eprintln!("warning: failed to copy to clipboard with xsel: {err}");
        }
        return;
    }

    eprintln!("warning: no supported Linux clipboard backend found");
}

/// Wayland-only counterpart to [`write_clipboard_text`] for image bytes: GPUI's
/// window is destroyed on hide, so a background wl-clipboard-rs writer keeps
/// serving paste requests after that. X11 doesn't need this — GPUI's own X11
/// client already implements `write_to_clipboard` for images natively.
pub(crate) fn write_clipboard_image_bytes(bytes: &[u8], mime_type: &str) {
    if !is_wayland_session() {
        return;
    }

    let options = CopyOptions::new();
    if let Err(err) = options.copy(
        Source::Bytes(bytes.to_vec().into_boxed_slice()),
        CopyMimeType::Specific(mime_type.to_owned()),
    ) {
        eprintln!("warning: failed to copy image to Wayland clipboard: {err}");
    }
}

pub(crate) fn read_clipboard_text() -> Option<String> {
    if is_wayland_session() {
        let (mut pipe, _) = get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            PasteMimeType::Text,
        )
        .ok()?;
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).ok()?;
        return String::from_utf8(bytes).ok();
    }

    if command_exists("xclip") {
        return read_via_command("xclip", &["-selection", "clipboard", "-o"]);
    }

    if command_exists("xsel") {
        return read_via_command("xsel", &["--clipboard", "--output"]);
    }

    None
}

// ---------------------------------------------------------------------------
// File dialogs (Phase 3)
// ---------------------------------------------------------------------------

pub(crate) fn choose_bowl_export_path(_prompt: &str, _default_name: &str) -> Option<PathBuf> {
    let mut path = FileDialog::new()
        .set_title(_prompt)
        .set_file_name(_default_name)
        .add_filter("YAML", &["yaml", "yml"])
        .save_file()?;
    if path.extension().is_none() {
        path.set_extension("yaml");
    }
    Some(path)
}

pub(crate) fn choose_bowl_import_path(_prompt: &str) -> Option<PathBuf> {
    FileDialog::new()
        .set_title(_prompt)
        .add_filter("YAML", &["yaml", "yml"])
        .pick_file()
}

// ---------------------------------------------------------------------------
// Hotkey (Phase 2)
// ---------------------------------------------------------------------------

pub(crate) fn setup_hotkey(_cx: &mut App) {
    // Registration happens in the Linux runtime listener.
}

// ---------------------------------------------------------------------------
// Single-instance guard
// ---------------------------------------------------------------------------

// Holds the flock'd lock file open for the whole process lifetime. Dropping the
// file releases the advisory lock, so it is parked in a static and never freed.
static INSTANCE_LOCK: OnceLock<std::fs::File> = OnceLock::new();

fn instance_lock_path() -> PathBuf {
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    base.join("pasta-launcher.lock")
}

/// Try to become the single running instance by taking an exclusive advisory
/// lock (flock) on a per-user lock file. Returns `true` if we acquired it — we
/// are the instance — and `false` if another instance already holds it. The
/// lock is released automatically by the kernel if the process exits or crashes,
/// so a stale lock never wedges future launches. Fails open (returns `true`) if
/// the lock file itself cannot be opened, so an environment quirk can never
/// block the app from starting.
pub(crate) fn acquire_single_instance_lock() -> bool {
    use nix::fcntl::{FlockArg, flock};
    use std::os::fd::AsRawFd;

    let path = instance_lock_path();
    let file = match std::fs::File::create(&path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("warning: unable to open instance lock '{path:?}': {err}; skipping guard");
            return true;
        }
    };
    match flock(file.as_raw_fd(), FlockArg::LockExclusiveNonblock) {
        Ok(()) => {
            // Keep the file (and its lock) alive for the process lifetime.
            let _ = INSTANCE_LOCK.set(file);
            true
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// External trigger socket
// ---------------------------------------------------------------------------
//
// Lets a second `pasta --show` invocation signal the already-running instance
// to open the launcher, so the launcher can be bound to a desktop-environment
// keyboard shortcut (e.g. a GNOME custom shortcut) without granting the app
// raw /dev/input access for the evdev hotkey listener.

fn trigger_socket_path() -> PathBuf {
    // XDG_RUNTIME_DIR is the correct home for per-user runtime sockets; fall
    // back to the temp dir when it is unset (e.g. minimal login environments).
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    base.join("pasta-launcher.sock")
}

/// Connect to a running Pasta instance and ask it to show the launcher.
/// Returns `true` only when a listening instance accepted the request.
pub(crate) fn send_show_trigger() -> bool {
    match std::os::unix::net::UnixStream::connect(trigger_socket_path()) {
        Ok(mut stream) => stream.write_all(b"show\n").is_ok(),
        Err(_) => false,
    }
}

/// Bind the trigger socket and forward any incoming request to the menu command
/// channel as `ShowLauncher`. Runs for the life of the process.
pub(crate) fn spawn_trigger_listener() {
    let Some(menu_tx) = MENU_COMMAND_TX.get().cloned() else {
        eprintln!("warning: trigger socket unavailable: menu command channel not initialized");
        return;
    };

    let path = trigger_socket_path();
    // A socket file left behind by a previous run makes bind() fail with
    // EADDRINUSE even though nobody is listening; clear it first.
    let _ = std::fs::remove_file(&path);
    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("warning: failed to bind trigger socket '{path:?}': {err}");
            return;
        }
    };

    std::thread::Builder::new()
        .name("pasta-trigger".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Only one command exists today, so any readable payload is a
                // request to show the launcher.
                let mut buf = [0u8; 16];
                if stream.read(&mut buf).is_ok() {
                    let _ = menu_tx.send(MenuCommand::ShowLauncher);
                }
            }
        })
        .ok();
}

// ---------------------------------------------------------------------------
// Autostart (Phase 3) — XDG counterpart to macOS launch_agent
// ---------------------------------------------------------------------------

fn autostart_desktop_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("pasta.desktop"))
}

fn render_autostart_entry() -> String {
    let exe_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "pasta-launcher".to_owned());
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Pasta\n\
         Comment=Clipboard manager for devs and devops\n\
         Exec={exe_path}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

pub(crate) fn launch_agent_is_installed() -> bool {
    autostart_desktop_path().is_some_and(|path| path.exists())
}

/// Called once on startup. If an autostart entry already exists, refresh its
/// Exec= line so app updates keep working. Never create a new entry here —
/// that is reserved for an explicit user opt-in via the tray menu.
pub(crate) fn ensure_launch_agent_registered() {
    let Some(desktop_path) = autostart_desktop_path() else {
        return;
    };
    if !desktop_path.exists() {
        return;
    }
    let entry = render_autostart_entry();
    let should_write = match std::fs::read_to_string(&desktop_path) {
        Ok(existing) => existing != entry,
        Err(_) => true,
    };
    if should_write && let Err(err) = std::fs::write(&desktop_path, entry) {
        eprintln!("warning: unable to refresh autostart entry: {err}");
    }
}

pub(crate) fn install_launch_agent() -> std::io::Result<()> {
    let Some(desktop_path) = autostart_desktop_path() else {
        return Err(std::io::Error::other("config directory unavailable"));
    };
    if let Some(parent) = desktop_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = render_autostart_entry();
    let should_write = match std::fs::read_to_string(&desktop_path) {
        Ok(existing) => existing != entry,
        Err(_) => true,
    };
    if should_write {
        std::fs::write(&desktop_path, entry)?;
    }
    Ok(())
}

pub(crate) fn uninstall_launch_agent() -> std::io::Result<()> {
    let Some(desktop_path) = autostart_desktop_path() else {
        return Ok(());
    };
    match std::fs::remove_file(&desktop_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

// ---------------------------------------------------------------------------
// System tray / menu (Phase 2)
// ---------------------------------------------------------------------------

pub(crate) struct StatusItemRegistration {
    _handle: ksni::blocking::Handle<PastaTray>,
}

impl gpui::Global for StatusItemRegistration {}

struct PastaTray {
    menu_tx: mpsc::Sender<MenuCommand>,
    secret_auto_clear: bool,
    pasta_brain_enabled: bool,
    neural_status: NeuralStatus,
    launch_at_login_enabled: bool,
}

impl PastaTray {
    fn sync_from_app(&mut self, style: &UiStyleState, neural_status: NeuralStatus) {
        self.secret_auto_clear = style.secret_auto_clear;
        self.pasta_brain_enabled = style.pasta_brain_enabled;
        self.neural_status = neural_status;
        self.launch_at_login_enabled = launch_agent_is_installed();
    }
}

impl Tray for PastaTray {
    fn id(&self) -> String {
        "pasta".into()
    }

    fn title(&self) -> String {
        "Pasta".into()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![pasta_tray_icon()]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![pasta_tray_icon()],
            title: "Pasta".into(),
            description: "Clipboard manager for devs and devops".into(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.menu_tx.send(MenuCommand::ShowLauncher);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();

        items.push(
            StandardItem {
                label: "Pasta".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.menu_tx.send(MenuCommand::ShowLauncher);
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);

        items.push(
            StandardItem {
                label: "About".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.menu_tx.send(MenuCommand::ShowAbout);
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);

        items.push(
            SubMenu {
                label: "Preferences".into(),
                submenu: vec![
                    SubMenu {
                        label: "Secret Auto-Clear".into(),
                        submenu: vec![
                            CheckmarkItem {
                                label: "Enable (30s)".into(),
                                checked: self.secret_auto_clear,
                                activate: Box::new(|tray: &mut Self| {
                                    tray.secret_auto_clear = true;
                                    let _ =
                                        tray.menu_tx.send(MenuCommand::SetSecretAutoClear(true));
                                }),
                                ..Default::default()
                            }
                            .into(),
                            CheckmarkItem {
                                label: "Disable".into(),
                                checked: !self.secret_auto_clear,
                                activate: Box::new(|tray: &mut Self| {
                                    tray.secret_auto_clear = false;
                                    let _ =
                                        tray.menu_tx.send(MenuCommand::SetSecretAutoClear(false));
                                }),
                                ..Default::default()
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                    SubMenu {
                        label: "Pasta Brain".into(),
                        submenu: vec![
                            CheckmarkItem {
                                label: "Enable".into(),
                                checked: self.pasta_brain_enabled,
                                activate: Box::new(|tray: &mut Self| {
                                    tray.pasta_brain_enabled = true;
                                    let _ = tray.menu_tx.send(MenuCommand::SetPastaBrain(true));
                                }),
                                ..Default::default()
                            }
                            .into(),
                            CheckmarkItem {
                                label: "Disable".into(),
                                checked: !self.pasta_brain_enabled,
                                activate: Box::new(|tray: &mut Self| {
                                    tray.pasta_brain_enabled = false;
                                    let _ = tray.menu_tx.send(MenuCommand::SetPastaBrain(false));
                                }),
                                ..Default::default()
                            }
                            .into(),
                            MenuItem::Separator,
                            StandardItem {
                                label: neural_download_label(self.neural_status).into(),
                                activate: Box::new(|tray: &mut Self| {
                                    let _ = tray.menu_tx.send(MenuCommand::DownloadBrain);
                                }),
                                ..Default::default()
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        items.push(
            CheckmarkItem {
                label: "Launch at Login".into(),
                checked: self.launch_at_login_enabled,
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.menu_tx.send(MenuCommand::ToggleLaunchAtLogin);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            StandardItem {
                label: "Clear Clipboard History…".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.menu_tx.send(MenuCommand::RequestClearHistory);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.menu_tx.send(MenuCommand::QuitApp);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        eprintln!("warning: Linux status item unavailable, continuing without tray: {reason:?}");
        true
    }
}

/// Configure the app as a background/accessory process. No-op on Linux.
pub(crate) fn configure_background_mode() {
    // On macOS this sets NSApplicationActivationPolicyAccessory.
    // On Linux, background mode is the default — no action needed.
}

pub(crate) fn setup_status_item(cx: &mut App) {
    let Some(menu_tx) = MENU_COMMAND_TX.get().cloned() else {
        eprintln!("warning: status item unavailable: menu command channel not initialized");
        return;
    };

    let style = cx.global::<UiStyleState>().clone();
    let neural_status = NEURAL_STATUS
        .lock()
        .map(|status| *status)
        .unwrap_or(NeuralStatus::Failed);

    let tray = PastaTray {
        menu_tx,
        secret_auto_clear: style.secret_auto_clear,
        pasta_brain_enabled: style.pasta_brain_enabled,
        neural_status,
        launch_at_login_enabled: launch_agent_is_installed(),
    };

    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => {
            eprintln!("info: Linux status item initialized");
            cx.set_global(StatusItemRegistration { _handle: handle });
        }
        Err(err) => {
            eprintln!("warning: failed to initialize Linux status item: {err:?}");
        }
    }
}

/// Update the brain menu item state. Stub is a no-op.
pub(crate) fn update_brain_menu_state(cx: &App) {
    let Some(registration) = cx.try_global::<StatusItemRegistration>() else {
        return;
    };

    let style = cx.global::<UiStyleState>().clone();
    let neural_status = NEURAL_STATUS
        .lock()
        .map(|status| *status)
        .unwrap_or(NeuralStatus::Failed);

    let _ = registration._handle.update(|tray| {
        tray.sync_from_app(&style, neural_status);
    });
}

pub(crate) fn update_launch_at_login_menu_state(cx: &App) {
    let Some(registration) = cx.try_global::<StatusItemRegistration>() else {
        return;
    };
    let installed = launch_agent_is_installed();
    let _ = registration._handle.update(|tray| {
        tray.launch_at_login_enabled = installed;
    });
}

// The ksni tray rebuilds its menu from UiStyleState on every sync, so the
// macOS-granular per-item updaters collapse to a single full refresh here.
pub(crate) fn update_secret_menu_state(cx: &App) {
    update_brain_menu_state(cx);
}

/// Map a menu tag integer to a MenuCommand. Stub for tests.
#[cfg(test)]
pub(crate) fn menu_command_from_tag(tag: isize) -> Option<crate::MenuCommand> {
    use crate::*;
    match tag {
        MENU_TAG_SHOW => Some(MenuCommand::ShowLauncher),
        MENU_TAG_QUIT => Some(MenuCommand::QuitApp),
        MENU_TAG_ABOUT => Some(MenuCommand::ShowAbout),
        MENU_TAG_SECRET_CLEAR_ON => Some(MenuCommand::SetSecretAutoClear(true)),
        MENU_TAG_SECRET_CLEAR_OFF => Some(MenuCommand::SetSecretAutoClear(false)),
        MENU_TAG_BRAIN_ON => Some(MenuCommand::SetPastaBrain(true)),
        MENU_TAG_BRAIN_OFF => Some(MenuCommand::SetPastaBrain(false)),
        MENU_TAG_BRAIN_DOWNLOAD => Some(MenuCommand::DownloadBrain),
        MENU_TAG_CLEAR_HISTORY => Some(MenuCommand::RequestClearHistory),
        MENU_TAG_LAUNCH_AT_LOGIN => Some(MenuCommand::ToggleLaunchAtLogin),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Style & Fonts (Phase 4)
// ---------------------------------------------------------------------------

fn ui_style_state_path() -> Option<PathBuf> {
    let base = dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .or_else(dirs::home_dir)?;
    let directory = base.join("PastaClipboard");
    if let Err(err) = std::fs::create_dir_all(&directory) {
        eprintln!("warning: unable to create config directory '{directory:?}': {err}");
        return None;
    }
    Some(directory.join("ui-style.json"))
}

fn default_ui_style_state(
    ui_font_family: gpui::SharedString,
    content_font_family: gpui::SharedString,
) -> UiStyleState {
    UiStyleState {
        ui_font_family,
        content_font_family,
        surface_alpha: 1.00,
        secret_auto_clear: true,
        pasta_brain_enabled: false,
    }
}

fn load_ui_style_state(
    ui_font_family: gpui::SharedString,
    content_font_family: gpui::SharedString,
) -> UiStyleState {
    let mut style = default_ui_style_state(ui_font_family, content_font_family);
    let Some(path) = ui_style_state_path() else {
        return style;
    };

    let data = match std::fs::read_to_string(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return style,
        Err(err) => {
            eprintln!("warning: unable to read style settings from '{path:?}': {err}");
            return style;
        }
    };

    let persisted: crate::PersistedUiStyleState = match serde_json::from_str(&data) {
        Ok(persisted) => persisted,
        Err(err) => {
            eprintln!("warning: unable to parse style settings from '{path:?}': {err}");
            return style;
        }
    };

    style.surface_alpha = persisted.surface_alpha.clamp(0.45, 1.0);
    style.secret_auto_clear = persisted.secret_auto_clear;
    style.pasta_brain_enabled = persisted.pasta_brain_enabled;
    style
}

fn save_ui_style_state(style: &UiStyleState) {
    let Some(path) = ui_style_state_path() else {
        return;
    };

    let serialized = match serde_json::to_string_pretty(&crate::PersistedUiStyleState {
        surface_alpha: style.surface_alpha.clamp(0.45, 1.0),
        secret_auto_clear: style.secret_auto_clear,
        pasta_brain_enabled: style.pasta_brain_enabled,
    }) {
        Ok(serialized) => serialized,
        Err(err) => {
            eprintln!("warning: unable to serialize style settings: {err}");
            return;
        }
    };

    if let Err(err) = std::fs::write(&path, serialized) {
        eprintln!("warning: unable to write style settings to '{path:?}': {err}");
    }
}

pub(crate) fn persist_ui_style_state(cx: &App) {
    save_ui_style_state(cx.global::<UiStyleState>());
}

pub(crate) fn load_embedded_ui_font(cx: &mut App) {
    use std::borrow::Cow;

    // Pasta is Geist-only by design: Geist Sans for UI chrome, Geist Mono for
    // clipboard content. No other font is bundled or selectable.
    let font_blobs: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Geist-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf").as_slice()),
    ];

    if let Err(err) = cx.text_system().add_fonts(font_blobs) {
        eprintln!("warning: unable to load embedded fonts: {err}");
    }

    let ui_font_family =
        resolve_font_family(cx, &["Geist"]).unwrap_or_else(|| "Geist".into());
    let content_font_family = resolve_font_family(cx, &["Geist Mono", "GeistMono"])
        .unwrap_or_else(|| "Geist Mono".into());
    cx.set_global(load_ui_style_state(ui_font_family, content_font_family));
}

/// Resolve a bundled font's family name via best-effort matching against the
/// text system's registered fonts.
pub(crate) fn resolve_font_family(cx: &App, candidates: &[&str]) -> Option<gpui::SharedString> {
    let all_names = cx.text_system().all_font_names();
    let all_normalized: Vec<String> = all_names
        .iter()
        .map(|name| normalize_font_name(name))
        .collect();

    for candidate in candidates {
        let candidate_normalized = normalize_font_name(candidate);
        if candidate_normalized.is_empty() {
            continue;
        }

        if let Some((ix, _)) = all_normalized
            .iter()
            .enumerate()
            .find(|(_, name)| *name == &candidate_normalized)
        {
            return Some(all_names[ix].clone().into());
        }
    }

    for candidate in candidates {
        let candidate_normalized = normalize_font_name(candidate);
        if candidate_normalized.is_empty() {
            continue;
        }

        if let Some((ix, _)) = all_normalized.iter().enumerate().find(|(_, name)| {
            name.contains(&candidate_normalized) || candidate_normalized.contains(*name)
        }) {
            return Some(all_names[ix].clone().into());
        }
    }

    None
}

fn normalize_font_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

// ---------------------------------------------------------------------------
// Touch ID / Auth (Phase 3)
// ---------------------------------------------------------------------------

/// Authenticate the user. On Linux this goes through polkit; the system's
/// polkit agent shows the prompt and routes through PAM, which picks up
/// howdy or fingerprint modules when the distro has them configured.
pub(crate) fn authenticate_with_touch_id(reason: &str) -> bool {
    polkit::authenticate(polkit::ACTION_REVEAL_SECRET, reason)
}

// ---------------------------------------------------------------------------
// Window (Phase 4)
// ---------------------------------------------------------------------------

pub(crate) struct BackgroundAnchorView;

impl gpui::Render for BackgroundAnchorView {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div().size_full()
    }
}

pub(crate) fn create_background_anchor_window(
    cx: &mut App,
) -> Option<WindowHandle<BackgroundAnchorView>> {
    use gpui::*;

    let display_id = cx.primary_display().map(|display| display.id());
    let bounds = Bounds::centered(display_id, size(px(1.0), px(1.0)), cx);

    match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            window_background: WindowBackgroundAppearance::Transparent,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_decorations: Some(WindowDecorations::Client),
            display_id,
            ..Default::default()
        },
        |_window, cx| cx.new(|_| BackgroundAnchorView),
    ) {
        Ok(window) => {
            eprintln!("info: Linux background anchor window created");
            Some(window)
        }
        Err(err) => {
            eprintln!("warning: failed to create Linux background anchor window: {err}");
            None
        }
    }
}

/// Create the main launcher window. Stub creates a basic GPUI window.
pub(crate) fn create_launcher_window(cx: &mut App) -> Option<WindowHandle<LauncherView>> {
    use gpui::*;

    let display_id = cx.primary_display().map(|display| display.id());
    let bounds = Bounds::centered(
        display_id,
        size(px(LAUNCHER_WIDTH), px(LAUNCHER_HEIGHT)),
        cx,
    );
    let storage = cx.global::<crate::StorageState>().storage.clone();
    let style = cx.global::<UiStyleState>().clone();
    let (search_tx, search_rx, generation_token) =
        crate::app::state::start_search_worker(storage.clone());

    let window = match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: false,
            kind: WindowKind::Normal,
            window_background: WindowBackgroundAppearance::Transparent,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_decorations: Some(WindowDecorations::Client),
            display_id,
            ..Default::default()
        },
        move |window, cx| {
            let storage = storage.clone();
            let style = style.clone();
            let search_tx = search_tx.clone();
            let generation_token = generation_token.clone();

            // GPUI's X11 backend requests a centered position but never marks it
            // user-specified, so Mutter ignores it and runs its own placement
            // (often top-left). Position the still-unmapped window on the primary
            // monitor and set USPosition so the window manager honors it.
            center_window_on_primary(window, LAUNCHER_WIDTH, LAUNCHER_HEIGHT);

            window.on_window_should_close(cx, |_, cx| {
                cx.hide();
                false
            });

            cx.new(move |cx| {
                let view = LauncherView::new(
                    storage,
                    style.ui_font_family.clone(),
                    style.content_font_family.clone(),
                    style.surface_alpha,
                    style.pasta_brain_enabled,
                    search_tx,
                    generation_token,
                    cx,
                );

                try_apply_kde_wayland_blur(window);

                cx.observe_window_activation(window, |view: &mut LauncherView, window, cx| {
                    view.note_window_activation(window.is_window_active());
                    cx.notify();
                })
                .detach();

                cx.observe_window_appearance(window, |_view: &mut LauncherView, _window, cx| {
                    cx.notify();
                })
                .detach();

                cx.observe_keystrokes(|view: &mut LauncherView, event, _window, cx| {
                    view.handle_keystroke(event, cx);
                })
                .detach();

                view
            })
        },
    ) {
        Ok(window) => {
            eprintln!("info: Linux launcher window created");
            window
        }
        Err(err) => {
            eprintln!("warning: failed to open Linux launcher window: {err}");
            return None;
        }
    };

    crate::app::spawn_search_result_listener(cx, window, search_rx);
    Some(window)
}

const ABOUT_GITHUB_URL: &str = "https://github.com/yafetgetachew/pasta";

/// Static "About" panel. Replaces the old kdialog/zenity/rfd dialog
/// chain, which — being windows the WM placed itself rather than ones we
/// created and centered — inherited the same off-center Mutter placement bug
/// the launcher had before `center_window_on_primary`.
pub(crate) struct AboutView;

impl gpui::Render for AboutView {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::*;

        let style = cx.global::<UiStyleState>().clone();
        let palette = palette_for(style.surface_alpha);
        let version = env!("CARGO_PKG_VERSION");

        div()
            .size_full()
            .bg(palette.window_bg)
            .font_family(style.ui_font_family.clone())
            .font_weight(FontWeight::NORMAL)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .px_6()
            .child(
                div()
                    .text_size(px(20.0))
                    .text_color(palette.title_text)
                    .child("Pasta"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted_text)
                    .child(format!("v{version}")),
            )
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(palette.row_text)
                    .text_center()
                    .child("The clipboard manager for devs and devops."),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted_text)
                    .text_center()
                    .child("Blazing-fast, Spotlight-style clipboard launcher built with Rust and GPUI."),
            )
            .child(
                div()
                    .id("about-github-link")
                    .mt_2()
                    .text_xs()
                    .text_color(palette.query_active)
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _, _, _| {
                        let _ = std::process::Command::new("xdg-open")
                            .arg(ABOUT_GITHUB_URL)
                            .spawn();
                    }))
                    .child("github.com/yafetgetachew/pasta"),
            )
            .child(
                div()
                    .id("about-close")
                    .mt_4()
                    .text_xs()
                    .text_color(palette.keycap_text)
                    .bg(palette.keycap_bg)
                    .rounded(px(4.0))
                    .px(px(10.0))
                    .py(px(4.0))
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _, window, _| {
                        window.remove_window();
                    }))
                    .child("Close"),
            )
    }
}

fn create_about_window(cx: &mut App) -> Option<WindowHandle<AboutView>> {
    use gpui::*;

    let display_id = cx.primary_display().map(|display| display.id());
    let bounds = Bounds::centered(
        display_id,
        size(px(ABOUT_WINDOW_WIDTH), px(ABOUT_WINDOW_HEIGHT)),
        cx,
    );

    match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("About".into()),
                ..Default::default()
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            is_resizable: false,
            is_minimizable: false,
            window_decorations: Some(WindowDecorations::Server),
            display_id,
            ..Default::default()
        },
        move |window, cx| {
            // Same Mutter placement bug as the launcher: an unmarked "centered"
            // request is advisory only, so pin the position ourselves.
            center_window_on_primary(window, ABOUT_WINDOW_WIDTH, ABOUT_WINDOW_HEIGHT);
            cx.new(|_| AboutView)
        },
    ) {
        Ok(window) => Some(window),
        Err(err) => {
            eprintln!("warning: failed to open About window: {err}");
            None
        }
    }
}

/// Show the About window, reusing the existing one (bringing it to front)
/// rather than opening a second copy if it's already open.
pub(crate) fn show_about_window(cx: &mut App) {
    if let Some(window) = cx
        .try_global::<AboutWindowState>()
        .and_then(|state| state.window)
        && window
            .update(cx, |_, window, _cx| window.activate_window())
            .is_ok()
    {
        return;
    }

    if let Some(created) = create_about_window(cx) {
        cx.global_mut::<AboutWindowState>().window = Some(created);
    }
}

/// Set the window to move to the active workspace/space. No-op on Wayland.
pub(crate) fn set_window_move_to_active_space(_window: &Window) {
    // On Wayland, the compositor controls workspace placement.
    // Hyprland window rules handle this via config, not code.
}

fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[derive(Default)]
struct KdeBlurState;

impl Dispatch<WlRegistry, GlobalListContents> for KdeBlurState {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

wayland_client::delegate_noop!(KdeBlurState: ignore WlCompositor);
wayland_client::delegate_noop!(KdeBlurState: ignore WlSurface);
wayland_client::delegate_noop!(KdeBlurState: ignore WlRegion);
wayland_client::delegate_noop!(KdeBlurState: ignore OrgKdeKwinBlurManager);
wayland_client::delegate_noop!(KdeBlurState: ignore OrgKdeKwinBlur);

fn try_apply_kde_wayland_blur(window: &Window) {
    if !is_wayland_session() {
        return;
    }

    let Ok(window_handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Wayland(raw_window) = window_handle.as_raw() else {
        return;
    };
    let surface_ptr = raw_window.surface.as_ptr();
    if surface_ptr.is_null() {
        return;
    }

    let Ok(display_handle) = HasDisplayHandle::display_handle(window) else {
        return;
    };
    let RawDisplayHandle::Wayland(raw_display) = display_handle.as_raw() else {
        return;
    };
    let display_ptr = raw_display.display.as_ptr();
    if display_ptr.is_null() {
        return;
    }

    // No surface-pointer dedup: the Wayland client recycles pointers across
    // window lifetimes, and KWin handles duplicate blur proxies idempotently.
    let backend =
        unsafe { wayland_client::backend::Backend::from_foreign_display(display_ptr.cast()) };
    let conn = Connection::from_backend(backend);
    let (globals, mut event_queue) = match registry_queue_init::<KdeBlurState>(&conn) {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("warning: failed to init Wayland globals for KWin blur: {err}");
            return;
        }
    };
    let qh = event_queue.handle();
    let compositor: WlCompositor = match globals.bind(&qh, 1..=6, ()) {
        Ok(proxy) => proxy,
        Err(_) => return,
    };
    let blur_manager: OrgKdeKwinBlurManager = match globals.bind(&qh, 1..=1, ()) {
        Ok(proxy) => proxy,
        Err(_) => return,
    };
    let surface_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface_ptr.cast()) };
    let surface = match surface_id.and_then(|id| WlSurface::from_id(&conn, id)) {
        Ok(surface) => surface,
        Err(err) => {
            eprintln!("warning: failed to access Wayland surface for blur: {err}");
            return;
        }
    };

    let region = compositor.create_region(&qh, ());
    add_rounded_blur_region(&region, LAUNCHER_WIDTH as i32, LAUNCHER_HEIGHT as i32, 22);

    let blur = blur_manager.create(&surface, &qh, ());
    blur.set_region(Some(&region));
    blur.commit();
    surface.commit();
    let _ = event_queue.roundtrip(&mut KdeBlurState);
}

fn add_rounded_blur_region(region: &WlRegion, width: i32, height: i32, radius: i32) {
    let radius = radius.clamp(0, width.min(height) / 2);
    if radius == 0 {
        region.add(0, 0, width, height);
        return;
    }

    // Center body.
    region.add(radius, 0, width - radius * 2, height);
    region.add(0, radius, width, height - radius * 2);

    // Approximate rounded corners with horizontal scanlines.
    for y in 0..radius {
        let dy = (radius - y) as f32 - 0.5;
        let inset =
            (radius as f32 - (radius as f32 * radius as f32 - dy * dy).sqrt()).floor() as i32;
        let span = width - inset * 2;
        if span > 0 {
            region.add(inset, y, span, 1);
            region.add(inset, height - y - 1, span, 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Window centering (X11)
// ---------------------------------------------------------------------------
//
// GPUI's X11 backend models every monitor as one combined screen and requests a
// centered position, but it never sets the ICCCM `USPosition` flag. GNOME's
// Mutter therefore treats the position as advisory and runs its own placement,
// which lands the window near the top-left (and, on multi-monitor, the "center"
// is the midpoint of the whole desktop rather than one screen). We correct both
// by positioning the still-unmapped window on the primary RandR monitor and
// marking the position user-specified so the window manager honors it.

/// Primary monitor rectangle (x, y, width, height) in device pixels via RandR.
/// Returns `None` if RandR has no primary output or the query fails.
fn primary_monitor_rect(conn: &impl x11rb::connection::Connection, root: u32) -> Option<(i32, i32, i32, i32)> {
    use x11rb::protocol::randr::ConnectionExt as _;

    let primary = conn.randr_get_output_primary(root).ok()?.reply().ok()?.output;
    let resources = conn
        .randr_get_screen_resources_current(root)
        .ok()?
        .reply()
        .ok()?;
    let output = conn
        .randr_get_output_info(primary, resources.config_timestamp)
        .ok()?
        .reply()
        .ok()?;
    if output.crtc == 0 {
        return None;
    }
    let crtc = conn
        .randr_get_crtc_info(output.crtc, resources.config_timestamp)
        .ok()?
        .reply()
        .ok()?;
    Some((
        crtc.x as i32,
        crtc.y as i32,
        crtc.width as i32,
        crtc.height as i32,
    ))
}

/// Center a still-unmapped window on the primary monitor and pin the position
/// so the window manager does not re-place it. No-op on Wayland (the
/// compositor owns window placement there) and best-effort on X11 — any
/// failure leaves GPUI's default behavior untouched. `want_width`/`want_height`
/// must match the window's logical size, since that's how it's located below.
///
/// GPUI 0.2.2's X11 backend leaves `HasWindowHandle` unimplemented, so we cannot
/// ask it for the window id. Instead we locate the freshly-created, still
/// unmapped window on the X server by matching GPUI's `_NET_WM_PID` stamp (our
/// process) and its device size (which distinguishes it from any other window
/// — e.g. the 1x1 background-anchor window — that shares our PID).
/// Find our freshly-created launcher window on the X server by matching GPUI's
/// `_NET_WM_PID` stamp and the window's device size. The size check is what
/// distinguishes it from the other windows sharing our PID (the 1x1
/// background anchor, GPUI's 2x2 notification window).
fn find_launcher_x_window(
    conn: &impl x11rb::connection::Connection,
    root: u32,
    pid_atom: u32,
    our_pid: u32,
    want_w: i32,
    want_h: i32,
) -> Option<u32> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let children = conn
        .query_tree(root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())?
        .children;

    children.into_iter().find(|&child| {
        let Some(geometry) = conn
            .get_geometry(child)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return false;
        };
        // Tolerate off-by-one from fractional-scale rounding.
        if (geometry.width as i32 - want_w).abs() > 2
            || (geometry.height as i32 - want_h).abs() > 2
        {
            return false;
        }
        let pid = conn
            .get_property(false, child, pid_atom, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().and_then(|mut values| values.next()));
        pid == Some(our_pid)
    })
}

/// Keep the launcher out of the dock, the window switcher and the pager.
///
/// GPUI creates the launcher as a plain window: no `_NET_WM_WINDOW_TYPE`, so
/// window managers treat it as `_NET_WM_WINDOW_TYPE_NORMAL` and list it as a
/// running app — on GNOME that means a dock entry, with a generic icon since
/// the window also carries no `WM_CLASS`. `_NET_WM_STATE_SKIP_TASKBAR` and
/// `_NET_WM_STATE_SKIP_PAGER` are the EWMH way to opt out of that while staying
/// an ordinary focusable window; this is the Linux counterpart to
/// `NSApplicationActivationPolicyAccessory` on macOS.
///
/// Set before the window is mapped, so the window manager reads it as part of
/// the initial map rather than us having to ask it to change state afterwards.
/// Deliberately *not* done by switching GPUI to `WindowKind::PopUp`: that sets
/// `_NET_WM_WINDOW_TYPE_NOTIFICATION`, which also forces always-on-top stacking
/// and lets the window manager refuse keyboard focus — fatal for a launcher
/// whose whole job is to be typed into.
fn set_skip_taskbar_and_pager(conn: &impl x11rb::connection::Connection, root: u32, win: u32) {
    // intern_atom/send_event come from the xproto extension trait,
    // change_property32 from the wrapper one; both are needed here.
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode,
    };
    use x11rb::wrapper::ConnectionExt as _;

    let atom = |name: &[u8]| {
        conn.intern_atom(false, name)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.atom)
            .filter(|&atom| atom != 0)
    };

    let (Some(state), Some(skip_taskbar), Some(skip_pager)) = (
        atom(b"_NET_WM_STATE"),
        atom(b"_NET_WM_STATE_SKIP_TASKBAR"),
        atom(b"_NET_WM_STATE_SKIP_PAGER"),
    ) else {
        return;
    };

    // Two routes, because we cannot be sure which side of the map we are on.
    // Writing the property directly is what the window manager reads at map
    // time, but it is ignored once the window is already mapped — and the
    // retrying lookup above can easily take us past that point. For the mapped
    // case EWMH requires asking the window manager to change the state via a
    // _NET_WM_STATE client message to the root. Doing both is harmless: the
    // client message is dropped for an unmapped window, and the property write
    // is redundant (not conflicting) once the message has been honoured.
    //
    // REPLACE is safe: the window is new, so nothing else has put state on it
    // yet (the window manager adds its own, e.g. _FOCUSED, later).
    let _ = conn.change_property32(
        PropMode::REPLACE,
        win,
        state,
        AtomEnum::ATOM,
        &[skip_taskbar, skip_pager],
    );

    // data = [action, first property, second property, source indication, 0];
    // action 1 = _NET_WM_STATE_ADD, source 1 = normal application.
    let message = ClientMessageEvent::new(32, win, state, [1, skip_taskbar, skip_pager, 1, 0]);
    let _ = conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        message,
    );
}

fn center_window_on_primary(window: &Window, want_width: f32, want_height: f32) {
    use x11rb::connection::Connection;
    use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
    use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt as _};

    if is_wayland_session() {
        return;
    }

    let scale = window.scale_factor();
    let want_w = (want_width * scale).round() as i32;
    let want_h = (want_height * scale).round() as i32;

    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("warning: X11 connect for window centering failed: {err}");
            return;
        }
    };
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let pid_atom = match conn
        .intern_atom(false, b"_NET_WM_PID")
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    {
        Some(reply) if reply.atom != 0 => reply.atom,
        _ => return,
    };
    let our_pid = std::process::id();

    // The lookup runs on our own X11 connection, which has no ordering
    // guarantee against the one GPUI created the window on: sometimes our
    // query_tree beats GPUI's CreateWindow (or its final resize) to the server
    // and finds nothing. Retry briefly instead of giving up on the first miss —
    // losing this race means both an off-centre window and a stray dock entry.
    let mut launcher = None;
    for attempt in 0..12 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        launcher = find_launcher_x_window(&conn, root, pid_atom, our_pid, want_w, want_h);
        if launcher.is_some() {
            break;
        }
    }

    let Some(win) = launcher else {
        return;
    };

    set_skip_taskbar_and_pager(&conn, root, win);

    let (mx, my, mw, mh) = primary_monitor_rect(&conn, root).unwrap_or((
        0,
        0,
        screen.width_in_pixels as i32,
        screen.height_in_pixels as i32,
    ));

    let x = mx + (mw - want_w) / 2;
    let y = my + (mh - want_h) / 2;

    let _ = conn.configure_window(win, &ConfigureWindowAux::new().x(x).y(y));
    let hints = WmSizeHints {
        position: Some((WmSizeHintsSpecification::UserSpecified, x, y)),
        ..WmSizeHints::default()
    };
    let _ = hints.set_normal_hints(&conn, win);
    let _ = conn.flush();
}

fn current_clipboard_signature() -> Option<String> {
    let mime_types = read_clipboard_mime_types();
    let mime_signature = if mime_types.is_empty() {
        "mime:none".to_owned()
    } else {
        format!("mime:{}", mime_types.join("|"))
    };

    let text_signature = read_clipboard_text()
        .map(|text| format!("text:{}", clipboard_text_hash(&text)))
        .unwrap_or_else(|| "text:none".to_owned());

    Some(format!("{mime_signature};{text_signature}"))
}

fn read_clipboard_mime_types() -> Vec<String> {
    if is_wayland_session() {
        return get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified)
            .unwrap_or_default();
    }

    if command_exists("xclip") {
        return read_via_command("xclip", &["-selection", "clipboard", "-t", "TARGETS", "-o"])
            .map(|output| {
                output
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
    }

    Vec::new()
}

fn clipboard_looks_concealed(mime_types: &[String]) -> bool {
    mime_types.iter().any(|mime| {
        let lowered = mime.to_ascii_lowercase();
        lowered.contains("concealed")
            || lowered.contains("secret")
            || lowered.contains("password")
            || lowered.contains("onepassword")
            || lowered.contains("bitwarden")
            || lowered.contains("keepass")
    })
}

fn clipboard_looks_transient(mime_types: &[String]) -> bool {
    mime_types.iter().any(|mime| {
        let lowered = mime.to_ascii_lowercase();
        lowered.contains("transient")
            || lowered.contains("x-kde-passwordmanagerhint")
            || lowered.contains("application/x-gtk-text-buffer-rich-text")
    })
}

fn neural_download_label(status: NeuralStatus) -> &'static str {
    match status {
        NeuralStatus::Loading => "Downloading Model...",
        NeuralStatus::Ready => "Model Ready",
        NeuralStatus::Failed => "Download Model (Retry)",
    }
}

fn pasta_tray_icon() -> Icon {
    // Lucide "clipboard-pen" glyph, pre-rendered to a 32x32 ARGB32 (network
    // byte order) bitmap so the tray icon matches the app/desktop icon
    // without pulling in an SVG rasterizer at runtime.
    const WIDTH: i32 = 32;
    const HEIGHT: i32 = 32;
    const BYTES: &[u8] = include_bytes!("../../../assets/tray/pasta-tray-32.argb");

    Icon {
        width: WIDTH,
        height: HEIGHT,
        data: BYTES.to_vec(),
    }
}

fn command_exists(program: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {program} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn read_via_command(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn write_via_command(program: &str, args: &[&str], value: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "missing stdin pipe".to_owned())?;
    stdin
        .write_all(value.as_bytes())
        .map_err(|err| err.to_string())?;
    drop(stdin);

    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with status {status}"))
    }
}
