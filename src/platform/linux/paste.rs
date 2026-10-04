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
