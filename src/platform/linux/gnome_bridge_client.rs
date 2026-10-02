//! Pasta's client for the GNOME Shell clipboard bridge, plus the identity check
//! the bridge service shares. Depends only on `std` and `zbus` so that
//! `examples/gnome_bridge_write.rs` can compile this exact file.
#![allow(dead_code)] // Temporary: removed once the bridge is wired in (Task 4).

use std::path::Path;

/// The only executable Pasta accepts as the GNOME Shell end of the bridge.
pub(crate) const SHELL_EXE: &str = "/usr/bin/gnome-shell";

/// Largest clipboard payload Pasta sends or accepts over the bridge.
pub(crate) const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

/// Whether `exe` (a `/proc/<pid>/exe` target) is the installed gnome-shell.
/// The kernel appends " (deleted)" when the binary on disk was replaced while
/// the process kept running, which an upgrade does.
pub(crate) fn is_shell_executable(exe: &Path) -> bool {
    let path = exe.to_string_lossy();
    path.strip_suffix(" (deleted)").unwrap_or(&path) == SHELL_EXE
}

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
