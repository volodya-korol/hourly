use crate::applog;
use crate::config::Config;
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Datelike, Days, Local, Months, TimeZone, Timelike};
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub date: String,
    pub time: String,
    pub text: String,
    pub sort_key: String,
}

/// Local time `offset` calendar days from now. Other days are taken at noon so
/// a daylight-saving jump can never move the result onto a different date.
pub fn day_at_offset(offset: i64) -> DateTime<Local> {
    let now = Local::now();
    if offset == 0 {
        return now;
    }
    let base = now.date_naive();
    let target = if offset > 0 {
        base.checked_add_days(Days::new(offset as u64))
    } else {
        base.checked_sub_days(Days::new(offset.unsigned_abs()))
    }
    .unwrap_or(base);
    target
        .and_hms_opt(12, 0, 0)
        .and_then(|naive| Local.from_local_datetime(&naive).earliest())
        .unwrap_or(now)
}

/// The same day of next month (the last day of it when that month is shorter),
/// at noon. Shown in Settings so a folder that never changes is easy to spot.
pub fn one_month_ahead() -> DateTime<Local> {
    let base = Local::now().date_naive();
    base.checked_add_months(Months::new(1))
        .unwrap_or(base)
        .and_hms_opt(12, 0, 0)
        .and_then(|naive| Local.from_local_datetime(&naive).earliest())
        .unwrap_or_else(Local::now)
}

/// Renders a chrono strftime template ("%Y/%B/%Y-%m-%d.md"). Only "%x" sequences
/// are tokens, so folder separators and the ".md" extension are always literal.
pub fn render_relative(template: &str, date: &DateTime<Local>) -> Result<String, String> {
    let items: Vec<Item> = StrftimeItems::new(template).collect();
    if items.iter().any(|item| matches!(item, Item::Error)) {
        return Err("Invalid date format".into());
    }
    Ok(date.format_with_items(items.into_iter()).to_string())
}

pub fn join_relative(root: &str, relative: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::from(root);
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err("The path template cannot contain \"..\"".into()),
            p if p.contains(':') => return Err("The path template cannot contain \":\"".into()),
            p => path.push(p),
        }
    }
    Ok(path)
}

pub fn path_for_date(cfg: &Config, date: &DateTime<Local>) -> Result<PathBuf, String> {
    let root = cfg
        .journal_root
        .as_deref()
        .ok_or("No journal folder selected")?;
    join_relative(root, &render_relative(&cfg.path_template, date)?)
}

fn read_note(path: &Path) -> Result<String, String> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| "The daily note is not valid UTF-8, so it was left untouched".to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("Cannot read the daily note: {e}")),
    }
}

/// The first entry of a day leaves the marker at the end of the note. Every
/// later entry is inserted before the marker, which pushes it further down, so
/// the marker always shows where the next entry will go.
pub fn append_entry(cfg: &Config, now: &DateTime<Local>, text: &str) -> Result<(), String> {
    let path = path_for_date(cfg, now)?;
    let one_line = text.trim().replace("\r\n", " ").replace(['\n', '\r'], " ");
    let entry = format!("{:02}:{:02} - {}\n", now.hour(), now.minute(), one_line);
    let marker = cfg.marker.as_str();

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Cannot create the folder: {e}"))?;
    }
    let mut content = read_note(&path)?;

    if content.contains(marker) {
        content = content.replacen(marker, &format!("{entry}\n{marker}"), 1);
    } else {
        let needs_gap = !content.is_empty() && !content.ends_with("\n\n");
        let separator = match (needs_gap, content.ends_with('\n')) {
            (false, _) => "",
            (true, true) => "\n",
            (true, false) => "\n\n",
        };
        content = format!("{content}{separator}{entry}\n{marker}\n");
    }

    write_note(&path, &content).map_err(|e| format!("Cannot write the daily note: {e}"))?;
    applog::info(&format!("Saved an entry to {}", path.display()));
    Ok(())
}

/// Puts `content` in the note without ever leaving it half written. The new text
/// goes into a temporary file next to the note, which then takes the note's place
/// in one step, so a crash or power cut leaves either the old note or the new one.
/// Windows' `ReplaceFile` is used because it also keeps the note's creation date
/// (Obsidian plugins such as Dataview read it). If the swap is refused (a sync
/// tool or antivirus may hold the note), the note is written in place instead.
fn write_note(path: &Path, content: &str) -> io::Result<()> {
    if !path.exists() {
        return fs::write(path, content);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.hourly-tmp"));
    fs::write(&temp, content)?;
    if let Err(error) = replace_file(&temp, path) {
        let _ = fs::remove_file(&temp);
        applog::warn(&format!("Could not swap in the new note ({error}); writing it in place"));
        return fs::write(path, content);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_file(temp: &Path, target: &Path) -> io::Result<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};

    let replaced = HSTRING::from(target.as_os_str());
    let replacement = HSTRING::from(temp.as_os_str());
    unsafe {
        ReplaceFileW(&replaced, &replacement, PCWSTR::null(), REPLACE_FILE_FLAGS(0), None, None)
    }
    .map_err(io::Error::other)
}

#[cfg(not(windows))]
fn replace_file(temp: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temp, target)
}

/// "HH:MM - text" is how Hourly writes entries; anything else in the note is ignored.
fn parse_entry_line(line: &str) -> Option<(&str, &str)> {
    let b = line.as_bytes();
    if b.len() < 9 {
        return None;
    }
    let looks_right = b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2] == b':'
        && b[3].is_ascii_digit()
        && b[4].is_ascii_digit()
        && &b[5..8] == b" - ";
    if !looks_right {
        return None;
    }
    // The first 8 bytes are ASCII, so byte 8 is always a character boundary.
    Some((&line[..5], &line[8..]))
}

pub fn scan_recent_entries(cfg: &Config, days: i64) -> Vec<Entry> {
    let mut entries = Vec::new();
    if cfg.journal_root.is_none() {
        return entries;
    }
    for i in 0..days {
        let day = day_at_offset(-i);
        let Ok(path) = path_for_date(cfg, &day) else {
            continue;
        };
        let Ok(content) = read_note(&path) else {
            continue;
        };
        let date_label = format!("{:02}.{:02}", day.day(), day.month());
        let iso = format!("{:04}-{:02}-{:02}", day.year(), day.month(), day.day());
        for line in content.lines() {
            if let Some((time, text)) = parse_entry_line(line) {
                entries.push(Entry {
                    date: date_label.clone(),
                    time: time.to_string(),
                    text: text.to_string(),
                    sort_key: format!("{iso}T{time}"),
                });
            }
        }
    }
    entries.sort_by(|a, b| b.sort_key.cmp(&a.sort_key));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 9, 29, h, m, 0).unwrap()
    }

    fn temp_config(tag: &str) -> (Config, PathBuf) {
        let dir = std::env::temp_dir().join(format!("hourly-journal-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            journal_root: Some(dir.to_string_lossy().into_owned()),
            ..Config::default()
        };
        (cfg, dir)
    }

    #[test]
    fn default_template_renders_year_month_name_and_date() {
        let out = render_relative("%Y/%B/%Y-%m-%d.md", &fixed(10, 15)).unwrap();
        assert_eq!(out, "2026/September/2026-09-29.md");
    }

    #[test]
    fn literal_md_and_slashes_can_never_be_mistaken_for_tokens() {
        // The Moment.js version turned ".md" into ".02" and swallowed "\".
        let out = render_relative("Daily\\%d.md", &fixed(1, 1)).unwrap();
        assert_eq!(out, "Daily\\29.md");
    }

    #[test]
    fn an_unknown_token_is_an_error_not_a_panic() {
        assert!(render_relative("%Q", &fixed(1, 1)).is_err());
    }

    #[test]
    fn join_uses_the_native_separator_for_both_slash_styles() {
        let p = join_relative("C:\\vault", "2026/September\\note.md").unwrap();
        assert_eq!(p, PathBuf::from("C:\\vault").join("2026").join("September").join("note.md"));
    }

    #[test]
    fn join_refuses_to_leave_the_vault() {
        assert!(join_relative("C:\\vault", "../x.md").is_err());
        assert!(join_relative("C:\\vault", "D:/x.md").is_err());
    }

    #[test]
    fn first_entry_of_the_day_creates_the_note_with_the_marker() {
        let (cfg, dir) = temp_config("first");
        append_entry(&cfg, &fixed(10, 15), "hello").unwrap();
        let path = path_for_date(&cfg, &fixed(10, 15)).unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "10:15 - hello\n\n<next log here>\n"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn later_entries_go_before_the_marker() {
        let (cfg, dir) = temp_config("later");
        append_entry(&cfg, &fixed(10, 15), "hello").unwrap();
        append_entry(&cfg, &fixed(11, 20), "second").unwrap();
        let path = path_for_date(&cfg, &fixed(10, 15)).unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "10:15 - hello\n\n11:20 - second\n\n<next log here>\n"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_text_in_the_note_is_kept() {
        let (cfg, dir) = temp_config("keep");
        let path = path_for_date(&cfg, &fixed(9, 0)).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# My day\nSome thoughts").unwrap();
        append_entry(&cfg, &fixed(9, 0), "started").unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "# My day\nSome thoughts\n\n09:00 - started\n\n<next log here>\n"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rewriting_a_note_keeps_its_creation_date_and_leaves_no_temporary_file() {
        let (cfg, dir) = temp_config("swap");
        append_entry(&cfg, &fixed(10, 15), "first").unwrap();
        let path = path_for_date(&cfg, &fixed(10, 15)).unwrap();
        let created = fs::metadata(&path).unwrap().created().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1200));
        append_entry(&cfg, &fixed(11, 20), "second").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().created().unwrap(), created);
        assert!(fs::read_to_string(&path).unwrap().contains("11:20 - second"));
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".hourly-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_leftover_temporary_file_from_a_crash_does_not_stop_saving() {
        let (cfg, dir) = temp_config("leftover");
        append_entry(&cfg, &fixed(9, 0), "one").unwrap();
        let path = path_for_date(&cfg, &fixed(9, 0)).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        fs::write(path.with_file_name(format!(".{name}.hourly-tmp")), "half wri").unwrap();
        append_entry(&cfg, &fixed(10, 0), "two").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("09:00 - one") && text.contains("10:00 - two"), "{text}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_note_that_is_not_utf8_is_never_overwritten() {
        let (cfg, dir) = temp_config("utf8");
        let path = path_for_date(&cfg, &fixed(9, 0)).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, [0xff, 0xfe, 0x41]).unwrap();
        assert!(append_entry(&cfg, &fixed(9, 0), "x").is_err());
        assert_eq!(fs::read(path).unwrap(), vec![0xff, 0xfe, 0x41]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn line_breaks_in_the_entry_become_spaces() {
        let (cfg, dir) = temp_config("nl");
        append_entry(&cfg, &fixed(8, 5), "one\r\ntwo\nthree").unwrap();
        let path = path_for_date(&cfg, &fixed(8, 5)).unwrap();
        assert!(fs::read_to_string(path).unwrap().starts_with("08:05 - one two three\n"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_only_lines_written_by_hourly() {
        assert_eq!(parse_entry_line("09:05 - wrote code"), Some(("09:05", "wrote code")));
        assert_eq!(parse_entry_line("9:05 - x"), None);
        assert_eq!(parse_entry_line("09:05 -x"), None);
        assert_eq!(parse_entry_line("09:05 - "), None);
        assert_eq!(parse_entry_line("# heading"), None);
        assert_eq!(parse_entry_line("09:05 - привіт"), Some(("09:05", "привіт")));
    }

    #[test]
    fn scan_reads_today_including_windows_line_endings() {
        let (cfg, dir) = temp_config("scan");
        let today = day_at_offset(0);
        let path = path_for_date(&cfg, &today).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "10:00 - first\r\nnoise\r\n11:30 - second\r\n").unwrap();
        let found = scan_recent_entries(&cfg, 3);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "second");
        assert_eq!(found[1].text, "first");
        let _ = fs::remove_dir_all(dir);
    }
}
