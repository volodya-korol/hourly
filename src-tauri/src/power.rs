//! Screen lock and sleep detection.
//!
//! Hourly counts an hour of *real* activity: the clock stops while the screen is
//! locked or the computer sleeps, and it deliberately does NOT stop when the
//! mouse and keyboard are idle (a paused video still counts as "here"). That
//! rules out idle-time APIs such as `GetLastInputInfo`; we need the real session
//! and power notifications, the same ones Electron's `powerMonitor` uses.
//!
//! A hidden message-only window receives:
//! * `WM_WTSSESSION_CHANGE` (lock / unlock), after `WTSRegisterSessionNotification`
//! * `WM_POWERBROADCAST` (suspend / resume), after `RegisterSuspendResumeNotification`

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEvent {
    Lock,
    Unlock,
    Suspend,
    Resume,
}

#[cfg(not(windows))]
pub fn start(_handler: impl Fn(PowerEvent) + Send + Sync + 'static) {}

#[cfg(windows)]
pub use win::start;

#[cfg(windows)]
mod win {
    use super::PowerEvent;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::OnceLock;
    use windows::core::w;
    use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Power::RegisterSuspendResumeNotification;
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, HWND_MESSAGE, MSG, PBT_APMRESUMEAUTOMATIC,
        PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST,
        WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
    };

    type Handler = Box<dyn Fn(PowerEvent) + Send + Sync + 'static>;

    static HANDLER: OnceLock<Handler> = OnceLock::new();
    /// Window handle, published so tests can post synthetic messages to it.
    pub(super) static WINDOW: AtomicIsize = AtomicIsize::new(0);

    fn emit(event: PowerEvent) {
        if let Some(handler) = HANDLER.get() {
            handler(event);
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_WTSSESSION_CHANGE => {
                match wparam.0 as u32 {
                    WTS_SESSION_LOCK => emit(PowerEvent::Lock),
                    WTS_SESSION_UNLOCK => emit(PowerEvent::Unlock),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_POWERBROADCAST => {
                match wparam.0 as u32 {
                    PBT_APMSUSPEND => emit(PowerEvent::Suspend),
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => emit(PowerEvent::Resume),
                    _ => {}
                }
                // Returning TRUE tells Windows the notification was handled.
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn run_message_loop() -> windows::core::Result<()> {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("HourlyPowerEvents"),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("HourlyPowerEvents"),
            w!("Hourly power events"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )?;
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);

        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        // The registration handle stays valid for the life of the process.
        let _ = RegisterSuspendResumeNotification(HANDLE(hwnd.0), DEVICE_NOTIFY_WINDOW_HANDLE)?;

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }

    /// Starts the listener thread. Call once; a second call is ignored.
    pub fn start(handler: impl Fn(PowerEvent) + Send + Sync + 'static) {
        if HANDLER.set(Box::new(handler)).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("power-events".into())
            .spawn(|| {
                if let Err(error) = unsafe { run_message_loop() } {
                    crate::applog::error(&format!("Lock and sleep detection could not start: {error}"));
                }
            })
            .expect("cannot start the power-events thread");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        PostMessageW, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, WM_POWERBROADCAST,
        WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
    };

    /// Proves the window procedure turns Windows messages into events. The real
    /// lock/unlock and sleep notifications are checked by hand on a real session.
    #[test]
    fn windows_messages_become_power_events() {
        let (tx, rx) = mpsc::channel();
        start(move |event| {
            let _ = tx.send(event);
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let hwnd = loop {
            let raw = win::WINDOW.load(Ordering::SeqCst);
            if raw != 0 {
                break HWND(raw as *mut _);
            }
            assert!(Instant::now() < deadline, "message window was never created");
            std::thread::sleep(Duration::from_millis(10));
        };

        unsafe {
            PostMessageW(Some(hwnd), WM_WTSSESSION_CHANGE, WPARAM(WTS_SESSION_LOCK as usize), LPARAM(0)).unwrap();
            PostMessageW(Some(hwnd), WM_WTSSESSION_CHANGE, WPARAM(WTS_SESSION_UNLOCK as usize), LPARAM(0)).unwrap();
            PostMessageW(Some(hwnd), WM_POWERBROADCAST, WPARAM(PBT_APMSUSPEND as usize), LPARAM(0)).unwrap();
            PostMessageW(Some(hwnd), WM_POWERBROADCAST, WPARAM(PBT_APMRESUMEAUTOMATIC as usize), LPARAM(0)).unwrap();
        }

        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(rx.recv_timeout(Duration::from_secs(3)).expect("event not delivered"));
        }
        assert_eq!(
            seen,
            vec![PowerEvent::Lock, PowerEvent::Unlock, PowerEvent::Suspend, PowerEvent::Resume]
        );
    }
}
