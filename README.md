<div align="center">

# Hourly

One honest sentence every hour. Kept on your computer, and copied into your Obsidian daily note if you like.

[![Download for Windows](https://img.shields.io/badge/Download-Windows-6f5fd6?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/volodya-korol/hourly/releases/latest/download/Hourly-Setup.exe)

Free. No account. Windows 10 and 11. An installer of about 2 MB.

</div>

---

## What it does

Hourly is a small Windows tray app. Once every hour of real activity it asks one
question: what did you just do? You type a sentence, and Hourly saves it:

* **On your computer, always.** Click the tray icon to see your entries by day.
* **In your notes, if you want.** Choose a folder (for example your Obsidian vault)
  and every entry is also added to that day's note:

```
09:10 - Reviewed the pull request and left comments
10:15 - Wrote the launch copy for the forum post
11:20 - Ported the check in window and tested dragging
```

Real activity means the clock stops while your screen is locked or your computer
sleeps, so Hourly never asks about time you were away.

## Why

A day rebuilt from memory at 11 pm feels accurate and is not. Timers fail because
you forget to start them. Hourly comes to you while the hour is still fresh, and
answering takes about ten seconds.

## Features

* **Works without Obsidian.** Entries are always kept on your computer. A notes
  folder is optional.
* **Plain markdown in the notes you already keep.** No plugin, and Obsidian does
  not need to be open. Pick how your daily notes are organised from a short list, or
  choose one of your own notes and Hourly works out the pattern by itself.
* **Export for an AI assistant.** Copy or save your entries as Markdown, plain text or
  JSON for the last day, week, month or everything, with an optional ready made
  instruction, and ask ChatGPT, Claude or Gemini where your time went.
* **Your rules.** Set a minimum entry length (50 characters by default), keep the one
  skip per day for meetings and deep work, and choose the marker line entries go above.
* **A dashboard in the tray.** Your entries grouped by day, with a countdown to the
  next check in.
* **Looks after itself.** Starts with Windows (you can turn that off in Settings) and
  installs updates in the background.

## Install

1. Click **Download for Windows** above and run the file.
2. Windows may show a SmartScreen warning because the installer is not code signed
   yet. Choose **More info**, then **Run anyway**.
3. Settings opens. If you use Obsidian, or any folder of daily notes, choose it now
   and how your notes are organised. If not, just close the window: Hourly works
   without it, and you can add a folder later in Settings.

That is all. Hourly now runs in the tray. On Windows 11 its icon may sit behind the
^ arrow: drag it onto the taskbar to keep it in view. Press **Ctrl+Shift+L** to write
an entry at any time.

## Privacy

Everything stays on your computer. There is no account and no server. Your entries
are kept in one file on your computer (`entries.jsonl`, which Settings can show you)
and, if you choose a notes folder, copied into your own notes. The internet is used
for two things: a check for new versions on GitHub every few hours, and the Get help
link, which opens the feedback page in your browser.

## Help and feedback

If something does not work, open Settings, click **Open log**, and send the file with
your message. Bugs and ideas are welcome in the
[feedback form](https://hourly.korol.app/feedback). More about the app is at
[hourly.korol.app](https://hourly.korol.app/).

## Source and license

Hourly is free, and its source code is public so you can read exactly what it does.
It is source available under the [Functional Source License](LICENSE.md)
(FSL-1.1-MIT): you may use, study and change it for any purpose except offering a
competing product, and every version becomes plain MIT two years after its release.

## For developers

Build, test and release notes are in [DEVELOPMENT.md](DEVELOPMENT.md).
