use crate::applog;
use crate::state::AppState;
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Rect, UserAttentionType, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

/// Window creation must not happen on the event-loop thread that delivers tray
/// and shortcut events, so every entry point hands the work to a new thread.
pub fn run_off_event_loop(app: &AppHandle, work: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || work(&app));
}

pub fn show_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("popup") {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.request_user_attention(Some(UserAttentionType::Informational));
        // The placeholder may have gone stale while the popup waited unanswered.
        let _ = window.emit("refresh-context", ());
        return;
    }

    let built = WebviewWindowBuilder::new(app, "popup", WebviewUrl::App("index.html".into()))
        .title("What did you just do?")
        .inner_size(440.0, 360.0)
        .min_inner_size(320.0, 240.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .closable(false)
        .center()
        .focused(true)
        .build();

    match built {
        Ok(window) => {
            let _ = window.set_focus();
            let _ = window.request_user_attention(Some(UserAttentionType::Informational));
        }
        Err(error) => applog::error(&format!("Cannot open the check-in popup: {error}")),
    }
}

pub fn show_tray_hint(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(
            "Hourly is set up and now runs in the background.\n\n\
             Its icon is in the system tray at the right end of the taskbar. On \
             Windows 11 it may sit behind the ^ arrow: drag it onto the taskbar to \
             keep it in view. Click it to see your entries.\n\n\
             Once every hour of activity Hourly asks what you did. Press \
             Ctrl+Shift+L to write an entry at any time.",
        )
        .title("Hourly is running")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

pub fn close_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("popup") {
        // destroy() skips the close-request guard that keeps the popup open.
        let _ = window.destroy();
    }
}

pub fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    // Starts hidden: the page measures its own height, resizes the window and
    // then shows it (see settings.js), so the window never visibly jumps.
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Settings")
        .inner_size(500.0, 480.0)
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .center()
        .visible(false)
        .build();

    match built {
        Ok(window) => {
            // Safety net: if the page's script fails, the window must still appear.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                if !window.is_visible().unwrap_or(true) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            });
        }
        Err(error) => applog::error(&format!("Cannot open settings: {error}")),
    }
}

pub fn toggle_dashboard(app: &AppHandle, tray_icon: Option<Rect>) {
    if let Some(window) = app.get_webview_window("dashboard") {
        let _ = window.close();
        return;
    }

    // Clicking the tray icon while the flyout is open first takes focus away
    // from it (closing it), then arrives here. Without this check the click
    // that meant "close" would immediately open it again.
    let closed_just_now = app
        .state::<AppState>()
        .lock()
        .dashboard_closed_at
        .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
    if closed_just_now {
        return;
    }

    let built =
        WebviewWindowBuilder::new(app, "dashboard", WebviewUrl::App("dashboard.html".into()))
            .title("Hourly")
            .inner_size(380.0, 520.0)
            .resizable(true)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .visible(false)
            .focused(true)
            .build();

    let window = match built {
        Ok(window) => window,
        Err(error) => {
            applog::error(&format!("Cannot open the dashboard: {error}"));
            return;
        }
    };
    place_near_tray(app, &window, tray_icon);
    let _ = window.show();
    let _ = window.set_focus();
}

/// Puts the flyout just above the tray icon (or below it when the taskbar is at
/// the top of the screen) and keeps it fully on the monitor.
fn place_near_tray(app: &AppHandle, window: &WebviewWindow, tray_icon: Option<Rect>) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(size) = window.outer_size() else {
        return;
    };
    let (width, height) = (f64::from(size.width), f64::from(size.height));
    let gap = 8.0 * scale;

    let (x, y) = match tray_icon {
        Some(icon) => {
            let position = icon.position.to_physical::<f64>(scale);
            let icon_size = icon.size.to_physical::<f64>(scale);
            let mut x = position.x + icon_size.width / 2.0 - width / 2.0;
            let mut y = position.y - height - gap;
            let monitor = app
                .monitor_from_point(position.x, position.y)
                .ok()
                .flatten()
                .or_else(|| app.primary_monitor().ok().flatten());
            if let Some(monitor) = monitor {
                let (left, top) = (f64::from(monitor.position().x), f64::from(monitor.position().y));
                let (mw, mh) = (f64::from(monitor.size().width), f64::from(monitor.size().height));
                x = x.clamp(left, (left + mw - width).max(left));
                if y < top {
                    y = position.y + icon_size.height + gap;
                }
                y = y.clamp(top, (top + mh - height).max(top));
            }
            (x, y)
        }
        // No icon position (second launch, keyboard): bottom-right corner,
        // where the Windows tray normally is.
        None => match app.primary_monitor().ok().flatten() {
            Some(monitor) => {
                let (left, top) = (f64::from(monitor.position().x), f64::from(monitor.position().y));
                let (mw, mh) = (f64::from(monitor.size().width), f64::from(monitor.size().height));
                (left + mw - width - gap, top + mh - height - 56.0 * scale)
            }
            None => return,
        },
    };
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}
