//! Self-update. A new version is downloaded in the background and installed by
//! itself as soon as no check-in popup is open (a half-written entry in it would
//! be lost when the app restarts). The Windows installer starts Hourly again
//! after it has finished. The tray item "Restart to update" installs at once.

use crate::applog;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const MENU_ID: &str = "update";
const IDLE_LABEL: &str = "Check for updates";

/// One waiting installer at a time.
static WAITING_TO_INSTALL: AtomicBool = AtomicBool::new(false);

pub struct UpdateState {
    pub menu_item: MenuItem<Wry>,
    ready: Mutex<Option<(Update, Vec<u8>)>>,
}

impl UpdateState {
    pub fn new(menu_item: MenuItem<Wry>) -> Self {
        Self {
            menu_item,
            ready: Mutex::new(None),
        }
    }

    fn has_ready(&self) -> bool {
        self.ready.lock().map(|r| r.is_some()).unwrap_or(false)
    }
}

fn tell_user(app: &AppHandle, kind: MessageDialogKind, message: &str) {
    app.dialog()
        .message(message)
        .title("Hourly")
        .kind(kind)
        .show(|_| {});
}

/// The app restarts to install, so a check-in popup with a half-written entry
/// must not be open.
fn safe_to_restart(app: &AppHandle) -> bool {
    app.get_webview_window("popup").is_none()
}

/// `manual` is true when the user asked from the tray menu, so silence
/// ("already up to date", "check failed") is answered with a message.
pub async fn check_and_download(app: AppHandle, manual: bool) {
    let state = app.state::<UpdateState>();
    if state.has_ready() {
        return;
    }

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(error) => {
            applog::error(&format!("Update check could not start: {error}"));
            if manual {
                tell_user(&app, MessageDialogKind::Error, &format!("Update check failed: {error}"));
            }
            return;
        }
    };

    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            applog::info(&format!("Update check: version {version} is available, downloading"));
            match update.download(|_, _| {}, || {}).await {
                Ok(bytes) => {
                    applog::info(&format!("Version {version} downloaded ({} bytes)", bytes.len()));
                    let _ = state
                        .menu_item
                        .set_text(format!("Restart to update to v{version}"));
                    if let Ok(mut ready) = state.ready.lock() {
                        *ready = Some((update, bytes));
                    }
                    if manual && !safe_to_restart(&app) {
                        tell_user(
                            &app,
                            MessageDialogKind::Info,
                            &format!("Version {version} is ready. It will be installed as soon as you finish the check-in."),
                        );
                    }
                    install_when_idle(app.clone());
                }
                Err(error) => {
                    applog::error(&format!("Downloading version {version} failed: {error}"));
                    if manual {
                        tell_user(&app, MessageDialogKind::Error, &format!("Update download failed: {error}"));
                    }
                }
            }
        }
        Ok(None) => {
            applog::info("Update check: already on the latest version");
            if manual {
                tell_user(&app, MessageDialogKind::Info, "You are on the latest version.");
            }
        }
        Err(error) => {
            applog::warn(&format!("Update check failed: {error}"));
            if manual {
                tell_user(&app, MessageDialogKind::Error, &format!("Update check failed: {error}"));
            }
        }
    }
}

/// Installs the downloaded update as soon as it is safe, checking every few
/// seconds while a popup is open. When the install works the app exits here and
/// the installer starts the new version; if this thread survives, it failed.
fn install_when_idle(app: AppHandle) {
    if WAITING_TO_INSTALL.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            if !app.state::<UpdateState>().has_ready() {
                break;
            }
            if safe_to_restart(&app) {
                applog::info("Installing the downloaded update");
                install_ready_update(&app, true);
                break;
            }
            std::thread::sleep(Duration::from_secs(5));
        }
        WAITING_TO_INSTALL.store(false, Ordering::SeqCst);
    });
}

/// Runs the downloaded installer (the app exits so it can be replaced).
/// Returns false when nothing has been downloaded yet. `automatic` keeps a
/// failure quiet (it goes to the log); a click on the tray item gets a message.
pub fn install_ready_update(app: &AppHandle, automatic: bool) -> bool {
    let state = app.state::<UpdateState>();
    let taken = state.ready.lock().ok().and_then(|mut r| r.take());
    let Some((update, bytes)) = taken else {
        return false;
    };
    if let Err(error) = update.install(bytes) {
        applog::error(&format!("Installing the update failed: {error}"));
        let _ = state.menu_item.set_text(IDLE_LABEL);
        if !automatic {
            tell_user(app, MessageDialogKind::Error, &format!("Update failed: {error}"));
        }
    }
    true
}

pub fn idle_label() -> &'static str {
    IDLE_LABEL
}
