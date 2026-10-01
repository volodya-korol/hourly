use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

pub const DEFAULT_PATH_TEMPLATE: &str = "%Y/%B/%Y-%m-%d.md";
pub const DEFAULT_MARKER: &str = "<next log here>";
/// An entry must have at least this many characters before it can be saved.
pub const DEFAULT_MIN_LENGTH: u32 = 50;
/// An entry is one sentence; a larger minimum would make saving impossible in practice.
pub const MAX_MIN_LENGTH: u32 = 500;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub journal_root: Option<String>,
    pub path_template: String,
    pub marker: String,
    pub min_length: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            journal_root: None,
            path_template: DEFAULT_PATH_TEMPLATE.into(),
            marker: DEFAULT_MARKER.into(),
            min_length: DEFAULT_MIN_LENGTH,
        }
    }
}

// Та сама тека, що й userData в Electron-версії: state.json (дата пропуску)
// читається без змін, а старий config.json імпортується один раз.
// HOURLY_DATA_DIR дозволяє запускати тести й smoke-прогони, не чіпаючи
// реальні дані.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HOURLY_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hourly-checkin")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// True when this computer has never run Hourly before: there is neither a
/// settings file nor an old `config.json` to bring over. Must be asked before
/// `load`, which writes the settings file.
pub fn first_run() -> bool {
    !settings_path().exists() && !data_dir().join("config.json").exists()
}

pub fn load() -> Config {
    if let Ok(text) = fs::read_to_string(settings_path()) {
        if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
            return cfg;
        }
    }
    match import_legacy(&data_dir().join("config.json")) {
        Some(cfg) => {
            let _ = save(&cfg);
            cfg
        }
        None => Config::default(),
    }
}

pub fn save(cfg: &Config) -> Result<(), String> {
    fs::create_dir_all(data_dir()).map_err(|e| format!("Cannot create settings folder: {e}"))?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(settings_path(), json).map_err(|e| format!("Cannot save settings: {e}"))
}

fn import_legacy(path: &std::path::Path) -> Option<Config> {
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let root = value.get("journalRoot")?.as_str()?.trim();
    if root.is_empty() {
        return None;
    }
    let mut cfg = Config {
        journal_root: Some(root.to_string()),
        ..Config::default()
    };
    if let Some(t) = value.get("pathTemplate").and_then(|v| v.as_str()) {
        if !t.trim().is_empty() {
            cfg.path_template = legacy_template_to_strftime(t);
        }
    }
    if let Some(m) = value.get("marker").and_then(|v| v.as_str()) {
        if !m.trim().is_empty() {
            cfg.marker = m.to_string();
        }
    }
    Some(cfg)
}

/// Converts the two template dialects the Electron builds used (Moment.js
/// tokens in 1.1.x, the `{yyyy}` placeholders before that) to chrono's
/// strftime syntax. Backslashes become "/" and "[text]" stays literal text.
pub fn legacy_template_to_strftime(t: &str) -> String {
    const PLACEHOLDERS: [(&str, &str); 4] = [
        ("{yyyy}", "%Y"),
        ("{month}", "%B"),
        ("{mm}", "%m"),
        ("{dd}", "%d"),
    ];
    const MOMENT_TOKENS: [(&str, &str); 7] = [
        ("YYYY", "%Y"),
        ("MMMM", "%B"),
        ("MMM", "%b"),
        ("MM", "%m"),
        ("DD", "%d"),
        ("YY", "%y"),
        ("M", "%-m"),
    ];

    let chars: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    'scan: while i < chars.len() {
        let rest: String = chars[i..].iter().collect();

        if chars[i] == '[' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == ']') {
                for c in &chars[i + 1..i + 1 + end] {
                    push_literal(&mut out, *c);
                }
                i += end + 2;
                continue;
            }
        }
        for (from, to) in PLACEHOLDERS {
            if rest.starts_with(from) {
                out.push_str(to);
                i += from.chars().count();
                continue 'scan;
            }
        }
        for (from, to) in MOMENT_TOKENS {
            if rest.starts_with(from) {
                out.push_str(to);
                i += from.chars().count();
                continue 'scan;
            }
        }
        if rest.starts_with('D') {
            out.push_str("%-d");
            i += 1;
            continue;
        }
        push_literal(&mut out, if chars[i] == '\\' { '/' } else { chars[i] });
        i += 1;
    }
    out
}

fn push_literal(out: &mut String, c: char) {
    if c == '%' {
        out.push_str("%%");
    } else {
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_moment_template_from_1_1_x() {
        assert_eq!(
            legacy_template_to_strftime("YYYY/MMMM/YYYY-MM-DD[.md]"),
            "%Y/%B/%Y-%m-%d.md"
        );
    }

    #[test]
    fn converts_the_older_placeholder_template() {
        assert_eq!(
            legacy_template_to_strftime("{yyyy}\\{month}\\{yyyy}-{mm}-{dd}.md"),
            "%Y/%B/%Y-%m-%d.md"
        );
    }

    #[test]
    fn keeps_unbracketed_md_as_literal_text() {
        // The broken 1.1.0 default: "md" was meant as the file extension.
        assert_eq!(
            legacy_template_to_strftime("YYYY\\MMMM\\YYYY-MM-DD.md"),
            "%Y/%B/%Y-%m-%d.md"
        );
    }

    #[test]
    fn escapes_percent_signs_in_literals() {
        assert_eq!(legacy_template_to_strftime("100%/[50%]"), "100%%/50%%");
    }

    #[test]
    fn imports_the_real_electron_config() {
        let dir = std::env::temp_dir().join(format!("hourly-cfg-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(
            &path,
            r#"{"journalRoot":"D:\\obsidian\\Books","pathTemplate":"YYYY/MMMM/YYYY-MM-DD[.md]","marker":"<next log here>"}"#,
        )
        .unwrap();
        let cfg = import_legacy(&path).unwrap();
        assert_eq!(cfg.journal_root.as_deref(), Some("D:\\obsidian\\Books"));
        assert_eq!(cfg.path_template, "%Y/%B/%Y-%m-%d.md");
        assert_eq!(cfg.marker, "<next log here>");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_saved_before_the_minimum_length_existed_get_the_default() {
        let cfg: Config =
            serde_json::from_str(r#"{"journalRoot":"D:\\v","pathTemplate":"%Y.md","marker":"m"}"#).unwrap();
        assert_eq!(cfg.min_length, DEFAULT_MIN_LENGTH);
        assert_eq!(cfg.journal_root.as_deref(), Some("D:\\v"));
    }

    #[test]
    fn the_minimum_length_is_read_from_the_settings_file() {
        let cfg: Config = serde_json::from_str(r#"{"minLength":12}"#).unwrap();
        assert_eq!(cfg.min_length, 12);
    }

    #[test]
    fn a_config_without_a_folder_is_not_imported() {
        let dir = std::env::temp_dir().join(format!("hourly-cfg2-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, r#"{"journalRoot":null}"#).unwrap();
        assert!(import_legacy(&path).is_none());
        let _ = fs::remove_dir_all(dir);
    }
}
