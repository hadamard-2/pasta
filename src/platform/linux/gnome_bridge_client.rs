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
