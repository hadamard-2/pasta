//! Pasting into the window that regains focus after the launcher hides:
//! through the GNOME Shell extension on GNOME, through XTest on X11, and not
//! at all on other Wayland desktops.

mod x11;

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
    /// The CLIPBOARD owner right after the GPUI write; the paste waits for
    /// the platform write to replace it.
    X11 {
        owner_before: u32,
    },
}

/// Proof that a paste was prepared before the platform clipboard write,
/// carrying what the platform needs to wait for that write.
pub(crate) struct PasteTicket(Ticket);

fn supported_on(path: ClipboardPath) -> bool {
    matches!(path, ClipboardPath::GnomeExtension | ClipboardPath::X11)
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
        ClipboardPath::X11 => match x11::clipboard_owner() {
            Some(owner_before) => Some(PasteTicket(Ticket::X11 { owner_before })),
            None => {
                eprintln!("warning: paste skipped: could not read the X11 clipboard owner");
                None
            }
        },
        ClipboardPath::Wayland => None,
    }
}

/// Presses the paste shortcut in the window that next holds focus, once it is
/// not one of Pasta's. Never blocks the caller.
pub(crate) fn request_paste(ticket: PasteTicket, _cx: &mut App) {
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
    fn gnome_and_x11_paste_other_wayland_desktops_do_not() {
        assert!(supported_on(ClipboardPath::GnomeExtension));
        assert!(supported_on(ClipboardPath::X11));
        assert!(!supported_on(ClipboardPath::Wayland));
    }
}
