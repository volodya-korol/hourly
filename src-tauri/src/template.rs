//! Ready-made note locations, and working out a location from a real note.
//!
//! Nobody should have to write `%Y/%B/%Y-%m-%d.md` by hand. Two easier ways in:
//! a short list of common layouts, and "detect from a note": the user picks one
//! of their own daily notes and `infer` works the pattern out from its path.

use crate::journal::render_relative;
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub template: &'static str,
    /// The path this layout gives for today, shown next to the name.
    pub example: String,
}

const PRESETS: [(&str, &str, &str); 5] = [
    ("flat", "All notes in one folder", "%Y-%m-%d.md"),
    ("year", "A folder per year", "%Y/%Y-%m-%d.md"),
    ("year-month-name", "Year and month name folders", "%Y/%B/%Y-%m-%d.md"),
    ("year-month-number", "Year and month number folders", "%Y/%m/%Y-%m-%d.md"),
    ("month-folder", "A folder per month", "%Y-%m/%Y-%m-%d.md"),
];

pub fn presets() -> Vec<Preset> {
    presets_for(&Local::now())
}

fn presets_for(now: &DateTime<Local>) -> Vec<Preset> {
    PRESETS
        .iter()
        .map(|(id, label, template)| Preset {
            id,
            label,
            template,
            example: render_relative(template, now).unwrap_or_default(),
        })
        .collect()
}

const NO_DATE: &str =
    "Couldn't find a date in that file's name. Choose a note named like 2026-09-29.md";

/// Works out the template for `note`, a daily note somewhere inside `root`.
///
/// The date in the file name says which day the note is for; every other part
/// of the path that matches that day (year, month number, month name, weekday)
/// becomes a token, and everything else is kept as fixed text. The result is
/// checked by rendering it for that day: it must give back the very same path.
pub fn infer(root: &str, note: &str) -> Result<String, String> {
    let parts = relative_parts(root, note)?;
    let (name, folders) = parts.split_last().ok_or("Choose a note, not the vault folder")?;
    let hit = find_date(name)?.ok_or(NO_DATE)?;

    let mut state = State::default();
    let mut template = String::new();
    for folder in folders {
        template.push_str(&tokenize(folder, hit.date, &mut state));
        template.push('/');
    }
    template.push_str(&tokenize(&name[..hit.start], hit.date, &mut state));
    template.push_str(&hit.format);
    template.push_str(&tokenize(&name[hit.end..], hit.date, &mut state));

    let noon = local_noon(hit.date).ok_or("That date does not exist on this computer")?;
    let rendered = render_relative(&template, &noon)?;
    if rendered.to_lowercase() != parts.join("/").to_lowercase() {
        return Err("Couldn't work out the pattern of that note's location".into());
    }
    Ok(template)
}

fn local_noon(date: NaiveDate) -> Option<DateTime<Local>> {
    date.and_hms_opt(12, 0, 0)
        .and_then(|naive| Local.from_local_datetime(&naive).earliest())
}

/// The folders and file name of `note` below `root`, compared without regard to
/// letter case (Windows paths are case-insensitive).
fn relative_parts(root: &str, note: &str) -> Result<Vec<String>, String> {
    let split = |path: &str| -> Vec<String> {
        path.split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    };
    let root_parts = split(root);
    let note_parts = split(note);
    let inside = note_parts.len() > root_parts.len()
        && root_parts
            .iter()
            .zip(&note_parts)
            .all(|(a, b)| a.to_lowercase() == b.to_lowercase());
    if !inside {
        return Err("That note is not inside your vault folder".into());
    }
    Ok(note_parts[root_parts.len()..].to_vec())
}

struct DateHit {
    start: usize,
    end: usize,
    date: NaiveDate,
    /// strftime for the matched text, e.g. "%Y-%m-%d".
    format: String,
}

/// Finds a full date in a file name: 2026-09-29 (also with "." or "_"),
/// 20260929, 29-09-2026 or 09-29-2026. Only ASCII digits are involved, so all
/// the byte offsets are character boundaries.
fn find_date(name: &str) -> Result<Option<DateHit>, String> {
    let bytes = name.as_bytes();
    let digits = |from: usize, count: usize| {
        from + count <= bytes.len() && bytes[from..from + count].iter().all(u8::is_ascii_digit)
    };
    let number = |from: usize, count: usize| name[from..from + count].parse::<u32>().unwrap_or(0);
    let is_separator = |b: u8| matches!(b, b'-' | b'.' | b'_');

    for i in 0..bytes.len() {
        // A date starts at the beginning of a run of digits, not inside one.
        if !bytes[i].is_ascii_digit() || (i > 0 && bytes[i - 1].is_ascii_digit()) {
            continue;
        }

        // 2026-09-29
        if digits(i, 4)
            && i + 10 <= bytes.len()
            && is_separator(bytes[i + 4])
            && digits(i + 5, 2)
            && bytes[i + 7] == bytes[i + 4]
            && digits(i + 8, 2)
            && !digits(i + 10, 1)
        {
            if let Some(date) =
                NaiveDate::from_ymd_opt(number(i, 4) as i32, number(i + 5, 2), number(i + 8, 2))
            {
                let s = bytes[i + 4] as char;
                return Ok(Some(DateHit {
                    start: i,
                    end: i + 10,
                    date,
                    format: format!("%Y{s}%m{s}%d"),
                }));
            }
        }

        // 20260929
        if digits(i, 8) && !digits(i + 8, 1) {
            if let Some(date) =
                NaiveDate::from_ymd_opt(number(i, 4) as i32, number(i + 4, 2), number(i + 6, 2))
            {
                return Ok(Some(DateHit {
                    start: i,
                    end: i + 8,
                    date,
                    format: "%Y%m%d".into(),
                }));
            }
        }

        // 29-09-2026 or 09-29-2026
        if digits(i, 2)
            && i + 10 <= bytes.len()
            && is_separator(bytes[i + 2])
            && digits(i + 3, 2)
            && bytes[i + 5] == bytes[i + 2]
            && digits(i + 6, 4)
            && !digits(i + 10, 1)
        {
            let (first, second, year) = (number(i, 2), number(i + 3, 2), number(i + 6, 4) as i32);
            let s = bytes[i + 2] as char;
            let (day, month, format) = match (first > 12, second > 12) {
                (true, false) => (first, second, format!("%d{s}%m{s}%Y")),
                (false, true) => (second, first, format!("%m{s}%d{s}%Y")),
                (false, false) => {
                    return Err(format!(
                        "The date {} could be day-first or month-first. Choose a note from a day after the 12th, for example 29{s}09{s}2026",
                        &name[i..i + 10]
                    ))
                }
                (true, true) => continue,
            };
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                return Ok(Some(DateHit { start: i, end: i + 10, date, format }));
            }
        }
    }
    Ok(None)
}

#[derive(Default)]
struct State {
    /// A "%m" is already in the template: a later two-digit number that equals
    /// the day is then the day, not the month again.
    month_used: bool,
}

/// Turns the parts of `text` that match `date` into tokens; the rest is kept as
/// fixed text ("%" is doubled so it stays a plain percent sign).
fn tokenize(text: &str, date: NaiveDate, state: &mut State) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let kind = |c: char| {
            if c.is_ascii_digit() {
                1
            } else if c.is_alphabetic() {
                2
            } else {
                0
            }
        };
        let k = kind(chars[i]);
        if k == 0 {
            if chars[i] == '%' {
                out.push_str("%%");
            } else {
                out.push(chars[i]);
            }
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && kind(chars[j]) == k {
            j += 1;
        }
        let run: String = chars[i..j].iter().collect();
        out.push_str(&if k == 1 {
            digit_run(&run, date, state)
        } else {
            letter_run(&run, date)
        });
        i = j;
    }
    out
}

fn digit_run(run: &str, date: NaiveDate, state: &mut State) -> String {
    let year = format!("{:04}", date.year());
    let month = format!("{:02}", date.month());
    let day = format!("{:02}", date.day());
    if run == year {
        "%Y".into()
    } else if run == month && (run != day || !state.month_used) {
        state.month_used = true;
        "%m".into()
    } else if run == day {
        "%d".into()
    } else {
        run.into()
    }
}

fn letter_run(run: &str, date: NaiveDate) -> String {
    let same = |format: &str| date.format(format).to_string().eq_ignore_ascii_case(run);
    if same("%B") {
        "%B".into()
    } else if same("%b") {
        "%b".into()
    } else if same("%A") {
        "%A".into()
    } else if same("%a") {
        "%a".into()
    } else {
        run.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "D:\\obsidian\\Books";

    fn note(relative: &str) -> String {
        format!("{ROOT}\\{}", relative.replace('/', "\\"))
    }

    fn inferred(relative: &str) -> Result<String, String> {
        infer(ROOT, &note(relative))
    }

    #[test]
    fn year_and_month_name_folders() {
        assert_eq!(inferred("2026/September/2026-09-29.md").unwrap(), "%Y/%B/%Y-%m-%d.md");
    }

    #[test]
    fn a_flat_vault_and_a_root_in_another_letter_case() {
        assert_eq!(infer("d:\\OBSIDIAN\\books", &note("2026-09-29.md")).unwrap(), "%Y-%m-%d.md");
    }

    #[test]
    fn a_fixed_folder_stays_fixed() {
        assert_eq!(inferred("Daily/2026-09-29.md").unwrap(), "Daily/%Y-%m-%d.md");
        assert_eq!(inferred("Journal/Daily notes/2026-09-29.md").unwrap(), "Journal/Daily notes/%Y-%m-%d.md");
    }

    #[test]
    fn month_number_folders_including_when_month_and_day_are_the_same() {
        assert_eq!(inferred("2026/09/2026-09-29.md").unwrap(), "%Y/%m/%Y-%m-%d.md");
        assert_eq!(inferred("2026/09/2026-09-09.md").unwrap(), "%Y/%m/%Y-%m-%d.md");
        assert_eq!(inferred("2026/01/2026-01-01.md").unwrap(), "%Y/%m/%Y-%m-%d.md");
    }

    #[test]
    fn a_folder_per_month_and_per_day() {
        assert_eq!(inferred("2026-09/2026-09-29.md").unwrap(), "%Y-%m/%Y-%m-%d.md");
        assert_eq!(inferred("2026/09/29/note.md").unwrap_err(), NO_DATE);
        assert_eq!(inferred("2026/09/29/2026-09-29.md").unwrap(), "%Y/%m/%d/%Y-%m-%d.md");
    }

    #[test]
    fn weekday_and_month_names_in_the_file_name() {
        assert_eq!(inferred("2026-09-29 Tuesday.md").unwrap(), "%Y-%m-%d %A.md");
        assert_eq!(inferred("Sep/2026-09-29 Tue.md").unwrap(), "%b/%Y-%m-%d %a.md");
        assert_eq!(inferred("september/2026-09-29.md").unwrap(), "%B/%Y-%m-%d.md");
    }

    #[test]
    fn other_date_spellings() {
        assert_eq!(inferred("2026.09.29.md").unwrap(), "%Y.%m.%d.md");
        assert_eq!(inferred("2026_09_29.md").unwrap(), "%Y_%m_%d.md");
        assert_eq!(inferred("20260929.md").unwrap(), "%Y%m%d.md");
        assert_eq!(inferred("29-09-2026.md").unwrap(), "%d-%m-%Y.md");
        assert_eq!(inferred("09-29-2026.md").unwrap(), "%m-%d-%Y.md");
        assert_eq!(inferred("Daily 2026-09-29 log.md").unwrap(), "Daily %Y-%m-%d log.md");
    }

    #[test]
    fn a_date_that_could_be_read_two_ways_asks_for_another_note() {
        let error = inferred("05-06-2026.md").unwrap_err();
        assert!(error.contains("day-first or month-first"), "{error}");
        assert!(error.contains("05-06-2026"), "{error}");
    }

    #[test]
    fn errors_are_plain() {
        assert_eq!(inferred("notes.md").unwrap_err(), NO_DATE);
        assert_eq!(inferred("2026-13-40.md").unwrap_err(), NO_DATE);
        assert!(infer(ROOT, "C:\\other\\2026-09-29.md").unwrap_err().contains("inside your vault"));
        assert!(infer(ROOT, ROOT).unwrap_err().contains("inside your vault"));
        assert!(infer(ROOT, "D:\\obsidian\\Books2\\2026-09-29.md").unwrap_err().contains("inside your vault"));
    }

    #[test]
    fn a_percent_sign_in_a_folder_name_stays_a_plain_percent_sign() {
        let template = inferred("100%/2026-09-29.md").unwrap();
        assert_eq!(template, "100%%/%Y-%m-%d.md");
        let noon = local_noon(NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()).unwrap();
        assert_eq!(render_relative(&template, &noon).unwrap(), "100%/2026-09-29.md");
    }

    #[test]
    fn non_ascii_folder_names_are_kept() {
        assert_eq!(inferred("Щоденник/2026-09-29.md").unwrap(), "Щоденник/%Y-%m-%d.md");
    }

    #[test]
    fn every_preset_renders_and_is_found_again_from_its_own_example() {
        for (year, month, day) in [(2026, 9, 29), (2026, 9, 9), (2027, 1, 1), (2026, 12, 12), (2028, 2, 29), (2026, 10, 31)] {
            let now = local_noon(NaiveDate::from_ymd_opt(year, month, day).unwrap()).unwrap();
            for preset in presets_for(&now) {
                assert!(preset.example.ends_with(".md"), "{preset:?}");
                let found = infer(ROOT, &note(&preset.example)).unwrap();
                assert_eq!(found, preset.template, "{preset:?} on {year}-{month}-{day}");
            }
        }
    }

    #[test]
    fn preset_ids_are_unique_and_none_is_called_custom() {
        let mut ids: Vec<_> = PRESETS.iter().map(|p| p.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        assert!(!ids.contains(&"custom"));
    }
}
