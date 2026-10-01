use crate::applog;
use crate::config::{self, Config, DEFAULT_MARKER, DEFAULT_PATH_TEMPLATE, MAX_MIN_LENGTH};
use crate::journal::{self, day_at_offset, Entry};
use crate::state::{active_hour_ms, save_skip_date, AppState, DASHBOARD_DAYS};
use crate::store::{self, Stored};
use crate::{export, template, ui};
use chrono::Local;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

/// Where the "Help" button leads: a plain email + message form.
const FEEDBACK_URL: &str = "https://hourly.korol.app/feedback";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    last_entry_at: Option<String>,
    last_entry_text: Option<String>,
    can_skip: bool,
    min_length: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickState {
    active_accumulated_ms: u64,
    active_hour_ms: u64,
    paused: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardData {
    entries: Vec<Entry>,
    #[serde(flatten)]
    tick: TickState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    today: String,
    tomorrow: String,
    next_month: String,
}

fn tick_state(state: &AppState) -> TickState {
    let s = state.lock();
    TickState {
        active_accumulated_ms: s.active_ms,
        active_hour_ms: active_hour_ms(),
        paused: s.paused,
    }
}

#[tauri::command]
pub fn get_context(state: State<'_, AppState>) -> Context {
    let s = state.lock();
    Context {
        last_entry_at: s.last_entry_at.map(|at| at.to_rfc3339()),
        last_entry_text: s.last_entry_text.clone(),
        can_skip: s.can_skip_today(),
        min_length: s.config.min_length,
    }
}

#[tauri::command]
pub fn submit_note(app: AppHandle, state: State<'_, AppState>, text: String) -> Result<(), String> {
    let now = Local::now();
    // The note is written without holding the state lock: file access can be
    // slow (a synced folder) and the activity timer needs the same lock.
    let config = state.lock().config.clone();
    // The popup already disables Save below the minimum; this is the same rule
    // enforced where it cannot be bypassed.
    let length = text.trim().chars().count();
    if length == 0 {
        return Err("Write something first".into());
    }
    if length < config.min_length as usize {
        return Err(format!(
            "Write at least {} characters (you have {length})",
            config.min_length
        ));
    }
    // The copy on this computer comes first and is the one that must succeed.
    if let Err(error) = store::append(&store::path(), &Stored::new(now, &text)) {
        applog::error(&format!("Saving an entry on this computer failed: {error}"));
        return Err(format!("Cannot save the entry on this computer: {error}"));
    }
    {
        let mut s = state.lock();
        s.last_entry_at = Some(now);
        s.last_entry_text = Some(text.trim().to_string());
        s.last_resolved_at = Some(now);
        s.active_ms = 0;
    }
    copy_to_notes_folder(&app, &state, &config, &now, &text);
    ui::close_popup(&app);
    Ok(())
}

/// The second copy, into the daily note, when a notes folder is chosen. The entry
/// is already safe on this computer, so a failure here never loses it: it is
/// logged and the user is told once (not once an hour while the folder is gone).
fn copy_to_notes_folder(
    app: &AppHandle,
    state: &State<'_, AppState>,
    config: &Config,
    now: &chrono::DateTime<Local>,
    text: &str,
) {
    if config.journal_root.as_deref().is_none_or(|root| root.trim().is_empty()) {
        return;
    }
    match journal::append_entry(config, now, text) {
        Ok(()) => {
            state.lock().notes_copy_failing = false;
        }
        Err(error) => {
            applog::error(&format!("Copying an entry to the notes folder failed: {error}"));
            let already_told = std::mem::replace(&mut state.lock().notes_copy_failing, true);
            if !already_told {
                app.dialog()
                    .message(format!(
                        "Your entry is saved on this computer, but Hourly could not add it \
                         to your notes folder:\n\n{error}\n\nCheck the folder in Settings. \
                         Nothing is lost: you can export all your entries at any time."
                    ))
                    .title("Hourly")
                    .kind(MessageDialogKind::Warning)
                    .show(|_| {});
            }
        }
    }
}

/// One skip per calendar day: a free pass for a meeting or a deep-work block,
/// not a loophole. The notes are not touched (nothing was written), but the
/// timer restarts so the popup does not come straight back.
#[tauri::command]
pub fn skip_note(app: AppHandle, state: State<'_, AppState>) {
    {
        let mut s = state.lock();
        if !s.can_skip_today() {
            return;
        }
        let today = crate::state::day_key();
        save_skip_date(&today);
        s.last_skip_date = Some(today);
        s.last_resolved_at = Some(Local::now());
        s.active_ms = 0;
    }
    ui::close_popup(&app);
}

#[tauri::command]
pub async fn record_now(app: AppHandle) {
    ui::show_popup(&app);
}

#[tauri::command]
pub async fn open_settings(app: AppHandle) {
    ui::open_settings(&app);
}

/// The tray list comes from the copy on this computer, not from the notes folder,
/// so it is the same whether or not a folder is chosen.
#[tauri::command]
pub fn get_dashboard_data(state: State<'_, AppState>) -> DashboardData {
    DashboardData {
        entries: store::recent(&store::read_all(&store::path()), DASHBOARD_DAYS),
        tick: tick_state(&state),
    }
}

#[tauri::command]
pub fn get_tick_state(state: State<'_, AppState>) -> TickState {
    tick_state(&state)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Config {
    state.lock().config.clone()
}

/// Settings save themselves as you type, so this is called often. The notes
/// folder is optional: an empty one means entries are kept only on this computer.
/// A newly chosen folder is tested for write access (with a throw-away file).
#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Config) -> Result<Config, String> {
    let root = settings.journal_root.clone().unwrap_or_default();
    let root = root.trim();
    let template = non_blank(&settings.path_template, DEFAULT_PATH_TEMPLATE);
    let marker = non_blank(&settings.marker, DEFAULT_MARKER);
    journal::render_relative(&template, &Local::now())?;
    if settings.min_length > MAX_MIN_LENGTH {
        return Err(format!("The minimum length can be 0 to {MAX_MIN_LENGTH} characters"));
    }

    let (root_changed, previous) = {
        let s = state.lock();
        (
            !root.is_empty() && s.config.journal_root.as_deref() != Some(root),
            s.config.clone(),
        )
    };
    if root_changed {
        if let Err(error) = check_writable(root) {
            applog::warn(&format!("Folder refused ({root}): {error}"));
            return Err(error);
        }
    }

    let saved = Config {
        journal_root: (!root.is_empty()).then(|| root.to_string()),
        path_template: template,
        marker,
        min_length: settings.min_length,
    };
    if let Err(error) = config::save(&saved) {
        applog::error(&format!("Saving the settings failed: {error}"));
        return Err(error);
    }
    state.lock().config = saved.clone();
    // Settings save while typing; only real changes are worth a line in the log.
    if saved != previous {
        applog::info(&format!(
            "Settings saved: notes folder {}, layout {}, minimum {} characters",
            saved.journal_root.as_deref().unwrap_or("(none)"),
            saved.path_template,
            saved.min_length
        ));
    }
    Ok(saved)
}

fn non_blank(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

fn check_writable(root: &str) -> Result<(), String> {
    let dir = std::path::Path::new(root);
    std::fs::create_dir_all(dir).map_err(|_| "Cannot write to that folder".to_string())?;
    let probe = dir.join(".hourly-write-test");
    std::fs::write(&probe, b"").map_err(|_| "Cannot write to that folder".to_string())?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

/// Live preview of where today's and tomorrow's notes will go. Works on the
/// values in the form, which may not be saved yet.
#[tauri::command]
pub fn preview_path(
    journal_root: String,
    path_template: String,
) -> Result<Option<Preview>, String> {
    let root = journal_root.trim();
    if root.is_empty() {
        return Ok(None);
    }
    let template = non_blank(&path_template, DEFAULT_PATH_TEMPLATE);
    let render = |day: chrono::DateTime<Local>| -> Result<String, String> {
        let relative = journal::render_relative(&template, &day)?;
        Ok(journal::join_relative(root, &relative)?
            .to_string_lossy()
            .into_owned())
    };
    Ok(Some(Preview {
        today: render(day_at_offset(0))?,
        tomorrow: render(day_at_offset(1))?,
        next_month: render(journal::one_month_ahead())?,
    }))
}

/// The ready-made note locations offered in Settings.
#[tauri::command]
pub fn template_presets() -> Vec<template::Preset> {
    template::presets()
}

/// Works out the template from one of the user's own daily notes.
#[tauri::command]
pub fn infer_template(journal_root: String, note_path: String) -> Result<String, String> {
    template::infer(&journal_root, &note_path)
}

/// Whether Hourly starts when the user signs in to Windows. The answer comes
/// from Windows itself (the "Run" registry entry), not from a copy in the
/// settings file that could disagree with it.
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("Cannot read the start-up setting: {e}"))
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    match result {
        Ok(()) => {
            applog::info(&format!("Start with Windows switched {}", if enabled { "on" } else { "off" }));
            Ok(())
        }
        Err(error) => {
            applog::error(&format!("Changing start with Windows failed: {error}"));
            Err(format!("Cannot change the start-up setting: {error}"))
        }
    }
}

#[derive(Serialize)]
pub struct EntryStats {
    count: usize,
    since: Option<String>,
}

/// How many entries are kept on this computer, for the line in Settings.
#[tauri::command]
pub fn entry_stats() -> EntryStats {
    let entries = store::read_all(&store::path());
    EntryStats {
        count: entries.len(),
        since: entries
            .first()
            .and_then(Stored::wall_clock)
            .map(|at| at.format("%Y-%m-%d").to_string()),
    }
}

/// Shows the entries file in Explorer, so people can see that it is theirs.
#[tauri::command]
pub fn reveal_entries(app: AppHandle) -> Result<(), String> {
    let path = store::path();
    let target = if path.exists() { path } else { config::data_dir() };
    app.opener()
        .reveal_item_in_dir(target)
        .map_err(|e| format!("Cannot open the folder: {e}"))
}

/// True once, on the very first launch: Settings then opens on the notes folder.
#[tauri::command]
pub fn take_welcome(state: State<'_, AppState>) -> bool {
    std::mem::replace(&mut state.lock().welcome, false)
}

#[derive(Serialize)]
pub struct ExportResult {
    text: String,
    count: usize,
}

fn build_export(
    format: &str,
    range: &str,
    with_instruction: bool,
) -> Result<export::Exported, String> {
    let format = export::Format::parse(format)?;
    let range = export::Range::parse(range)?;
    let entries = store::read_all(&store::path());
    let exported = export::export(&entries, format, range, with_instruction, Local::now());
    if exported.count == 0 {
        return Err("There are no entries in that period".into());
    }
    Ok(exported)
}

/// The export as text, for copying to the clipboard.
#[tauri::command]
pub fn export_text(format: String, range: String, with_prompt: bool) -> Result<ExportResult, String> {
    let exported = build_export(&format, &range, with_prompt)?;
    applog::info(&format!("Export copied: {} entries as {format}", exported.count));
    Ok(ExportResult { text: exported.text, count: exported.count })
}

/// The export written to a file the user picked in the save dialog.
#[tauri::command]
pub fn export_to_file(
    path: String,
    format: String,
    range: String,
    with_prompt: bool,
) -> Result<usize, String> {
    let exported = build_export(&format, &range, with_prompt)?;
    std::fs::write(&path, exported.text).map_err(|e| format!("Cannot write the file: {e}"))?;
    applog::info(&format!("Export saved: {} entries as {format} to {path}", exported.count));
    Ok(exported.count)
}

/// Shows the log file in Explorer, so a person can attach it to a bug report.
#[tauri::command]
pub fn open_log(app: AppHandle) -> Result<(), String> {
    applog::info("Log opened from Settings");
    app.opener()
        .reveal_item_in_dir(applog::path())
        .map_err(|e| format!("Cannot open the log folder: {e}"))
}

#[tauri::command]
pub fn open_help(app: AppHandle) -> Result<(), String> {
    let url = format!("{FEEDBACK_URL}?v={}", app.package_info().version);
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only https links can be opened".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
