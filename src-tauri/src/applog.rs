//! A small log file, so that a problem can be looked at afterwards. A tray app
//! has no console: without this, "nothing happens" cannot be investigated.
//!
//! It records what the app did and what went wrong (settings saved, the path an
//! entry was written to, update checks, lock and sleep events, panics). The text
//! of an entry is never written to it.

use crate::config;
use chrono::Local;
use std::io::Write;
use std::path::PathBuf;

/// When the log grows past this it becomes `hourly.log.old` and a new one starts,
/// so it can never fill the disk.
const MAX_BYTES: u64 = 512 * 1024;

pub fn path() -> PathBuf {
    config::data_dir().join("hourly.log")
}

fn write(level: &str, message: &str) {
    // Unit tests run the real code, but must never write to the user's own log.
    if cfg!(test) {
        return;
    }
    append_line(&path(), level, message);
}

fn append_line(path: &std::path::Path, level: &str, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} {level:<5} {message}",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
        );
    }
}

pub fn info(message: &str) {
    write("INFO", message);
}

pub fn warn(message: &str) {
    write("WARN", message);
}

pub fn error(message: &str) {
    write("ERROR", message);
}

/// Release builds abort on a panic, which would end the app without a trace.
/// The hook runs first and leaves the reason and place in the log.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()))
            .unwrap_or_else(|| "unknown place".into());
        let payload = info.payload();
        let reason = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".into());
        let thread = std::thread::current();
        write(
            "PANIC",
            &format!("thread '{}' at {place}: {reason}", thread.name().unwrap_or("unnamed")),
        );
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hourly-log-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn lines_are_appended_with_a_level() {
        let dir = scratch("append");
        let file = dir.join("hourly.log");
        append_line(&file, "INFO", "first");
        append_line(&file, "ERROR", "second");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(" INFO  first\n"), "{text}");
        assert!(text.contains(" ERROR second\n"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_big_log_is_rotated_so_it_cannot_grow_forever() {
        let dir = scratch("rotate");
        let file = dir.join("hourly.log");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, vec![b'x'; (MAX_BYTES + 10) as usize]).unwrap();
        append_line(&file, "INFO", "fresh start");
        assert!(dir.join("hourly.log.old").exists());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("fresh start") && text.len() < 200, "{}", text.len());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn running_the_real_code_in_tests_leaves_the_users_log_alone() {
        let before = std::fs::metadata(path()).map(|m| m.len()).ok();
        info("this must not be written");
        assert_eq!(std::fs::metadata(path()).map(|m| m.len()).ok(), before);
    }
}
