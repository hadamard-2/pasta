//! Spike: Pasta's side of a clipboard bridge to GNOME Shell. Throwaway; see
//! docs/gnome-extension-spike-findings.md.
//!
//!     cargo build --example gnome_bridge_spike
//!     gnome_bridge_spike serve
//!     gnome_bridge_spike write <mimetype> <file>

#[cfg(target_os = "linux")]
mod spike {
    use std::collections::HashMap;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    /// The only executable allowed to call `Offer` or to own the ShellBridge name.
    pub(crate) const SHELL_EXE: &str = "/usr/bin/gnome-shell";

    /// Payloads larger than this are cut off and reported as truncated instead
    /// of being held in memory.
    pub(crate) const MAX_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;

    pub(crate) fn is_shell_executable(exe: &Path) -> bool {
        let path = exe.to_string_lossy();
        path.strip_suffix(" (deleted)").unwrap_or(&path) == SHELL_EXE
    }

    /// The payloads worth pulling from an offer: the first image, plain text
    /// (UTF-8 preferred), and file-manager file references. Every other
    /// format is recorded by name only.
    pub(crate) fn select_wanted_mimes(offered: &[String]) -> Vec<String> {
        let mut wanted = Vec::new();
        if let Some(image) = offered.iter().find(|mime| mime.starts_with("image/")) {
            wanted.push(image.clone());
        }
        let text = offered
            .iter()
            .find(|mime| *mime == "text/plain;charset=utf-8")
            .or_else(|| offered.iter().find(|mime| *mime == "text/plain"));
        if let Some(text) = text {
            wanted.push(text.clone());
        }
        wanted.extend(
            offered
                .iter()
                .filter(|mime| *mime == "text/uri-list" || *mime == "x-special/gnome-copied-files")
                .cloned(),
        );
        wanted
    }

    /// Reads `reader` to EOF, keeping at most `cap` bytes. The flag says
    /// whether anything past `cap` was discarded.
    pub(crate) fn read_capped(reader: impl Read, cap: u64) -> std::io::Result<(Vec<u8>, bool)> {
        let mut bytes = Vec::new();
        reader.take(cap + 1).read_to_end(&mut bytes)?;
        let truncated = bytes.len() as u64 > cap;
        bytes.truncate(cap as usize);
        Ok((bytes, truncated))
    }

    pub(crate) fn short_sha256(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))[..12].to_owned()
    }

    const PASTA_NAME: &str = "com.pasta.Launcher";
    const CLIPBOARD_PATH: &str = "/com/pasta/Launcher/Clipboard";

    fn log(message: &str) {
        eprintln!("PASTA-SPIKE-RS {message}");
    }

    async fn executable_of(
        conn: &zbus::Connection,
        name: zbus::names::BusName<'_>,
    ) -> zbus::fdo::Result<PathBuf> {
        let pid = zbus::fdo::DBusProxy::new(conn)
            .await?
            .get_connection_unix_process_id(name)
            .await?;
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .map_err(|err| zbus::fdo::Error::Failed(format!("reading /proc/{pid}/exe: {err}")))
    }

    /// Reads one payload to EOF on its own thread. There is deliberately no
    /// deadline: the spike records whether writers always close their end.
    fn spawn_payload_reader(mime: String, reader: std::io::PipeReader, offered_at: Instant) {
        std::thread::Builder::new()
            .name("spike-payload-reader".to_owned())
            .spawn(move || match read_capped(reader, MAX_PAYLOAD_BYTES) {
                Ok((bytes, truncated)) => log(&format!(
                    "RECEIVED {mime} {} bytes in {} ms sha256={}{}",
                    bytes.len(),
                    offered_at.elapsed().as_millis(),
                    short_sha256(&bytes),
                    if truncated { " TRUNCATED" } else { "" }
                )),
                Err(err) => log(&format!("read of {mime} failed: {err}")),
            })
            .expect("spawn payload reader thread");
    }

    struct Clipboard;

    #[zbus::interface(name = "com.pasta.Launcher.Clipboard1")]
    impl Clipboard {
        /// Answers a clipboard change with one pipe write end per wanted payload.
        async fn offer(
            &self,
            mimetypes: Vec<String>,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] conn: &zbus::Connection,
        ) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::OwnedFd>> {
            let offered_at = Instant::now();
            let caller = header
                .sender()
                .ok_or_else(|| zbus::fdo::Error::AccessDenied("message has no sender".to_owned()))?
                .to_owned();
            let exe = executable_of(conn, caller.into()).await?;
            if !is_shell_executable(&exe) {
                log(&format!("rejected Offer from {}", exe.display()));
                return Err(zbus::fdo::Error::AccessDenied(format!(
                    "{} is not {SHELL_EXE}",
                    exe.display()
                )));
            }

            let wanted = select_wanted_mimes(&mimetypes);
            log(&format!("Offer mimetypes={mimetypes:?} wanted={wanted:?}"));
            let mut writers = HashMap::new();
            for mime in wanted {
                let (reader, writer) = std::io::pipe()
                    .map_err(|err| zbus::fdo::Error::Failed(format!("pipe: {err}")))?;
                spawn_payload_reader(mime.clone(), reader, offered_at);
                writers.insert(mime, std::os::fd::OwnedFd::from(writer).into());
            }
            Ok(writers)
        }
    }

    fn serve() -> Result<(), Box<dyn std::error::Error>> {
        let _conn = zbus::blocking::connection::Builder::session()?
            .name(PASTA_NAME)?
            .serve_at(CLIPBOARD_PATH, Clipboard)?
            .build()?;
        log(&format!("serving {PASTA_NAME}"));
        loop {
            std::thread::park();
        }
    }

    pub(crate) fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let result = match args.as_slice() {
            [command] if command == "serve" => serve(),
            _ => Err("usage: gnome_bridge_spike serve | write <mimetype> <file>".into()),
        };
        if let Err(err) = result {
            log(&format!("error: {err}"));
            std::process::exit(1);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Cursor;

        fn strings(items: &[&str]) -> Vec<String> {
            items.iter().map(|s| (*s).to_owned()).collect()
        }

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

        #[test]
        fn wants_first_image_utf8_text_and_file_references() {
            let offered = strings(&[
                "image/webp",
                "image/png",
                "text/plain",
                "text/plain;charset=utf-8",
                "text/uri-list",
                "x-special/gnome-copied-files",
                "x-kde-passwordManagerHint",
            ]);
            assert_eq!(
                select_wanted_mimes(&offered),
                strings(&[
                    "image/webp",
                    "text/plain;charset=utf-8",
                    "text/uri-list",
                    "x-special/gnome-copied-files",
                ])
            );
        }

        #[test]
        fn falls_back_to_bare_text_plain() {
            assert_eq!(
                select_wanted_mimes(&strings(&["text/plain"])),
                strings(&["text/plain"])
            );
        }

        #[test]
        fn wants_nothing_from_unknown_formats() {
            assert!(select_wanted_mimes(&strings(&["application/x-custom", "TARGETS"])).is_empty());
        }

        #[test]
        fn reads_everything_under_the_cap() {
            let (bytes, truncated) = read_capped(Cursor::new(b"hello".to_vec()), 5).unwrap();
            assert_eq!(bytes, b"hello");
            assert!(!truncated);
        }

        #[test]
        fn cuts_off_and_reports_payloads_over_the_cap() {
            let (bytes, truncated) = read_capped(Cursor::new(b"hello!".to_vec()), 5).unwrap();
            assert_eq!(bytes, b"hello");
            assert!(truncated);
        }

        #[test]
        fn short_sha256_is_the_first_12_hex_chars() {
            // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
            assert_eq!(short_sha256(b"abc"), "ba7816bf8f01");
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    spike::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {}
