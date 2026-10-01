# Developing Hourly

Hourly is a Tauri v2 app: a Rust core with three small web pages (the popup, the
dashboard and the settings). It uses the system web view (WebView2), which is why
the installer is only about 2 MB.

## Commands

Run from the repository root. Rust, the Tauri CLI (`cargo install tauri-cli`) and
the Visual Studio C++ build tools are required.

| What | Command |
| --- | --- |
| Build and install on this computer | `npm run update` (builds, closes the installed Hourly, installs, starts it) |
| Run in development | `cargo build --manifest-path src-tauri/Cargo.toml`, then `src-tauri\target\debug\hourly.exe --dev` |
| Unit tests (settings, notes, note location, log, lock and sleep messages) | `npm test` (they open no windows and never touch your real settings or log) |
| Screenshots of the windows | `.\scripts\ui-shots.ps1 -AppArgs '--dev','--show=settings' -Name settings` |
| Release installer | see "Releasing" |

`npm run update` refuses to go on while the check-in popup is open, because the
installed app has to be closed and a half-written entry would be lost. Save or skip
the popup first, or run `npm run update -- -Force`.

The app allows one running copy, except when `HOURLY_DATA_DIR` is set: such a run is
a separate world and does not touch the installed Hourly.

`--dev` makes the popup come every 20 seconds and turns off autostart and the update
check. `--dev --show=settings|dashboard|popup` opens just that window.
`HOURLY_DATA_DIR` moves settings, state and the log to another folder, so experiments
never touch real data.

## Where things are

| File | What it does |
| --- | --- |
| `src-tauri/src/power.rs` | Screen lock, unlock, sleep and resume, from Windows messages. Idle time is deliberately not used: a paused video still counts as being at the computer. |
| `src-tauri/src/checkin.rs` | The hourly loop: counts real activity, decides when to ask. |
| `src-tauri/src/store.rs` | The app's own copy of every entry: `entries.jsonl`, one JSON object per line, appended and flushed per entry. The tray list, the "last entry" hint and exports all read it. On the first run entries already in a notes folder (up to a year back) are brought in. |
| `src-tauri/src/export.rs` | Turns the stored entries into Markdown, plain text or JSON for a period, optionally with an instruction for an AI assistant. |
| `src-tauri/src/journal.rs` | The optional copy into a notes folder: renders the note path, writes entries before the marker, reads notes. Notes are replaced with `ReplaceFile`, which never leaves a half written note and keeps the creation date. |
| `src-tauri/src/template.rs` | The ready made note layouts, and working out a layout from one of the user's notes. |
| `src-tauri/src/config.rs` | The settings file, and the one time import of `config.json` from the earlier Electron build. |
| `src-tauri/src/applog.rs` | The log file and the panic hook (see "Log"). |
| `src-tauri/src/ui.rs` | Creates the popup, dashboard and settings windows. |
| `src-tauri/src/commands.rs` | Everything the pages can call. |
| `src-tauri/src/updater.rs` | Downloads updates and installs them by itself once no popup is open. |
| `src/*.html`, `src/*.js`, `src/scrollbar.css` | The three windows. No bundler: plain files. |

Settings and state live in `%APPDATA%\hourly-checkin\`: `settings.json`,
`state.json` (the skip date), `entries.jsonl` (the entries) and `hourly.log`.

## How an entry is saved

1. It is appended to `entries.jsonl`. This must succeed, or the popup shows the error
   and stays open.
2. If the user chose a notes folder, it is also written into that day's note. A
   failure here is logged and the user is told once per run of failures (the entry is
   safe in step 1).

Nothing needs to be set up first: there is no notes folder by default, so a new
install records straight away. Settings opens on the first launch only to offer the
folder. The notes folder is a plain `journal_root` in `settings.json`; empty means
off.

## Note location

The location of the daily note is a path template in chrono / strftime tokens
(`%Y/%B/%Y-%m-%d.md`). Only `%x` sequences are tokens, so folder separators and the
`.md` extension are always literal text. Nobody has to write one:

* Settings offers five ready made layouts, each shown with today's real path
  (`template.rs`, `PRESETS`).
* **Detect from a note** lets the user pick one of their own daily notes.
  `template::infer` reads the date from the file name (`2026-09-29`, `2026.09.29`,
  `20260929`, `29-09-2026`, `09-29-2026`), turns every part of the path that matches
  that day (year, month number, month name, weekday) into a token, keeps the rest as
  fixed text, and renders the result for that day to prove it gives back the same
  path. `05-06-2026` is refused as ambiguous (day first or month first) and the user
  is asked for another note.
* Anything else goes in the **Custom format** field, shown only when the saved
  template is not one of the ready made layouts.

Month and weekday names are English (chrono's default). The preview shows the note
for today, tomorrow and a month ahead, so a folder that never changes is easy to spot.

## Log

`%APPDATA%\hourly-checkin\hourly.log` records what the app did and what went wrong:
start up, settings changes, the path each entry was saved to, update checks, lock and
sleep events and panics (release builds abort on a panic, so the hook writes the
reason first). The text of an entry is never written to it. It rotates at 512 KB to
`hourly.log.old`. Settings has an "Open log" link that shows the file in Explorer.

## Releasing

The updater checks the address in `plugins.updater.endpoints` of
`src-tauri/tauri.conf.json` (a `latest.json` in the GitHub releases of this
repository, which must be public) and only installs files signed with the private key
that matches the public key in the same file. When a new version is downloaded it is
installed as soon as no popup is open, and the Windows installer starts Hourly again.

1. Raise `version` in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`.
2. Build with the signing key:
   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$env:USERPROFILE\.tauri\hourly.key" -Raw
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
   cargo tauri build
   ```
3. `.\scripts\prepare-release.ps1 -Notes "What changed"` copies the installer and
   writes `latest.json` into `release\v<version>\`, then prints the `gh release create`
   command. The repository is taken from the update address in the config. The script
   publishes nothing by itself.

The source is under FSL-1.1-MIT (`LICENSE.md`). Forks cannot update other people's
installs: that needs the private signing key below.

**Back up `%USERPROFILE%\.tauri\hourly.key`.** If it is lost, installed copies can
never be updated again (users would have to reinstall by hand). Never commit it.
