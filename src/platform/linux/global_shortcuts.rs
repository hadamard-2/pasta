//! Global hotkey via the `org.freedesktop.portal.GlobalShortcuts` desktop
//! portal.
//!
//! This is the compositor-blessed way to own a system-wide shortcut: the app
//! asks the portal to bind a shortcut id, the compositor (GNOME/Mutter, KWin,
//! Hyprland, …) does the actual key grab, and we get a D-Bus signal when it
//! fires. Unlike the evdev fallback in `app/runtime.rs` it needs no privileged
//! access to `/dev/input`, it works identically on Wayland and X11, and the
//! binding shows up in the desktop's own shortcut UI instead of being an
//! invisible grab.
//!
//! The exchange is the standard portal request/response dance:
//!
//! 1. `CreateSession` — everything below hangs off the returned session, and
//!    the session dies with our D-Bus connection, so the thread running this
//!    keeps the connection alive for the life of the process.
//! 2. `ListShortcuts` — returns what this app bound in a *previous* run. If our
//!    shortcut is already there we must not call `BindShortcuts` again: some
//!    backends prompt on every bind, and the spec only allows one bind attempt
//!    per session anyway.
//! 3. `BindShortcuts` — first run only. May block for as long as it takes the
//!    user to answer a permission dialog; that is why this lives on its own
//!    thread.
//! 4. Loop on `Activated`, which is what actually opens the launcher.
//!
//! Everything is best-effort: if the portal, the interface, or the binding is
//! unavailable the caller falls back to the evdev listener.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::MenuCommand;

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS_INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const SESSION_INTERFACE: &str = "org.freedesktop.portal.Session";
const REGISTRY_INTERFACE: &str = "org.freedesktop.host.portal.Registry";

/// Must match the basename of `packaging/linux/com.pasta.launcher.desktop`;
/// the portal rejects an app id it cannot resolve to an installed entry.
const APP_ID: &str = "com.pasta.launcher";

/// Identifies our one shortcut inside the session. Unsandboxed apps that never
/// register an app id share a single permission bucket in some portal
/// implementations, so this is namespaced rather than a bare `show`.
pub(crate) const SHOW_LAUNCHER_SHORTCUT_ID: &str = "pasta-show-launcher";

/// Shown to the user by the portal's permission and shortcut-settings UI.
const SHOW_LAUNCHER_DESCRIPTION: &str = "Show the Pasta clipboard launcher";

/// Requested key combination, in the syntax of the freedesktop shortcuts
/// specification: `LOGO` is the Super/Meta key and key names are xkbcommon
/// keysym names minus the `XKB_KEY_` prefix. Only a *preference* — the
/// compositor may hand us something else (or let the user rebind it later),
/// which is why the granted trigger is read back from the response.
const PREFERRED_TRIGGER: &str = "LOGO+space";

/// `org.freedesktop.portal.Request::Response` codes.
const RESPONSE_SUCCESS: u32 = 0;
const RESPONSE_CANCELLED: u32 = 1;

/// Signals we care about are rare (one per keypress at worst), so a small
/// queue is plenty; it only has to absorb bursts while we are mid-handshake.
const SIGNAL_QUEUE_DEPTH: usize = 64;

/// How many times a session that *was* working may drop (portal service
/// restart, compositor replaced) before we give up and let the caller fall
/// back.
const MAX_SESSION_RESTARTS: u32 = 5;
const SESSION_RESTART_BACKOFF: Duration = Duration::from_secs(2);

type Vardict = HashMap<String, OwnedValue>;
/// The portal's `a(sa{sv})` shortcut list: id plus metadata.
type ShortcutList = Vec<(String, Vardict)>;

/// How a portal attempt ended, from the caller's point of view.
pub(crate) enum PortalOutcome {
    /// The portal is authoritative for the hotkey — either it is serving it now
    /// (this variant is only returned once that is no longer true) or the user
    /// declined to grant it. Either way the caller must not start a second,
    /// competing listener.
    Handled,
    /// The portal cannot provide the hotkey here. The caller should fall back.
    Unavailable(String),
}

/// How a single portal session ended.
enum SessionEnd {
    /// The bind was refused — by the user, or by a backend that granted
    /// nothing. Retrying would just re-prompt.
    Declined,
    /// The session was established and later closed (portal restart, logout).
    Closed,
    /// The session could not be established at all.
    Failed(String),
}

/// Serial for `handle_token`s. Tokens become the last element of a D-Bus object
/// path, so they must stay `[A-Za-z0-9_]`.
static TOKEN_SERIAL: AtomicU64 = AtomicU64::new(0);

fn next_token(kind: &str) -> String {
    let serial = TOKEN_SERIAL.fetch_add(1, Ordering::Relaxed);
    format!("pasta_{kind}_{}_{serial}", std::process::id())
}

/// Runs the portal-backed hotkey until it is no longer viable. Blocks for the
/// life of the session, so call it from a dedicated thread.
pub(crate) fn run_show_launcher_shortcut(menu_tx: &mpsc::Sender<MenuCommand>) -> PortalOutcome {
    let conn = match Connection::session() {
        Ok(conn) => conn,
        Err(err) => return PortalOutcome::Unavailable(format!("no session bus: {err}")),
    };

    // Must precede every other portal call on this connection. Only succeeds
    // for an installed .desktop file, and the id is load-bearing: GNOME refuses
    // CreateSession outright without one. The error is kept so a failure later
    // can explain itself instead of just saying "not allowed".
    let app_id_error = register_app_id(&conn);

    let shortcuts = match Proxy::new(&conn, PORTAL_BUS, PORTAL_PATH, SHORTCUTS_INTERFACE) {
        Ok(proxy) => proxy,
        Err(err) => return PortalOutcome::Unavailable(format!("portal proxy failed: {err}")),
    };

    // Reading `version` answers "is the portal running?" and "does this backend
    // implement GlobalShortcuts?" in one cheap call, so an unsupported desktop
    // fails here instead of halfway through the handshake.
    if let Err(err) = shortcuts.get_property::<u32>("version") {
        return PortalOutcome::Unavailable(format!("GlobalShortcuts portal not available: {err}"));
    }

    let mut restarts = 0u32;
    loop {
        match run_session(&conn, &shortcuts, menu_tx) {
            SessionEnd::Declined => {
                eprintln!(
                    "warning: the desktop refused Pasta's global shortcut; open the launcher from the tray icon or bind `pasta-launcher --show` to a shortcut of your own"
                );
                return PortalOutcome::Handled;
            }
            SessionEnd::Failed(reason) => {
                return PortalOutcome::Unavailable(describe_failure(
                    reason,
                    app_id_error.as_deref(),
                ));
            }
            SessionEnd::Closed => {
                restarts += 1;
                if restarts > MAX_SESSION_RESTARTS {
                    return PortalOutcome::Unavailable(
                        "portal shortcut session kept closing".to_owned(),
                    );
                }
                std::thread::sleep(SESSION_RESTART_BACKOFF * restarts);
            }
        }
    }
}

/// Tell the portal which .desktop file we are, so the shortcut is attributed to
/// Pasta and its permission survives restarts. Only meaningful for unsandboxed
/// apps and only once per connection. Returns the failure reason, if any — a
/// sandboxed build is *expected* to fail here (it already has an identity),
/// which is why this is reported rather than treated as fatal.
fn register_app_id(conn: &Connection) -> Option<String> {
    let registry = match Proxy::new(conn, PORTAL_BUS, PORTAL_PATH, REGISTRY_INTERFACE) {
        Ok(registry) => registry,
        Err(err) => return Some(err.to_string()),
    };
    let options: HashMap<&str, Value> = HashMap::new();
    registry
        .call::<_, _, ()>("Register", &(APP_ID, options))
        .err()
        .map(|err| err.to_string())
}

/// Compose the message the caller logs before falling back. An unregistered app
/// id is by far the most common cause of a portal refusal — GNOME rejects
/// `CreateSession` with a bare "an app id is required" — so when we know the id
/// did not take, say so instead of forwarding a message that reads like a bug.
fn describe_failure(reason: String, app_id_error: Option<&str>) -> String {
    match app_id_error {
        Some(err) => format!(
            "{reason}; app id '{APP_ID}' was not registered ({err}), which most desktops require — install the desktop entry with scripts/install-linux-app.sh instead of running the binary straight out of the build tree"
        ),
        None => reason,
    }
}

fn run_session(
    conn: &Connection,
    shortcuts: &Proxy<'_>,
    menu_tx: &mpsc::Sender<MenuCommand>,
) -> SessionEnd {
    // Subscribe before the first call: the Response signal for a request can
    // land before (or as) the method reply does, and a missed one would hang
    // the handshake forever.
    let rule = match MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(PORTAL_BUS)
    {
        Ok(builder) => builder.build(),
        Err(err) => return SessionEnd::Failed(format!("bad match rule: {err}")),
    };
    let mut messages = match MessageIterator::for_match_rule(rule, conn, Some(SIGNAL_QUEUE_DEPTH)) {
        Ok(messages) => messages,
        Err(err) => return SessionEnd::Failed(format!("portal signal subscription failed: {err}")),
    };

    let session = match create_session(shortcuts, &mut messages) {
        Ok(session) => session,
        Err(err) => return SessionEnd::Failed(err),
    };

    // What a previous run of this app already bound. Re-binding an existing
    // shortcut is both unnecessary and user-hostile on backends that prompt.
    let existing = list_shortcuts(shortcuts, &session, &mut messages).unwrap_or_default();
    let mut trigger = trigger_description(&existing, SHOW_LAUNCHER_SHORTCUT_ID);

    if trigger.is_none() {
        match bind_shortcut(shortcuts, &session, &mut messages) {
            Ok(bound) => match trigger_description(&bound, SHOW_LAUNCHER_SHORTCUT_ID) {
                Some(description) => trigger = Some(description),
                // A success response that omits our id means the backend chose
                // not to grant it; treat that exactly like a refusal.
                None => return SessionEnd::Declined,
            },
            Err(BindError::Declined) => return SessionEnd::Declined,
            Err(BindError::Failed(err)) => return SessionEnd::Failed(err),
        }
    }

    eprintln!(
        "pasta: global shortcut registered with the desktop portal ({})",
        trigger.as_deref().unwrap_or("trigger unknown")
    );

    listen(&mut messages, &session, menu_tx)
}

fn create_session(
    shortcuts: &Proxy<'_>,
    messages: &mut MessageIterator,
) -> Result<OwnedObjectPath, String> {
    let mut options: HashMap<&str, Value> = HashMap::new();
    let handle_token = next_token("req");
    options.insert("handle_token", Value::from(handle_token.as_str()));
    let session_token = next_token("session");
    options.insert("session_handle_token", Value::from(session_token.as_str()));

    let request: OwnedObjectPath = shortcuts
        .call("CreateSession", &(options,))
        .map_err(|err| format!("CreateSession failed: {err}"))?;
    let (code, results) = wait_for_response(messages, request.as_str())?;
    if code != RESPONSE_SUCCESS {
        return Err(format!("CreateSession refused (response {code})"));
    }

    // The session handle is documented as `o` but was shipped as `s`; the spec
    // keeps the wrong type for compatibility, so read a string and convert.
    let handle = results
        .get("session_handle")
        .cloned()
        .ok_or_else(|| "CreateSession returned no session_handle".to_owned())?;
    let handle =
        String::try_from(handle).map_err(|err| format!("malformed session_handle: {err}"))?;
    OwnedObjectPath::try_from(handle.as_str())
        .map_err(|err| format!("malformed session_handle '{handle}': {err}"))
}

fn list_shortcuts(
    shortcuts: &Proxy<'_>,
    session: &OwnedObjectPath,
    messages: &mut MessageIterator,
) -> Result<ShortcutList, String> {
    let mut options: HashMap<&str, Value> = HashMap::new();
    let handle_token = next_token("req");
    options.insert("handle_token", Value::from(handle_token.as_str()));

    let request: OwnedObjectPath = shortcuts
        .call("ListShortcuts", &(session, options))
        .map_err(|err| format!("ListShortcuts failed: {err}"))?;
    let (code, results) = wait_for_response(messages, request.as_str())?;
    if code != RESPONSE_SUCCESS {
        return Err(format!("ListShortcuts refused (response {code})"));
    }
    Ok(shortcut_list(&results))
}

enum BindError {
    Declined,
    Failed(String),
}

fn bind_shortcut(
    shortcuts: &Proxy<'_>,
    session: &OwnedObjectPath,
    messages: &mut MessageIterator,
) -> Result<ShortcutList, BindError> {
    let mut metadata: HashMap<&str, Value> = HashMap::new();
    metadata.insert("description", Value::from(SHOW_LAUNCHER_DESCRIPTION));
    metadata.insert("preferred_trigger", Value::from(PREFERRED_TRIGGER));
    let to_bind = vec![(SHOW_LAUNCHER_SHORTCUT_ID, metadata)];

    let mut options: HashMap<&str, Value> = HashMap::new();
    let handle_token = next_token("req");
    options.insert("handle_token", Value::from(handle_token.as_str()));

    // `parent_window` is empty: the launcher has no window at this point, and
    // an unparented permission dialog is the right shape for a background app.
    let request: OwnedObjectPath = shortcuts
        .call("BindShortcuts", &(session, to_bind, "", options))
        .map_err(|err| BindError::Failed(format!("BindShortcuts failed: {err}")))?;

    // No timeout: a backend that prompts holds this open until the user
    // answers, and we would rather wait than race the dialog.
    let (code, results) =
        wait_for_response(messages, request.as_str()).map_err(BindError::Failed)?;
    match code {
        RESPONSE_SUCCESS => Ok(shortcut_list(&results)),
        RESPONSE_CANCELLED => Err(BindError::Declined),
        other => Err(BindError::Failed(format!(
            "BindShortcuts refused (response {other})"
        ))),
    }
}

/// Main loop: turn `Activated` into a launcher show, and stop when the session
/// goes away.
fn listen(
    messages: &mut MessageIterator,
    session: &OwnedObjectPath,
    menu_tx: &mpsc::Sender<MenuCommand>,
) -> SessionEnd {
    for message in messages.by_ref() {
        let Ok(message) = message else { continue };
        let header = message.header();
        let interface = header.interface().map(|i| i.as_str().to_owned());
        let member = header.member().map(|m| m.as_str().to_owned());

        match (interface.as_deref(), member.as_deref()) {
            (Some(SHORTCUTS_INTERFACE), Some("Activated")) => {
                let Ok((signal_session, id, _timestamp, _options)) =
                    message
                        .body()
                        .deserialize::<(OwnedObjectPath, String, u64, Vardict)>()
                else {
                    continue;
                };
                if signal_session != *session || id != SHOW_LAUNCHER_SHORTCUT_ID {
                    continue;
                }
                // A dead channel means the app is shutting down; nothing left
                // for this listener to do.
                if menu_tx.send(MenuCommand::ShowLauncher).is_err() {
                    return SessionEnd::Closed;
                }
            }
            (Some(SHORTCUTS_INTERFACE), Some("ShortcutsChanged")) => {
                let Ok((signal_session, shortcuts)) = message
                    .body()
                    .deserialize::<(OwnedObjectPath, ShortcutList)>()
                else {
                    continue;
                };
                if signal_session != *session {
                    continue;
                }
                // The user can rebind us from the desktop's shortcut settings;
                // log the new trigger so support questions are answerable.
                if let Some(trigger) = trigger_description(&shortcuts, SHOW_LAUNCHER_SHORTCUT_ID) {
                    eprintln!("pasta: global shortcut is now {trigger}");
                }
            }
            (Some(SESSION_INTERFACE), Some("Closed"))
                if header.path().map(|path| path.as_str()) == Some(session.as_str()) =>
            {
                return SessionEnd::Closed;
            }
            _ => {}
        }
    }

    SessionEnd::Closed
}

/// Block until the `Response` for `request_path` arrives, ignoring unrelated
/// portal signals.
fn wait_for_response(
    messages: &mut MessageIterator,
    request_path: &str,
) -> Result<(u32, Vardict), String> {
    for message in messages.by_ref() {
        let Ok(message) = message else { continue };
        let header = message.header();
        let is_response = header.interface().map(|i| i.as_str()) == Some(REQUEST_INTERFACE)
            && header.member().map(|m| m.as_str()) == Some("Response")
            && header.path().map(|p| p.as_str()) == Some(request_path);
        if !is_response {
            continue;
        }
        return message
            .body()
            .deserialize::<(u32, Vardict)>()
            .map_err(|err| format!("malformed portal response: {err}"));
    }
    Err("portal signal stream ended".to_owned())
}

/// Pull the `shortcuts` array out of a request response, tolerating a backend
/// that omits it.
fn shortcut_list(results: &Vardict) -> ShortcutList {
    results
        .get("shortcuts")
        .cloned()
        .and_then(|value| ShortcutList::try_from(value).ok())
        .unwrap_or_default()
}

/// The human-readable trigger the compositor granted for `id` (e.g. "Press
/// <Super>space"), or `None` if `id` is not in the list at all. Backends are
/// allowed to omit `trigger_description`, so a bound-but-undescribed shortcut
/// falls back to the id itself rather than reading as "not bound".
fn trigger_description(shortcuts: &ShortcutList, id: &str) -> Option<String> {
    let (_, metadata) = shortcuts.iter().find(|(entry, _)| entry == id)?;
    let description = metadata
        .get("trigger_description")
        .cloned()
        .and_then(|value| String::try_from(value).ok())
        .filter(|text| !text.is_empty());
    Some(description.unwrap_or_else(|| id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(id: &str, trigger: Option<&str>) -> (String, Vardict) {
        let mut metadata: Vardict = HashMap::new();
        metadata.insert(
            "description".to_owned(),
            Value::from(SHOW_LAUNCHER_DESCRIPTION)
                .try_to_owned()
                .expect("owned value"),
        );
        if let Some(trigger) = trigger {
            metadata.insert(
                "trigger_description".to_owned(),
                Value::from(trigger).try_to_owned().expect("owned value"),
            );
        }
        (id.to_owned(), metadata)
    }

    #[test]
    fn handle_tokens_are_valid_object_path_elements() {
        for kind in ["req", "session"] {
            let token = next_token(kind);
            assert!(
                token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "token '{token}' is not a valid object path element"
            );
            assert!(!token.is_empty());
        }
    }

    #[test]
    fn handle_tokens_are_unique_per_call() {
        let first = next_token("req");
        let second = next_token("req");
        assert_ne!(first, second);
    }

    #[test]
    fn trigger_description_reads_the_granted_trigger() {
        let list = vec![shortcut(
            SHOW_LAUNCHER_SHORTCUT_ID,
            Some("Press <Super>space"),
        )];
        assert_eq!(
            trigger_description(&list, SHOW_LAUNCHER_SHORTCUT_ID).as_deref(),
            Some("Press <Super>space")
        );
    }

    #[test]
    fn trigger_description_is_none_when_the_shortcut_was_not_granted() {
        let list = vec![shortcut("someone-elses-shortcut", Some("Press <Super>k"))];
        assert!(trigger_description(&list, SHOW_LAUNCHER_SHORTCUT_ID).is_none());
    }

    #[test]
    fn bound_shortcut_without_a_description_still_counts_as_bound() {
        let list = vec![shortcut(SHOW_LAUNCHER_SHORTCUT_ID, None)];
        assert_eq!(
            trigger_description(&list, SHOW_LAUNCHER_SHORTCUT_ID).as_deref(),
            Some(SHOW_LAUNCHER_SHORTCUT_ID)
        );
        let list = vec![shortcut(SHOW_LAUNCHER_SHORTCUT_ID, Some(""))];
        assert_eq!(
            trigger_description(&list, SHOW_LAUNCHER_SHORTCUT_ID).as_deref(),
            Some(SHOW_LAUNCHER_SHORTCUT_ID)
        );
    }

    #[test]
    fn shortcut_list_tolerates_a_response_without_shortcuts() {
        let results: Vardict = HashMap::new();
        assert!(shortcut_list(&results).is_empty());
    }

    #[test]
    fn shortcut_list_reads_the_portal_array() {
        let mut results: Vardict = HashMap::new();
        let list = vec![shortcut(
            SHOW_LAUNCHER_SHORTCUT_ID,
            Some("Press <Super>space"),
        )];
        results.insert(
            "shortcuts".to_owned(),
            Value::from(list).try_to_owned().expect("owned value"),
        );
        let parsed = shortcut_list(&results);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, SHOW_LAUNCHER_SHORTCUT_ID);
    }

    #[test]
    fn preferred_trigger_uses_shortcuts_spec_syntax() {
        // Modifiers are the XKB_MOD_NAME_* names (LOGO, not SUPER/META) and the
        // key is an xkbcommon keysym name minus the XKB_KEY_ prefix.
        let (modifiers, key) = PREFERRED_TRIGGER
            .rsplit_once('+')
            .expect("trigger has a modifier");
        assert_eq!(modifiers, "LOGO");
        assert_eq!(key, "space");
    }

    #[test]
    fn failure_message_points_at_the_app_id_when_registration_failed() {
        let message = describe_failure(
            "CreateSession failed: NotAllowed".to_owned(),
            Some("App info not found"),
        );
        assert!(message.contains("CreateSession failed: NotAllowed"));
        assert!(message.contains(APP_ID));
        assert!(message.contains("install-linux-app.sh"));
    }

    #[test]
    fn failure_message_is_left_alone_when_the_app_id_registered() {
        let reason = "portal signal stream ended".to_owned();
        assert_eq!(describe_failure(reason.clone(), None), reason);
    }

    #[test]
    fn app_id_matches_the_packaged_desktop_entry() {
        // The portal only accepts an app id that resolves to an installed
        // .desktop file, so these must not drift apart.
        assert!(
            std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/packaging/linux/com.pasta.launcher.desktop"
            ))
            .exists()
        );
        assert_eq!(APP_ID, "com.pasta.launcher");
    }
}
