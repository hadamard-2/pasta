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
