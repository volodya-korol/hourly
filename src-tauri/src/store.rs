//! The app's own copy of every entry, kept on this computer whatever the user
//! does about notes folders. It is the source for the tray list, for the "last
//! entry" hint in the popup and for exports. A notes folder (Obsidian or any
//! other) only ever gets an extra copy of each entry.
//!
//! One JSON object per line (`entries.jsonl`): appending is a single small write,
//! a damaged line costs one entry instead of the file, and a person can read it.

use crate::journal::{self, Entry};
use crate::{applog, config};
use chrono::{DateTime, Datelike, Days, Local, NaiveDateTime, SecondsFormat, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// How far back entries are picked up from an existing notes folder the first
/// time the app runs with this store.
const IMPORT_DAYS: i64 = 365;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    /// RFC 3339 with the offset in force when the entry was written, for example
    /// `2026-09-30T08:51:23+02:00`.
    pub at: String,
    pub text: String,
}

impl Stored {
    pub fn new(at: DateTime<Local>, text: &str) -> Self {
        Self {
            at: at.to_rfc3339_opts(SecondsFormat::Secs, false),
            text: one_line(text),
        }
    }

    /// The clock on the wall when the entry was written. It is deliberately not
    /// converted to the current time zone, so an entry keeps the time it was
    /// written at after the user travels.
    pub fn wall_clock(&self) -> Option<NaiveDateTime> {
        DateTime::parse_from_rfc3339(&self.at)
            .ok()
            .map(|at| at.naive_local())
    }

    pub fn instant(&self) -> Option<DateTime<Local>> {
        DateTime::parse_from_rfc3339(&self.at)
            .ok()
            .map(|at| at.with_timezone(&Local))
    }
}

fn one_line(text: &str) -> String {
    text.trim().replace("\r\n", " ").replace(['\n', '\r'], " ")
}

pub fn path() -> PathBuf {
    config::data_dir().join("entries.jsonl")
}

/// Adds one entry. It reaches the disk before this returns, so a power cut can
/// lose at most the entry being written.
pub fn append(path: &Path, entry: &Stored) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(entry).map_err(io::Error::other)?;
    line.push('\n');

    // A crash in the middle of a write can leave a line without its line break.
    // Appending straight after it would glue the new entry to the broken one.
    if !ends_with_line_break(path)? {
        line.insert(0, '\n');
    }
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.sync_all()
}

fn ends_with_line_break(path: &Path) -> io::Result<bool> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

/// Every readable entry, oldest first. A line that cannot be read is skipped.
pub fn read_all(path: &Path) -> Vec<Stored> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut entries = Vec::new();
    let mut skipped = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Stored>(line) {
            Ok(entry) if entry.wall_clock().is_some() => entries.push(entry),
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        applog::warn(&format!("{skipped} unreadable line(s) skipped in the entries file"));
    }
    entries.sort_by_key(Stored::wall_clock);
    entries
}

/// The newest `days` calendar days (today included), newest entry first, in the
/// shape the tray list shows.
pub fn recent(entries: &[Stored], days: i64) -> Vec<Entry> {
    let today = Local::now().date_naive();
    let first_day = today
        .checked_sub_days(Days::new((days - 1).max(0) as u64))
        .unwrap_or(today);
    let mut shown: Vec<Entry> = entries
        .iter()
        .filter_map(|entry| {
            let at = entry.wall_clock()?;
            (at.date() >= first_day).then(|| Entry {
                date: format!("{:02}.{:02}", at.day(), at.month()),
                time: format!("{:02}:{:02}", at.hour(), at.minute()),
                text: entry.text.clone(),
                sort_key: at.format("%Y-%m-%dT%H:%M").to_string(),
            })
        })
        .collect();
    shown.sort_by(|a, b| b.sort_key.cmp(&a.sort_key));
    shown
}

/// Creates the store the first time. Entries already sitting in the notes folder
/// (from before the app had its own copy) are brought in, so the tray list does
/// not start empty. Afterwards the file exists and this does nothing.
pub fn ensure_initialized(cfg: &config::Config) {
    let path = path();
    if path.exists() {
        return;
    }
    let imported = import_from_notes(cfg, &path);
    applog::info(&format!(
        "Created the entries file with {imported} entr{} from the notes folder",
        if imported == 1 { "y" } else { "ies" }
    ));
}

fn import_from_notes(cfg: &config::Config, path: &Path) -> usize {
    let mut lines = String::new();
    let mut count = 0;
    // The scan is newest first; the file is kept oldest first.
    for entry in journal::scan_recent_entries(cfg, IMPORT_DAYS).iter().rev() {
        let Ok(naive) = NaiveDateTime::parse_from_str(&entry.sort_key, "%Y-%m-%dT%H:%M") else {
            continue;
        };
        let Some(local) = Local.from_local_datetime(&naive).earliest() else {
            continue;
        };
        if let Ok(line) = serde_json::to_string(&Stored::new(local, &entry.text)) {
            lines.push_str(&line);
            lines.push('\n');
            count += 1;
        }
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // An empty file still counts: it marks the store as created.
    if let Err(error) = fs::write(path, lines) {
        applog::error(&format!("Cannot create the entries file: {error}"));
        return 0;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 9, day, hour, minute, 7).unwrap()
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hourly-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn an_entry_keeps_its_text_and_its_wall_clock_time() {
        let entry = Stored::new(at(30, 8, 51), "  Did exercises\nand stretching ");
        assert_eq!(entry.text, "Did exercises and stretching");
        let clock = entry.wall_clock().unwrap();
        assert_eq!(clock.format("%Y-%m-%d %H:%M").to_string(), "2026-09-30 08:51");
    }

    #[test]
    fn entries_survive_a_round_trip_through_the_file_in_order() {
        let dir = scratch("roundtrip");
        let file = dir.join("entries.jsonl");
        append(&file, &Stored::new(at(30, 11, 0), "second")).unwrap();
        append(&file, &Stored::new(at(29, 21, 0), "first, written later but earlier in time")).unwrap();
        let all = read_all(&file);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text, "first, written later but earlier in time");
        assert_eq!(all[1].text, "second");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn text_with_quotes_and_non_latin_letters_is_stored_exactly() {
        let dir = scratch("unicode");
        let file = dir.join("entries.jsonl");
        let text = "Сказав \"привіт\" і поставив 100% на {план} \\ ✓";
        append(&file, &Stored::new(at(30, 9, 0), text)).unwrap();
        assert_eq!(read_all(&file)[0].text, text);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_half_written_last_line_does_not_swallow_the_next_entry() {
        let dir = scratch("torn");
        let file = dir.join("entries.jsonl");
        append(&file, &Stored::new(at(30, 8, 0), "kept")).unwrap();
        let mut broken = fs::read(&file).unwrap();
        broken.extend_from_slice(br#"{"at":"2026-09-30T09:0"#);
        fs::write(&file, broken).unwrap();
        append(&file, &Stored::new(at(30, 10, 0), "also kept")).unwrap();
        let texts: Vec<_> = read_all(&file).into_iter().map(|e| e.text).collect();
        assert_eq!(texts, vec!["kept", "also kept"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unreadable_lines_are_skipped_not_fatal() {
        let dir = scratch("garbage");
        let file = dir.join("entries.jsonl");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &file,
            "not json\n{\"at\":\"nonsense\",\"text\":\"x\"}\n\n{\"at\":\"2026-09-30T08:00:00+02:00\",\"text\":\"good\"}\n",
        )
        .unwrap();
        let all = read_all(&file);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text, "good");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_is_just_no_entries() {
        assert!(read_all(&scratch("missing").join("entries.jsonl")).is_empty());
    }

    #[test]
    fn the_tray_list_is_newest_first_and_limited_to_recent_days() {
        let today = Local::now();
        let old = today - chrono::Duration::days(40);
        let entries = vec![
            Stored::new(old, "too old"),
            Stored::new(today - chrono::Duration::days(1), "yesterday"),
            Stored::new(today, "today"),
        ];
        let shown = recent(&entries, 30);
        let texts: Vec<_> = shown.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["today", "yesterday"]);
        assert_eq!(shown[0].time.len(), 5);
        assert_eq!(shown[0].date.len(), 5);
    }

    #[test]
    fn entries_already_in_a_notes_folder_are_brought_in_once() {
        let dir = scratch("import");
        let vault = dir.join("vault");
        let day = NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        let note = vault.join("2026").join("September");
        fs::create_dir_all(&note).unwrap();
        fs::write(note.join(format!("{day}.md")), "08:30 - from the notes\n\n<next log here>\n").unwrap();
        // The scan only goes back from today, so also put one entry in today's note.
        let today = Local::now();
        let todays = vault
            .join(today.format("%Y").to_string())
            .join(today.format("%B").to_string());
        fs::create_dir_all(&todays).unwrap();
        fs::write(
            todays.join(format!("{}.md", today.format("%Y-%m-%d"))),
            "07:15 - written today\n\n<next log here>\n",
        )
        .unwrap();
        let cfg = config::Config {
            journal_root: Some(vault.to_string_lossy().into_owned()),
            ..config::Config::default()
        };
        let file = dir.join("entries.jsonl");
        assert!(import_from_notes(&cfg, &file) >= 1);
        let texts: Vec<_> = read_all(&file).into_iter().map(|e| e.text).collect();
        assert!(texts.contains(&"written today".to_string()), "{texts:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn with_no_notes_folder_an_empty_store_is_still_created() {
        let dir = scratch("nofolder");
        let file = dir.join("entries.jsonl");
        assert_eq!(import_from_notes(&config::Config::default(), &file), 0);
        assert!(file.exists());
        assert!(read_all(&file).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
