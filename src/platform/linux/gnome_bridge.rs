//! Pasta's side of the GNOME Shell clipboard bridge: answers `Offer` with pipe
//! write ends for the formats it wants, reads them, and publishes the result as
//! the current clipboard snapshot.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::gnome_bridge_client::{MAX_PAYLOAD_BYTES, SHELL_EXE, is_shell_executable};

const PASTA_NAME: &str = "com.pasta.Launcher";
const CLIPBOARD_PATH: &str = "/com/pasta/Launcher/Clipboard";

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

/// The most recent clipboard contents the shell offered, as far as Pasta read
/// them: every offered format name, plus the payloads it asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GnomeClipboardSnapshot {
    pub(crate) mimetypes: Vec<String>,
    pub(crate) payloads: HashMap<String, Vec<u8>>,
}

impl GnomeClipboardSnapshot {
    pub(crate) fn text(&self) -> Option<String> {
        ["text/plain;charset=utf-8", "text/plain"]
            .iter()
            .find_map(|mime| self.payloads.get(*mime))
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
    }

    pub(crate) fn image(&self) -> Option<(Vec<u8>, String)> {
        self.mimetypes
            .iter()
            .filter(|mime| mime.starts_with("image/"))
            .find_map(|mime| {
                self.payloads
                    .get(mime)
                    .map(|bytes| (bytes.clone(), mime.clone()))
            })
    }

    pub(crate) fn bytes(&self, mime: &str) -> Option<Vec<u8>> {
        self.payloads.get(mime).cloned()
    }
}

/// Latest snapshot plus a counter the clipboard watcher compares, mirroring
/// the change counters of the other clipboard paths.
pub(crate) struct SnapshotStore {
    /// The published snapshot and the offer sequence it came from.
    snapshot: Mutex<(Option<GnomeClipboardSnapshot>, u64)>,
    change_count: AtomicI64,
}

impl SnapshotStore {
    pub(crate) const fn new() -> Self {
        Self {
            snapshot: Mutex::new((None, 0)),
            change_count: AtomicI64::new(0),
        }
    }

    pub(crate) fn snapshot(&self) -> Option<GnomeClipboardSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0
            .clone()
    }

    pub(crate) fn change_count(&self) -> i64 {
        self.change_count.load(Ordering::Acquire)
    }

    /// Publishes the snapshot of offer `sequence`, unless a newer offer has
    /// already been published. Offers are read on separate threads, so a slow
    /// older one can finish after a newer one; it must not become current.
    /// Returns whether the snapshot was published.
    fn publish(&self, sequence: u64, snapshot: GnomeClipboardSnapshot) -> bool {
        let mut current = self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if sequence <= current.1 {
            return false;
        }
        *current = (Some(snapshot), sequence);
        self.change_count.fetch_add(1, Ordering::AcqRel);
        true
    }
}

pub(crate) static STORE: SnapshotStore = SnapshotStore::new();

/// Orders offers as they arrive; the first offer gets 1.
static OFFER_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Reason shown when Pasta's own clipboard service code panicked.
const SERVICE_PANIC_REASON: &str = "Pasta's clipboard service hit an internal error, so this copy was not captured. Restart Pasta if copies stop appearing.";

/// Runs one piece of the clipboard service's own work. A panic would otherwise
/// vanish silently (zbus keeps a panicking handler task to itself, and a
/// panicking reader thread just ends), so it is caught here and handed to
/// `report` as an unavailable-capture reason instead.
fn guard_service_panic<T>(report: impl FnOnce(String), work: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => Some(value),
        Err(_) => {
            eprintln!("warning: GNOME clipboard service panicked");
            report(SERVICE_PANIC_REASON.to_owned());
            None
        }
    }
}

/// Reads every wanted payload of one offer on its own thread, then publishes
/// the snapshot once all of them have finished or been given up on.
fn collect_offer(sequence: u64, mimetypes: Vec<String>, readers: Vec<(String, OwnedFd)>) {
    let spawned = std::thread::Builder::new()
        .name("pasta-gnome-offer".to_owned())
        .spawn(move || {
            guard_service_panic(super::set_clipboard_capture_blocked, || {
                read_and_publish(sequence, mimetypes, readers)
            });
        });
    if let Err(err) = spawned {
        eprintln!("warning: could not start GNOME clipboard reader thread: {err}");
    }
}

fn read_and_publish(sequence: u64, mimetypes: Vec<String>, readers: Vec<(String, OwnedFd)>) {
    let mut payloads = HashMap::new();
    for (mime, fd) in readers {
        match read_payload(fd, MAX_PAYLOAD_BYTES, PAYLOAD_DEADLINE) {
            Ok(bytes) => {
                payloads.insert(mime, bytes);
            }
            Err(err) => {
                eprintln!("warning: GNOME clipboard payload {mime} dropped: {err:?}");
            }
        }
    }
    let mut summary: Vec<String> = payloads
        .iter()
        .map(|(mime, bytes)| format!("{mime}={}", bytes.len()))
        .collect();
    summary.sort();
    let summary = summary.join(", ");
    let logged_mimetypes = format!("{mimetypes:?}");
    if STORE.publish(
        sequence,
        GnomeClipboardSnapshot {
            mimetypes,
            payloads,
        },
    ) {
        eprintln!(
            "info: GNOME clipboard snapshot published: mimetypes={logged_mimetypes} payloads=[{summary}]"
        );
    } else {
        eprintln!(
            "info: GNOME clipboard snapshot dropped, a newer copy was already published: mimetypes={logged_mimetypes} payloads=[{summary}]"
        );
    }
}

async fn executable_of(
    conn: &zbus::Connection,
    name: zbus::names::BusName<'_>,
) -> zbus::fdo::Result<std::path::PathBuf> {
    let pid = zbus::fdo::DBusProxy::new(conn)
        .await?
        .get_connection_unix_process_id(name)
        .await?;
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .map_err(|err| zbus::fdo::Error::Failed(format!("reading /proc/{pid}/exe: {err}")))
}

struct ClipboardService;

#[zbus::interface(name = "com.pasta.Launcher.Clipboard1")]
impl ClipboardService {
    /// Answers a clipboard change with one pipe write end per wanted format.
    async fn offer(
        &self,
        mimetypes: Vec<String>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::OwnedFd>> {
        let caller = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::AccessDenied("message has no sender".to_owned()))?
            .to_owned();
        let exe = executable_of(conn, caller.into()).await?;
        if !is_shell_executable(&exe) {
            eprintln!(
                "warning: refused GNOME clipboard Offer from {}",
                exe.display()
            );
            return Err(zbus::fdo::Error::AccessDenied(format!(
                "{} is not {SHELL_EXE}",
                exe.display()
            )));
        }

        guard_service_panic(super::set_clipboard_capture_blocked, || {
            answer_offer(mimetypes)
        })
        .unwrap_or_else(|| {
            Err(zbus::fdo::Error::Failed(
                "internal error handling Offer".to_owned(),
            ))
        })
    }
}

/// Opens one pipe per wanted format, starts reading them, and returns the
/// write ends for the shell.
fn answer_offer(
    mimetypes: Vec<String>,
) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::OwnedFd>> {
    let sequence = OFFER_SEQUENCE.fetch_add(1, Ordering::AcqRel);
    let mut writers = HashMap::new();
    let mut readers = Vec::new();
    for mime in select_wanted_mimes(&mimetypes) {
        let (reader, writer) =
            std::io::pipe().map_err(|err| zbus::fdo::Error::Failed(format!("pipe: {err}")))?;
        readers.push((mime.clone(), OwnedFd::from(reader)));
        writers.insert(mime, OwnedFd::from(writer).into());
    }
    collect_offer(sequence, mimetypes, readers);
    Ok(writers)
}

/// Registers Pasta's clipboard service on the session bus, once. The
/// connection lives in a static so the service stays up for the process
/// lifetime; zbus runs it on its own executor thread.
pub(crate) fn ensure_service() -> Result<(), String> {
    static SERVICE: std::sync::OnceLock<Result<zbus::blocking::Connection, String>> =
        std::sync::OnceLock::new();
    SERVICE
        .get_or_init(|| {
            let built = zbus::blocking::connection::Builder::session()
                .and_then(|builder| builder.name(PASTA_NAME))
                .and_then(|builder| builder.serve_at(CLIPBOARD_PATH, ClipboardService))
                .and_then(|builder| builder.build())
                .map_err(|err| err.to_string());
            if let Err(err) = &built {
                eprintln!("warning: could not register {PASTA_NAME} on the session bus: {err}");
            }
            built
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_panic_in_service_work_is_reported_as_a_reason() {
        let mut reported = None;
        let result: Option<()> =
            guard_service_panic(|reason| reported = Some(reason), || panic!("boom"));
        assert!(result.is_none());
        assert_eq!(reported.as_deref(), Some(SERVICE_PANIC_REASON));
    }

    #[test]
    fn service_work_that_finishes_reports_nothing() {
        let mut reported = None;
        let result = guard_service_panic(|reason| reported = Some(reason), || 7);
        assert_eq!(result, Some(7));
        assert!(reported.is_none());
    }

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

    fn snapshot(mimes: &[&str], payloads: &[(&str, &[u8])]) -> GnomeClipboardSnapshot {
        GnomeClipboardSnapshot {
            mimetypes: strings(mimes),
            payloads: payloads
                .iter()
                .map(|(mime, bytes)| ((*mime).to_owned(), bytes.to_vec()))
                .collect(),
        }
    }

    #[test]
    fn snapshot_text_prefers_utf8() {
        let snap = snapshot(
            &["text/plain", "text/plain;charset=utf-8"],
            &[("text/plain;charset=utf-8", "héllo".as_bytes())],
        );
        assert_eq!(snap.text().as_deref(), Some("héllo"));
    }

    #[test]
    fn snapshot_text_falls_back_to_bare_text_plain() {
        let snap = snapshot(&["text/plain"], &[("text/plain", b"plain")]);
        assert_eq!(snap.text().as_deref(), Some("plain"));
    }

    #[test]
    fn snapshot_image_is_the_first_offered_image_that_arrived() {
        let snap = snapshot(
            &["image/png", "text/plain"],
            &[("image/png", b"\x89PNG"), ("text/plain", b"caption")],
        );
        assert_eq!(
            snap.image(),
            Some((b"\x89PNG".to_vec(), "image/png".to_owned()))
        );
    }

    #[test]
    fn snapshot_without_image_payload_has_no_image() {
        let snap = snapshot(&["image/png"], &[]);
        assert_eq!(snap.image(), None);
    }

    #[test]
    fn publishing_replaces_the_snapshot_and_bumps_the_counter() {
        // A local store, not the process-global one, so this test does not
        // depend on the order other tests run in.
        let store = SnapshotStore::new();
        assert_eq!(store.change_count(), 0);
        assert_eq!(store.snapshot(), None);
        assert!(store.publish(1, snapshot(&["text/plain"], &[("text/plain", b"a")])));
        assert!(store.publish(2, snapshot(&["text/plain"], &[("text/plain", b"b")])));
        assert_eq!(store.change_count(), 2);
        assert_eq!(
            store.snapshot().and_then(|s| s.text()).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn an_older_offer_finishing_late_does_not_replace_a_newer_one() {
        let store = SnapshotStore::new();
        assert!(store.publish(2, snapshot(&["text/plain"], &[("text/plain", b"new")])));
        assert!(!store.publish(1, snapshot(&["text/plain"], &[("text/plain", b"old")])));
        assert_eq!(store.change_count(), 1);
        assert_eq!(
            store.snapshot().and_then(|s| s.text()).as_deref(),
            Some("new")
        );
    }
}
