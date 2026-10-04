//! Test-only: sends a file to the clipboard through the GNOME extension bridge
//! using Pasta's real client code. The isolated harness in gnome-extension/tests
//! runs it under the name `pasta-launcher`, the only caller the bridge serves.
//!
//!     cargo build --example gnome_bridge_write
//!     gnome_bridge_write <mimetype> <file>

#[cfg(target_os = "linux")]
#[allow(dead_code)] // the shared client module also carries paste
#[path = "../src/platform/linux/gnome_bridge_client.rs"]
mod gnome_bridge_client;

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mimetype, file] = args.as_slice() else {
        eprintln!("usage: gnome_bridge_write <mimetype> <file>");
        std::process::exit(2);
    };
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("refused: reading {file}: {err}");
            std::process::exit(1);
        }
    };
    let len = bytes.len();
    match gnome_bridge_client::set_clipboard(mimetype, bytes) {
        Ok(()) => println!("WROTE {mimetype} {len}"),
        Err(err) => {
            eprintln!("refused: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
