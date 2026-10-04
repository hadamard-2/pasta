//! Test-only: asks the GNOME extension bridge to paste, using Pasta's real
//! client code. The isolated harness in gnome-extension/tests runs it under
//! the name `pasta-launcher`, the only caller the bridge serves.
//!
//!     cargo build --example gnome_bridge_paste
//!     gnome_bridge_paste [terminal_app_id...]

#[cfg(target_os = "linux")]
#[allow(dead_code)] // the shared client module also carries set_clipboard
#[path = "../src/platform/linux/gnome_bridge_client.rs"]
mod gnome_bridge_client;

#[cfg(target_os = "linux")]
fn main() {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    match gnome_bridge_client::paste(&ids) {
        Ok(()) => println!("PASTED"),
        Err(err) => {
            eprintln!("refused: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
