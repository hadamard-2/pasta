//! Pasting into the app that becomes frontmost after the launcher hides, by
//! posting Cmd+V through Quartz Event Services, which requires the
//! Accessibility permission.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use cocoa::base::YES;

use crate::*;

const KCG_HID_EVENT_TAP: u32 = 0;
const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
/// Virtual key code of the ANSI-layout V key.
const KVK_ANSI_V: u16 = 0x09;
const POLL_INTERVAL: Duration = Duration::from_millis(16);
/// About 1000 ms at `POLL_INTERVAL`.
const POLL_ATTEMPTS: u32 = 63;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: id) -> u8;
    static kAXTrustedCheckOptionPrompt: id;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        keycode: u16,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// Proof that a paste was prepared before the clipboard write.
pub(crate) struct PasteTicket(());

/// Whether this session can paste on the user's behalf.
pub(crate) fn paste_supported() -> bool {
    true
}

/// Prepares a paste. Without the Accessibility permission, asks for it (once
/// per run, through the system's own dialog) and returns `None`, so the
/// delivery falls back to copy-only.
pub(crate) fn begin_paste() -> Option<PasteTicket> {
    if unsafe { AXIsProcessTrusted() } != 0 {
        return Some(PasteTicket(()));
    }
    static PROMPTED: AtomicBool = AtomicBool::new(false);
    if !PROMPTED.swap(true, Ordering::Relaxed) {
        unsafe {
            let yes: id = msg_send![class!(NSNumber), numberWithBool: YES];
            let options: id = msg_send![class!(NSDictionary),
                dictionaryWithObject: yes
                forKey: kAXTrustedCheckOptionPrompt
            ];
            AXIsProcessTrustedWithOptions(options);
        }
    }
    eprintln!("warning: paste skipped: Pasta lacks the Accessibility permission; copied only");
    None
}

fn frontmost_pid() -> Option<i32> {
    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![workspace, frontmostApplication];
        if app == nil {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        Some(pid)
    }
}

fn post_command_v() {
    unsafe {
        for key_down in [true, false] {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), KVK_ANSI_V, key_down);
            if event.is_null() {
                eprintln!("warning: paste skipped: could not create a key event");
                return;
            }
            CGEventSetFlags(event, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }
}

/// Posts Cmd+V once another app is frontmost, checking on the main thread
/// every `POLL_INTERVAL` for up to about a second. Never blocks the caller.
pub(crate) fn request_paste(_ticket: PasteTicket, cx: &mut App) {
    let our_pid = std::process::id() as i32;
    cx.spawn(async move |cx| {
        for _ in 0..POLL_ATTEMPTS {
            if frontmost_pid().is_some_and(|pid| pid != our_pid) {
                post_command_v();
                return;
            }
            cx.background_executor().timer(POLL_INTERVAL).await;
        }
        eprintln!("warning: paste skipped: no other app became frontmost within 1000 ms");
    })
    .detach();
}
