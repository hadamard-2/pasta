//! Whether Pasta's GNOME Shell extension is usable, and what the launcher
//! should tell the user when it is not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::OwnedValue;

use crate::CaptureFixAction;

pub(crate) const EXTENSION_UUID: &str = "clipboard@pasta.launcher";

const SHELL_BUS: &str = "org.gnome.Shell";
const SHELL_PATH: &str = "/org/gnome/Shell";
const EXTENSIONS_INTERFACE: &str = "org.gnome.Shell.Extensions";

// GNOME 50 ExtensionState values (misc/extensionUtils.js).
const STATE_INACTIVE: f64 = 2.0;
const STATE_ERROR: f64 = 3.0;
const STATE_OUT_OF_DATE: f64 = 4.0;
const STATE_INITIALIZED: f64 = 6.0;

/// What GNOME reports for a loaded extension.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LoadedExtension {
    pub(crate) state: f64,
    pub(crate) version: Option<f64>,
    pub(crate) path: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExtensionStatus {
    ExtensionsDisabled,
    NeedsLogout,
    NotInstalled,
    UpdatePending,
    Failed(String),
    OutOfDate,
    Disabled,
    Active,
}

pub(crate) fn classify(
    user_extensions_enabled: bool,
    loaded: Option<&LoadedExtension>,
    installed_on_disk: bool,
    on_disk_version: Option<f64>,
    first_error: Option<&str>,
) -> ExtensionStatus {
    if !user_extensions_enabled {
        return ExtensionStatus::ExtensionsDisabled;
    }
    let Some(loaded) = loaded else {
        return if installed_on_disk {
            ExtensionStatus::NeedsLogout
        } else {
            ExtensionStatus::NotInstalled
        };
    };
    if let (Some(running), Some(on_disk)) = (loaded.version, on_disk_version)
        && running != on_disk
    {
        return ExtensionStatus::UpdatePending;
    }
    if loaded.state == STATE_ERROR {
        return ExtensionStatus::Failed(first_error.unwrap_or("unknown error").to_owned());
    }
    if loaded.state == STATE_OUT_OF_DATE {
        return ExtensionStatus::OutOfDate;
    }
    if loaded.state == STATE_INACTIVE || loaded.state == STATE_INITIALIZED {
        return ExtensionStatus::Disabled;
    }
    ExtensionStatus::Active
}

pub(crate) fn notice_for(status: &ExtensionStatus) -> Option<(String, Option<CaptureFixAction>)> {
    let text = match status {
        ExtensionStatus::Active => return None,
        ExtensionStatus::Disabled => {
            return Some((
                "Pasta needs its GNOME extension to see your clipboard.".to_owned(),
                Some(CaptureFixAction::EnableGnomeExtension),
            ));
        }
        ExtensionStatus::ExtensionsDisabled => "Extensions are turned off in GNOME. Turn them on in the Extensions app so Pasta can see your clipboard.".to_owned(),
        ExtensionStatus::NeedsLogout => {
            "Log out and back in to finish installing Pasta's GNOME extension.".to_owned()
        }
        ExtensionStatus::NotInstalled => {
            "Pasta's GNOME extension isn't installed. Reinstall Pasta to add it.".to_owned()
        }
        ExtensionStatus::UpdatePending => {
            "Log out and back in to finish updating Pasta's GNOME extension.".to_owned()
        }
        ExtensionStatus::Failed(error) => {
            format!("Pasta's GNOME extension failed to load: {error}.")
        }
        ExtensionStatus::OutOfDate => {
            "This GNOME version isn't supported by Pasta's extension yet.".to_owned()
        }
    };
    Some((text, None))
}

fn extension_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/gnome-shell/extensions").join(EXTENSION_UUID)];
    if let Some(data) = dirs::data_dir() {
        dirs.push(data.join("gnome-shell/extensions").join(EXTENSION_UUID));
    }
    dirs
}

fn read_metadata_version(extension_dir: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(extension_dir.join("metadata.json")).ok()?;
    let metadata: serde_json::Value = serde_json::from_str(&text).ok()?;
    metadata.get("version")?.as_f64()
}

fn parse_loaded(info: &HashMap<String, OwnedValue>) -> Option<LoadedExtension> {
    let state = info.get("state")?.downcast_ref::<f64>().ok()?;
    Some(LoadedExtension {
        state,
        version: info
            .get("version")
            .and_then(|value| value.downcast_ref::<f64>().ok()),
        path: info
            .get("path")
            .and_then(|value| value.downcast_ref::<&str>().ok())
            .map(str::to_owned),
    })
}

fn shell_extensions(conn: &Connection) -> Result<Proxy<'static>, String> {
    Proxy::new(conn, SHELL_BUS, SHELL_PATH, EXTENSIONS_INTERFACE).map_err(|err| err.to_string())
}

fn query_status(conn: &Connection) -> Result<ExtensionStatus, String> {
    let shell = shell_extensions(conn)?;
    let enabled: bool = shell
        .get_property("UserExtensionsEnabled")
        .map_err(|err| err.to_string())?;
    let info: HashMap<String, OwnedValue> = shell
        .call("GetExtensionInfo", &(EXTENSION_UUID,))
        .map_err(|err| err.to_string())?;
    let loaded = parse_loaded(&info);
    let errors: Vec<String> = shell
        .call("GetExtensionErrors", &(EXTENSION_UUID,))
        .unwrap_or_default();
    let on_disk_version = loaded
        .as_ref()
        .and_then(|loaded| loaded.path.as_deref())
        .and_then(|path| read_metadata_version(Path::new(path)));
    let installed_on_disk = extension_dirs().iter().any(|dir| dir.is_dir());
    Ok(classify(
        enabled,
        loaded.as_ref(),
        installed_on_disk,
        on_disk_version,
        errors.first().map(String::as_str),
    ))
}

/// Reports the current notice, then re-checks whenever GNOME Shell signals
/// anything on its object (extension state changes, the global extensions
/// switch), reporting only when the notice changes. Runs for the process
/// lifetime on its own thread.
pub(crate) fn spawn_status_watcher(
    on_change: impl Fn(Option<(String, Option<CaptureFixAction>)>) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("pasta-gnome-extension-status".to_owned())
        .spawn(move || {
            let conn = match Connection::session() {
                Ok(conn) => conn,
                Err(err) => {
                    on_change(Some((
                        format!("Pasta could not check its GNOME extension: {err}"),
                        None,
                    )));
                    return;
                }
            };
            let messages = MatchRule::builder()
                .msg_type(MessageType::Signal)
                .sender(SHELL_BUS)
                .and_then(|builder| builder.path(SHELL_PATH))
                .map(|builder| builder.build())
                .and_then(|rule| MessageIterator::for_match_rule(rule, &conn, Some(16)));
            let mut messages = match messages {
                Ok(messages) => Some(messages),
                Err(err) => {
                    eprintln!("warning: cannot follow GNOME extension state changes: {err}");
                    None
                }
            };

            let mut last: Option<Option<(String, Option<CaptureFixAction>)>> = None;
            loop {
                let notice = match query_status(&conn) {
                    Ok(status) => notice_for(&status),
                    Err(err) => Some((
                        format!("Pasta could not check its GNOME extension: {err}"),
                        None,
                    )),
                };
                if last.as_ref() != Some(&notice) {
                    on_change(notice.clone());
                    last = Some(notice);
                }
                match messages.as_mut().and_then(|messages| messages.next()) {
                    Some(_) => continue,
                    None => return,
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("warning: could not start GNOME extension status thread: {err}");
    }
}

/// Asks GNOME Shell to enable the extension. Only ever called from a user's
/// click on the launcher's Enable button.
pub(crate) fn enable_extension() -> Result<bool, String> {
    let conn = Connection::session().map_err(|err| err.to_string())?;
    shell_extensions(&conn)?
        .call("EnableExtension", &(EXTENSION_UUID,))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(state: f64, version: Option<f64>) -> LoadedExtension {
        LoadedExtension {
            state,
            version,
            path: None,
        }
    }

    #[test]
    fn extensions_switched_off_wins_over_everything() {
        let active = loaded(1.0, Some(1.0));
        assert_eq!(
            classify(false, Some(&active), true, Some(1.0), None),
            ExtensionStatus::ExtensionsDisabled
        );
    }

    #[test]
    fn unknown_to_gnome_but_on_disk_needs_a_logout() {
        assert_eq!(
            classify(true, None, true, None, None),
            ExtensionStatus::NeedsLogout
        );
    }

    #[test]
    fn unknown_to_gnome_and_absent_is_not_installed() {
        assert_eq!(
            classify(true, None, false, None, None),
            ExtensionStatus::NotInstalled
        );
    }

    #[test]
    fn a_different_on_disk_version_is_a_pending_update() {
        let old = loaded(1.0, Some(1.0));
        assert_eq!(
            classify(true, Some(&old), true, Some(2.0), None),
            ExtensionStatus::UpdatePending
        );
    }

    #[test]
    fn error_state_carries_gnomes_first_error() {
        let broken = loaded(STATE_ERROR, Some(1.0));
        assert_eq!(
            classify(true, Some(&broken), true, Some(1.0), Some("SyntaxError: x")),
            ExtensionStatus::Failed("SyntaxError: x".to_owned())
        );
        assert_eq!(
            classify(true, Some(&broken), true, Some(1.0), None),
            ExtensionStatus::Failed("unknown error".to_owned())
        );
    }

    #[test]
    fn out_of_date_state_is_reported() {
        let stale = loaded(STATE_OUT_OF_DATE, Some(1.0));
        assert_eq!(
            classify(true, Some(&stale), true, Some(1.0), None),
            ExtensionStatus::OutOfDate
        );
    }

    #[test]
    fn inactive_and_initialized_are_disabled() {
        for state in [STATE_INACTIVE, STATE_INITIALIZED] {
            let off = loaded(state, Some(1.0));
            assert_eq!(
                classify(true, Some(&off), true, Some(1.0), None),
                ExtensionStatus::Disabled
            );
        }
    }

    #[test]
    fn active_and_transitional_states_are_active() {
        for state in [1.0, 5.0, 7.0, 8.0] {
            let on = loaded(state, Some(1.0));
            assert_eq!(
                classify(true, Some(&on), true, Some(1.0), None),
                ExtensionStatus::Active
            );
        }
    }

    #[test]
    fn only_a_disabled_extension_offers_the_enable_action() {
        assert_eq!(
            notice_for(&ExtensionStatus::Disabled),
            Some((
                "Pasta needs its GNOME extension to see your clipboard.".to_owned(),
                Some(CaptureFixAction::EnableGnomeExtension)
            ))
        );
        for status in [
            ExtensionStatus::ExtensionsDisabled,
            ExtensionStatus::NeedsLogout,
            ExtensionStatus::NotInstalled,
            ExtensionStatus::UpdatePending,
            ExtensionStatus::Failed("x".to_owned()),
            ExtensionStatus::OutOfDate,
        ] {
            let (_, action) = notice_for(&status).expect("a notice");
            assert_eq!(action, None, "{status:?}");
        }
    }

    #[test]
    fn notice_texts_match_the_spec() {
        let text = |status| notice_for(&status).map(|(text, _)| text);
        assert_eq!(
            text(ExtensionStatus::ExtensionsDisabled).as_deref(),
            Some(
                "Extensions are turned off in GNOME. Turn them on in the Extensions app so Pasta can see your clipboard."
            )
        );
        assert_eq!(
            text(ExtensionStatus::NeedsLogout).as_deref(),
            Some("Log out and back in to finish installing Pasta's GNOME extension.")
        );
        assert_eq!(
            text(ExtensionStatus::NotInstalled).as_deref(),
            Some("Pasta's GNOME extension isn't installed. Reinstall Pasta to add it.")
        );
        assert_eq!(
            text(ExtensionStatus::UpdatePending).as_deref(),
            Some("Log out and back in to finish updating Pasta's GNOME extension.")
        );
        assert_eq!(
            text(ExtensionStatus::Failed("boom".to_owned())).as_deref(),
            Some("Pasta's GNOME extension failed to load: boom.")
        );
        assert_eq!(
            text(ExtensionStatus::OutOfDate).as_deref(),
            Some("This GNOME version isn't supported by Pasta's extension yet.")
        );
        assert_eq!(text(ExtensionStatus::Active), None);
    }
}
