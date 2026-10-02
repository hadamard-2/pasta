//! Pasta's side of the GNOME Shell clipboard bridge: answers `Offer` with pipe
//! write ends for the formats it wants, reads them, and publishes the result as
//! the current clipboard snapshot.
#![allow(dead_code)] // Temporary: removed once the bridge is wired in (Task 4).

use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

/// How long one payload may take to arrive before Pasta gives up on it.
pub(crate) const PAYLOAD_DEADLINE: Duration = Duration::from_secs(5);

/// The offered formats worth reading: the first image, plain text (UTF-8
/// preferred), and file-manager file references. Every other format is kept
/// by name only (it still drives concealed/transient detection).
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PayloadError {
    TooLarge,
    DeadlineExceeded,
    Io(String),
}

/// Reads `fd` to EOF, giving up when it exceeds `cap` bytes or does not reach
/// EOF within `deadline`.
pub(crate) fn read_payload(
    fd: OwnedFd,
    cap: usize,
    deadline: Duration,
) -> Result<Vec<u8>, PayloadError> {
    use nix::poll::{PollFd, PollFlags, poll};

    let mut file = std::fs::File::from(fd);
    let give_up_at = Instant::now() + deadline;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let remaining = give_up_at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PayloadError::DeadlineExceeded);
        }
        let mut fds = [PollFd::new(file.as_raw_fd(), PollFlags::POLLIN)];
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
        match poll(&mut fds, timeout_ms) {
            Ok(0) => return Err(PayloadError::DeadlineExceeded),
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(err) => return Err(PayloadError::Io(err.to_string())),
        }
        match file.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => {
                if bytes.len() + read > cap {
                    return Err(PayloadError::TooLarge);
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(PayloadError::Io(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
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
    fn reads_a_payload_to_eof() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let feeder = std::thread::spawn(move || writer.write_all(b"hello"));
        let bytes = read_payload(OwnedFd::from(reader), 1024, Duration::from_secs(2));
        feeder.join().unwrap().unwrap();
        assert_eq!(bytes, Ok(b"hello".to_vec()));
    }

    #[test]
    fn rejects_a_payload_over_the_cap() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let feeder = std::thread::spawn(move || {
            let _ = writer.write_all(b"0123456789");
        });
        let result = read_payload(OwnedFd::from(reader), 8, Duration::from_secs(2));
        feeder.join().unwrap();
        assert_eq!(result, Err(PayloadError::TooLarge));
    }

    #[test]
    fn gives_up_on_a_writer_that_never_finishes() {
        // Stands in for a source app that stalls mid-transfer: the write end
        // stays open and nothing arrives.
        let (reader, writer) = std::io::pipe().unwrap();
        let started = Instant::now();
        let result = read_payload(OwnedFd::from(reader), 1024, Duration::from_millis(150));
        assert_eq!(result, Err(PayloadError::DeadlineExceeded));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(writer);
    }
}
