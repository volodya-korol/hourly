mod applog;
mod checkin;
mod commands;
mod config;
mod export;
mod journal;
mod power;
mod state;
mod store;
mod template;
mod ui;
mod updater;

use state::{dev_log, dev_show_request, is_dev, AppState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Rect, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Global shortcut for "record now" (same as the Electron version).
const RECORD_SHORTCUT: &str = "CommandOrControl+Shift+L";

/// Screen position of the tray icon, remembered from its last mouse event so
/// the dashboard can also open next to it from the tray's context menu.
static LAST_TRAY_ICON: Mutex<Option<Rect>> = Mutex::new(None);
static DASHBOARD_WAS_FOCUSED: AtomicBool = AtomicBool::new(false);

fn last_tray_icon() -> Option<Rect> {
    LAST_TRAY_ICON.lock().ok().and_then(|rect| *rect)
}

fn remember_tray_icon(rect: Rect) {
    if let Ok(mut slot) = LAST_TRAY_ICON.lock() {
        *slot = Some(rect);
    }
}

/// Closes the dashboard when a click elsewhere takes the focus away.
///
/// The web view briefly takes focus from its own window when the window is
/// created, which arrives as "focus lost, focus gained" within a few
/// milliseconds. So a loss of focus is only acted on after a short wait, and
/// only if the window still does not have the focus then.
fn on_dashboard_focus(window: &tauri::Window, focused: bool) {
    dev_log(&format!("dashboard focus changed: {focused}"));
    if focused {
        DASHBOARD_WAS_FOCUSED.store(true, Ordering::SeqCst);
        return;
    }
    if !DASHBOARD_WAS_FOCUSED.load(Ordering::SeqCst) {
        return;
    }
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        if window.is_focused().unwrap_or(false) {
            return;
        }
        DASHBOARD_WAS_FOCUSED.store(false, Ordering::SeqCst);
        window.state::<AppState>().lock().dashboard_closed_at = Some(Instant::now());
        let _ = window.close();
    });
}

/// "Record now". Entries are always saved on this computer, so this never needs
/// anything to be set up first.
fn open_check_in(app: &AppHandle) {
    ui::show_popup(app);
}

/// Shown once, the first time Settings is closed. Windows 11 often puts a new
/// tray icon behind the "^" arrow, and a person who closes Settings then has no
/// sign that anything is running.
fn show_tray_hint_once(app: &AppHandle) {
    let marker = config::data_dir().join("tray-hint.shown");
    if marker.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(config::data_dir());
    let _ = std::fs::write(&marker, b"");
    ui::show_tray_hint(app);
}

/// Autostart is switched on once, on the first launch of an installed build. A
/// user who later turns it off must not have it switched back on.
///
/// The "already done" marker lives next to the program, not in the data folder.
/// Uninstalling removes the program folder and also the Windows start-up entry, so
/// a fresh install starts again from "first launch" and switches it on. A marker
/// in the data folder (which uninstalling keeps) would leave it off for good.
fn enable_autostart_once(app: &AppHandle) {
    let Some(marker) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("autostart.set")))
    else {
        return;
    };
    if marker.exists() {
        return;
    }
    let launcher = app.autolaunch();
    if launcher.is_enabled().unwrap_or(false) || launcher.enable().is_ok() {
        applog::info("Start with Windows switched on (first launch of this install)");
        if let Err(error) = std::fs::write(&marker, b"") {
            applog::warn(&format!("Could not write {}: {error}", marker.display()));
        }
    } else {
        applog::warn("Could not switch on start with Windows");
    }
}

/// Development only: `--dev --show=settings` (or `dashboard`) opens that window
/// at start-up, so it can be looked at without clicking through the tray.
fn open_requested_window(app: &AppHandle) {
    match dev_show_request().as_deref() {
        Some("settings") => ui::run_off_event_loop(app, ui::open_settings),
        Some("dashboard") => ui::run_off_event_loop(app, |app| ui::toggle_dashboard(app, None)),
        Some("popup") => ui::run_off_event_loop(app, ui::show_popup),
        _ => {}
    }
}

fn spawn_update_checks(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        loop {
            tauri::async_runtime::block_on(updater::check_and_download(app.clone(), false));
            std::thread::sleep(Duration::from_secs(6 * 60 * 60));
        }
    });
}

pub fn run() {
    applog::install_panic_hook();
    applog::info(&format!(
        "Hourly {} starting{}",
        env!("CARGO_PKG_VERSION"),
        if is_dev() { " (development mode)" } else { "" }
    ));
    // Asked before `load`, which creates the settings file.
    let first_run = config::first_run();
    let settings = config::load();
    if first_run {
        // So that the next launch is not "the first" again if Settings is closed
        // without changing anything.
        let _ = config::save(&settings);
    }
    store::ensure_initialized(&settings);
    let state = AppState::new(settings);
    state.lock().welcome = first_run;

    let mut builder = tauri::Builder::default();
    // A second launch (login autostart plus a manual start) must not create a
    // second tray icon: show the dashboard of the running app instead. A run
    // with its own HOURLY_DATA_DIR (tests, experiments) is a separate world and
    // must neither hand over to the real app nor be handed over to.
    if std::env::var_os("HOURLY_DATA_DIR").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            ui::run_off_event_loop(app, |app| ui::toggle_dashboard(app, last_tray_icon()));
        }));
    }
    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        ui::run_off_event_loop(app, open_check_in);
                    }
                })
                .build(),
        )
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_context,
            commands::submit_note,
            commands::skip_note,
            commands::record_now,
            commands::open_settings,
            commands::get_dashboard_data,
            commands::get_tick_state,
            commands::get_settings,
            commands::save_settings,
            commands::preview_path,
            commands::template_presets,
            commands::infer_template,
            commands::get_autostart,
            commands::set_autostart,
            commands::open_log,
            commands::open_help,
            commands::open_external,
            commands::entry_stats,
            commands::reveal_entries,
            commands::take_welcome,
            commands::export_text,
            commands::export_to_file,
        ])
        .on_window_event(|window, event| match (window.label(), event) {
            // The popup only goes away by saving or skipping.
            ("popup", WindowEvent::CloseRequested { api, .. }) => api.prevent_close(),
            // The dashboard behaves like a flyout: a click anywhere else closes it.
            ("dashboard", WindowEvent::Focused(focused)) => on_dashboard_focus(window, *focused),
            ("dashboard", WindowEvent::Destroyed) => {
                DASHBOARD_WAS_FOCUSED.store(false, Ordering::SeqCst)
            }
            ("settings", WindowEvent::Destroyed) => show_tray_hint_once(window.app_handle()),
            _ => {}
        })
        .setup(move |app| {
            let handle = app.handle();

            let dashboard = MenuItem::with_id(handle, "dashboard", "Dashboard", true, None::<&str>)?;
            let record = MenuItem::with_id(handle, "record", "Record now", true, None::<&str>)?;
            let settings = MenuItem::with_id(handle, "settings", "Settings", true, None::<&str>)?;
            let help = MenuItem::with_id(handle, "help", "Help", true, None::<&str>)?;
            let update = MenuItem::with_id(handle, updater::MENU_ID, updater::idle_label(), true, None::<&str>)?;
            let quit = MenuItem::with_id(handle, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                handle,
                &[
                    &dashboard,
                    &record,
                    &settings,
                    &help,
                    &PredefinedMenuItem::separator(handle)?,
                    &update,
                    &quit,
                ],
            )?;
            app.manage(updater::UpdateState::new(update));

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("no window icon").clone())
                .tooltip("Hourly")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "dashboard" => {
                        ui::run_off_event_loop(app, |app| ui::toggle_dashboard(app, last_tray_icon()))
                    }
                    "record" => ui::run_off_event_loop(app, open_check_in),
                    "settings" => ui::run_off_event_loop(app, ui::open_settings),
                    "help" => {
                        let _ = commands::open_help(app.clone());
                    }
                    updater::MENU_ID => {
                        if !updater::install_ready_update(app, false) {
                            let app = app.clone();
                            tauri::async_runtime::spawn(updater::check_and_download(app, true));
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Enter { rect, .. } | TrayIconEvent::Move { rect, .. } => {
                        remember_tray_icon(rect);
                    }
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } => {
                        dev_log(&format!("tray click, icon rect: {rect:?}"));
                        remember_tray_icon(rect);
                        ui::run_off_event_loop(tray.app_handle(), move |app| {
                            ui::toggle_dashboard(app, Some(rect))
                        });
                    }
                    _ => {}
                })
                .build(handle)?;

            match app.global_shortcut().register(RECORD_SHORTCUT) {
                Ok(()) => {}
                Err(error) => {
                    applog::warn(&format!("Shortcut {RECORD_SHORTCUT} is unavailable: {error}"))
                }
            }

            {
                let handle = handle.clone();
                power::start(move |event| checkin::on_power_event(&handle, event));
            }
            checkin::spawn_timer(handle.clone());

            ui::run_off_event_loop(handle, checkin::start_checkin_loop);
            if first_run {
                // Offer the optional notes folder; recording works without it.
                ui::run_off_event_loop(handle, ui::open_settings);
            }

            if !is_dev() && !cfg!(debug_assertions) {
                enable_autostart_once(handle);
                spawn_update_checks(handle.clone());
            }
            if is_dev() {
                open_requested_window(handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Hourly");

    app.run(|_app, event| {
        // Having no window open is the normal state of a tray app. Only the
        // explicit exit code 0 from "Quit" (or an update) may end the process.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
