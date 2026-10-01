use crate::config::{self, Config};
use chrono::{DateTime, Local};
use std::io::Write;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

pub const DASHBOARD_DAYS: i64 = 30;

/// `--dev`: the popup comes every 20 seconds instead of every hour, and
/// autostart and the update check are switched off.
pub fn is_dev() -> bool {
    std::env::args().any(|a| a == "--dev")
}

/// Development only: `--show=settings|dashboard|popup` opens just that window.
pub fn dev_show_request() -> Option<String> {
    if !is_dev() {
        return None;
    }
    std::env::args().find_map(|arg| arg.strip_prefix("--show=").map(str::to_string))
}

pub fn active_hour_ms() -> u64 {
    if is_dev() {
        20_000
    } else {
        60 * 60 * 1000
    }
}

pub fn check_interval_ms() -> u64 {
    if is_dev() {
        1_000
    } else {
        30_000
    }
}

pub fn day_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub struct Inner {
    pub config: Config,
    /// Milliseconds of real activity since the last check-in.
    pub active_ms: u64,
    /// True while the screen is locked or the computer sleeps.
    pub paused: bool,
    pub last_entry_at: Option<DateTime<Local>>,
    pub last_entry_text: Option<String>,
    /// When the popup was last dealt with (saved OR skipped); the next popup is
    /// due one hour after this.
    pub last_resolved_at: Option<DateTime<Local>>,
    /// Date (YYYY-MM-DD) of the last "Skip" — one skip is allowed per day.
    pub last_skip_date: Option<String>,
    pub dashboard_closed_at: Option<Instant>,
    /// The last copy to the notes folder failed. One warning is shown when a run
    /// of failures starts, not one an hour for as long as the folder is gone.
    pub notes_copy_failing: bool,
    /// True on the very first launch, until Settings has picked it up.
    pub welcome: bool,
}

impl Inner {
    pub fn can_skip_today(&self) -> bool {
        self.last_skip_date.as_deref() != Some(day_key().as_str())
    }
}

pub struct AppState {
    inner: Mutex<Inner>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            inner: Mutex::new(Inner {
                config,
                active_ms: 0,
                paused: false,
                last_entry_at: None,
                last_entry_text: None,
                last_resolved_at: None,
                last_skip_date: None,
                dashboard_closed_at: None,
                notes_copy_failing: false,
                welcome: false,
            }),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn state_path() -> std::path::PathBuf {
    config::data_dir().join("state.json")
}

pub fn load_skip_date() -> Option<String> {
    let text = std::fs::read_to_string(state_path()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("lastSkipDate")?.as_str().map(str::to_string)
}

pub fn save_skip_date(date: &str) {
    let _ = std::fs::create_dir_all(config::data_dir());
    let json = serde_json::json!({ "lastSkipDate": date }).to_string();
    let _ = std::fs::write(state_path(), json);
}

/// Development log (only with `--dev`): lock/unlock/sleep events and tray
/// clicks, so they can be checked by hand on a real desktop session.
pub fn dev_log(message: &str) {
    if !is_dev() {
        return;
    }
    println!("[dev] {message}");
    let _ = std::fs::create_dir_all(config::data_dir());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config::data_dir().join("dev.log"))
    {
        let _ = writeln!(file, "{} {message}", Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}
