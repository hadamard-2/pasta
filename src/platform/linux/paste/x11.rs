//! XTest paste for X11 sessions.

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
    Some(
        conn.get_selection_owner(selection)
            .ok()?
            .reply()
            .ok()?
            .owner,
    )
}

/// The first 32-bit value of a window property: `Err` when the request failed
/// (for example the window is gone), `Ok(None)` when the window answered but
/// has no such value.
fn read_u32(
    conn: &impl Connection,
    window: Window,
    property: u32,
    kind: AtomEnum,
) -> Result<Option<u32>, ()> {
    let reply = conn
        .get_property(false, window, property, kind, 0, 1)
        .map_err(drop)?
        .reply()
        .map_err(drop)?;
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn first_u32(conn: &impl Connection, window: Window, property: u32, kind: AtomEnum) -> Option<u32> {
    read_u32(conn, window, property, kind).ok().flatten()
}

/// Whether a `_NET_WM_PID` read shows a window that is not ours. A failed read
/// shows nothing, so it never qualifies; a live window without the property
/// does, since its owner cannot be identified as this process.
fn is_other_process(pid_read: Result<Option<u32>, ()>, our_pid: u32) -> bool {
    matches!(pid_read, Ok(pid) if pid != Some(our_pid))
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
/// there with XTest. The platform clipboard write may complete after it has
/// returned, so the paste waits for the CLIPBOARD owner to change before
/// pressing any keys.
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
    let active =
        intern(&conn, b"_NET_ACTIVE_WINDOW").ok_or("could not intern _NET_ACTIVE_WINDOW")?;
    let pid = intern(&conn, b"_NET_WM_PID").ok_or("could not intern _NET_WM_PID")?;
    let our_pid = std::process::id();

    let deadline = Instant::now() + WAIT_LIMIT;
    let target = loop {
        let owner = selection_owner(&conn, clipboard).unwrap_or(x11rb::NONE);
        let owner_changed = owner != x11rb::NONE && owner != owner_before;
        let focused = first_u32(&conn, root, active, AtomEnum::WINDOW)
            .filter(|&window| window != x11rb::NONE);
        if owner_changed
            && let Some(window) = focused
            && is_other_process(read_u32(&conn, window, pid, AtomEnum::CARDINAL), our_pid)
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
    fn a_failed_pid_read_is_not_proof_of_another_process() {
        assert!(!is_other_process(Err(()), 42));
        assert!(!is_other_process(Ok(Some(42)), 42));
        assert!(is_other_process(Ok(Some(7)), 42));
        assert!(is_other_process(Ok(None), 42));
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
