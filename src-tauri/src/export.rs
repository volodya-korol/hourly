//! Turns the stored entries into text that can be pasted into an AI assistant (or
//! saved as a file). Three formats:
//!
//! * Markdown: a heading per day. Easy for a person and for a model to read.
//! * Plain text: one line per entry. The smallest, good for long periods.
//! * JSON: structured data for scripts and tools.
//!
//! Entries are always oldest first, so a model reads the days in the order they
//! happened.

use crate::store::Stored;
use chrono::{DateTime, Days, Local, NaiveDate, Timelike};
use serde_json::json;

/// Put in front of the export when the user asks for it (as a field of its own
/// in JSON, so the JSON stays valid).
const INSTRUCTION: &str = "Below is my personal log of what I did, written down about once an hour. \
The entries may be in any language. Please read it and tell me where my time went, what I got done, \
and any patterns or problems you notice. Then suggest one or two small changes for next week.";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Markdown,
    Text,
    Json,
}

impl Format {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "markdown" => Ok(Self::Markdown),
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(format!("Unknown export format: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Range {
    Today,
    Last7Days,
    Last30Days,
    All,
}

impl Range {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "today" => Ok(Self::Today),
            "7days" => Ok(Self::Last7Days),
            "30days" => Ok(Self::Last30Days),
            "all" => Ok(Self::All),
            other => Err(format!("Unknown export period: {other}")),
        }
    }

    /// The first day included; `None` means no limit.
    fn first_day(self, today: NaiveDate) -> Option<NaiveDate> {
        let back = |days: u64| Some(today.checked_sub_days(Days::new(days)).unwrap_or(today));
        match self {
            Self::Today => Some(today),
            Self::Last7Days => back(6),
            Self::Last30Days => back(29),
            Self::All => None,
        }
    }
}

pub struct Exported {
    pub text: String,
    pub count: usize,
}

pub fn export(
    entries: &[Stored],
    format: Format,
    range: Range,
    with_instruction: bool,
    now: DateTime<Local>,
) -> Exported {
    let today = now.date_naive();
    let first_day = range.first_day(today);

    let mut chosen: Vec<_> = entries
        .iter()
        .filter_map(|entry| Some((entry.wall_clock()?, entry)))
        .filter(|(at, _)| at.date() <= today && first_day.is_none_or(|first| at.date() >= first))
        .collect();
    chosen.sort_by_key(|(at, _)| *at);

    let (Some((first, _)), Some((last, _))) = (chosen.first(), chosen.last()) else {
        return Exported { text: String::new(), count: 0 };
    };
    let (from, to) = (first.date(), last.date());
    let count = chosen.len();
    let prefix = if with_instruction && format != Format::Json {
        format!("{INSTRUCTION}\n\n")
    } else {
        String::new()
    };

    let body = match format {
        Format::Markdown => {
            let mut out = format!("# Hourly log\n\nPeriod: {from} to {to}. {count} entries.\n");
            let mut day = None;
            for (at, entry) in &chosen {
                if day != Some(at.date()) {
                    day = Some(at.date());
                    out.push_str(&format!("\n## {}, {}\n\n", at.date(), at.format("%A")));
                }
                out.push_str(&format!("- {:02}:{:02} {}\n", at.hour(), at.minute(), entry.text));
            }
            out
        }
        Format::Text => chosen
            .iter()
            .map(|(at, entry)| format!("{} - {}\n", at.format("%Y-%m-%d %H:%M"), entry.text))
            .collect(),
        Format::Json => {
            let rows: Vec<_> = chosen
                .iter()
                .map(|(at, entry)| {
                    json!({
                        "date": at.date().to_string(),
                        "time": format!("{:02}:{:02}", at.hour(), at.minute()),
                        "weekday": at.format("%A").to_string(),
                        "text": entry.text,
                    })
                })
                .collect();
            let mut document = json!({
                "app": "Hourly",
                "exported_at": now.to_rfc3339(),
                "period": { "from": from.to_string(), "to": to.to_string() },
                "count": count,
                "entries": rows,
            });
            if with_instruction {
                document["instruction"] = json!(INSTRUCTION);
            }
            let mut text = serde_json::to_string_pretty(&document).unwrap_or_default();
            text.push('\n');
            text
        }
    };

    Exported { text: format!("{prefix}{body}"), count }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn stored(day: u32, hour: u32, minute: u32, text: &str) -> Stored {
        Stored::new(Local.with_ymd_and_hms(2026, 9, day, hour, minute, 0).unwrap(), text)
    }

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 9, 30, 18, 0, 0).unwrap()
    }

    fn sample() -> Vec<Stored> {
        vec![
            stored(30, 11, 45, "Planned the launch"),
            stored(29, 21, 56, "Wrote the app"),
            stored(30, 8, 51, "Did exercises"),
            stored(10, 9, 0, "A long time ago"),
        ]
    }

    #[test]
    fn markdown_has_a_heading_per_day_oldest_first() {
        let out = export(&sample(), Format::Markdown, Range::Last7Days, false, now());
        assert_eq!(out.count, 3);
        assert_eq!(
            out.text,
            "# Hourly log\n\nPeriod: 2026-09-29 to 2026-09-30. 3 entries.\n\n\
             ## 2026-09-29, Tuesday\n\n- 21:56 Wrote the app\n\n\
             ## 2026-09-30, Wednesday\n\n- 08:51 Did exercises\n- 11:45 Planned the launch\n"
        );
    }

    #[test]
    fn plain_text_is_one_line_per_entry() {
        let out = export(&sample(), Format::Text, Range::Last7Days, false, now());
        assert_eq!(
            out.text,
            "2026-09-29 21:56 - Wrote the app\n2026-09-30 08:51 - Did exercises\n2026-09-30 11:45 - Planned the launch\n"
        );
    }

    #[test]
    fn json_is_valid_and_complete() {
        let out = export(&sample(), Format::Json, Range::Today, false, now());
        let value: serde_json::Value = serde_json::from_str(&out.text).unwrap();
        assert_eq!(value["count"], 2);
        assert_eq!(value["period"]["from"], "2026-09-30");
        assert_eq!(value["entries"][0]["time"], "08:51");
        assert_eq!(value["entries"][0]["weekday"], "Wednesday");
        assert_eq!(value["entries"][1]["text"], "Planned the launch");
        assert!(value.get("instruction").is_none());
    }

    #[test]
    fn the_instruction_leads_text_formats_and_is_a_field_in_json() {
        let md = export(&sample(), Format::Markdown, Range::Today, true, now()).text;
        assert!(md.starts_with("Below is my personal log"), "{md}");
        let json_text = export(&sample(), Format::Json, Range::Today, true, now()).text;
        let value: serde_json::Value = serde_json::from_str(&json_text).expect("still valid JSON");
        assert!(value["instruction"].as_str().unwrap().starts_with("Below is my personal log"));
    }

    #[test]
    fn periods_cut_at_the_right_day() {
        let counts = |range| export(&sample(), Format::Text, range, false, now()).count;
        assert_eq!(counts(Range::Today), 2);
        assert_eq!(counts(Range::Last7Days), 3);
        assert_eq!(counts(Range::Last30Days), 4);
        assert_eq!(counts(Range::All), 4);
    }

    #[test]
    fn seven_days_means_today_and_the_six_days_before() {
        let entries = vec![stored(24, 9, 0, "six days ago"), stored(23, 9, 0, "seven days ago")];
        let out = export(&entries, Format::Text, Range::Last7Days, false, now());
        assert_eq!(out.count, 1);
        assert!(out.text.contains("six days ago"));
    }

    #[test]
    fn nothing_in_the_period_gives_an_empty_result() {
        let out = export(&sample(), Format::Markdown, Range::Today, false, now() + chrono::Duration::days(5));
        assert_eq!(out.count, 0);
        assert!(out.text.is_empty());
    }

    #[test]
    fn entries_from_the_future_are_left_out() {
        let entries = vec![stored(30, 9, 0, "now"), stored(30, 9, 0, "x")];
        let earlier = Local.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
        assert_eq!(export(&entries, Format::Text, Range::All, false, earlier).count, 0);
    }

    #[test]
    fn unknown_names_are_errors() {
        assert!(Format::parse("pdf").is_err());
        assert!(Range::parse("forever").is_err());
        assert_eq!(Format::parse("json"), Ok(Format::Json));
        assert_eq!(Range::parse("30days"), Ok(Range::Last30Days));
    }
}
