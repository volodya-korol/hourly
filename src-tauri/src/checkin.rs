//! The hourly check-in loop: counting real activity and deciding when to ask.

use crate::power::PowerEvent;
use crate::state::{
    active_hour_ms, check_interval_ms, dev_log, dev_show_request, is_dev, load_skip_date, AppState,
};
use crate::{store, ui};
use chrono::{Duration, Local};
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Manager};

/// Runs at start-up. Hourly records with or without a notes folder, so nothing
/// has to be set up first.
pub fn start_checkin_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut s = state.lock();
        // The process restarts at every Windows login, so "last entry" is read
        // back from the entries file instead of being kept only in memory.
        if let Some(latest) = store::read_all(&store::path()).pop() {
            let at = latest.instant();
            s.last_entry_at = at;
            s.last_entry_text = Some(latest.text);
            s.last_resolved_at = at;
        }
        s.last_skip_date = load_skip_date();

        if is_dev() && s.last_entry_at.is_none() {
            let yesterday = Local::now() - Duration::hours(20);
            s.last_entry_at = Some(yesterday);
            s.last_entry_text = Some("Refactored the app: added a once-a-day skip button".into());
            s.last_resolved_at = Some(yesterday);
        }
    }

    if is_dev() {
        // With --show=<window> only that window is wanted, not the popup too.
        if dev_show_request().is_none() {
            ui::show_popup(app);
        }
    } else {
        // The computer may have been off longer than an hour while this process
        // did not exist; starting up is the first chance to notice.
        maybe_show_popup_for_gap(app);
    }
}

/// The activity counter cannot see time when the whole process was gone (shut
/// down, hibernated), so the wall-clock gap since the last check-in is
/// compared too. Called at start-up and after unlock / resume.
pub fn maybe_show_popup_for_gap(app: &AppHandle) {
    let state = app.state::<AppState>();
    let due = {
        let mut s = state.lock();
        let Some(resolved) = s.last_resolved_at else {
            return;
        };
        let gap_ms = (Local::now() - resolved).num_milliseconds();
        if gap_ms >= active_hour_ms() as i64 {
            s.active_ms = 0;
            true
        } else {
            false
        }
    };
    if due {
        ui::show_popup(app);
    }
}

pub fn spawn_timer(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(StdDuration::from_millis(check_interval_ms()));
        let fire = {
            let state = app.state::<AppState>();
            let mut s = state.lock();
            if s.paused {
                false
            } else {
                s.active_ms += check_interval_ms();
                if s.active_ms >= active_hour_ms() {
                    s.active_ms = 0;
                    true
                } else {
                    false
                }
            }
        };
        if fire {
            ui::show_popup(&app);
        }
    });
}

pub fn on_power_event(app: &AppHandle, event: PowerEvent) {
    dev_log(&format!("power event: {event:?}"));
    crate::applog::info(&format!("Windows event: {event:?}"));
    let state = app.state::<AppState>();
    match event {
        PowerEvent::Lock | PowerEvent::Suspend => state.lock().paused = true,
        PowerEvent::Unlock | PowerEvent::Resume => {
            state.lock().paused = false;
            ui::run_off_event_loop(app, maybe_show_popup_for_gap);
        }
    }
}
