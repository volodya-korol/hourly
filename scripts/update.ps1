# Builds the release installer, replaces the installed copy of Hourly and starts
# it. The Tauri counterpart of `npm run update` in the Electron project.
#
# The installed app is closed to be replaced, so an open check-in popup (with a
# half-written entry in it) would be lost. The script refuses to go on while the
# popup is open; -Force overrides that.
param([switch]$Force)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..").Path
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class HourlyWindows {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  public static List<string> Titles(uint pid) {
    var titles = new List<string>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid || !IsWindowVisible(h)) return true;
      var t = new StringBuilder(256); GetWindowText(h, t, 256);
      titles.Add(t.ToString());
      return true;
    }, IntPtr.Zero);
    return titles;
  }
}
"@

$keyFile = "$env:USERPROFILE\.tauri\hourly.key"
if (-not (Test-Path $keyFile)) { throw "Signing key not found: $keyFile" }
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $keyFile -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''

Write-Host '== Build ==' -ForegroundColor Cyan
Push-Location $root
try {
  cargo tauri build
  if ($LASTEXITCODE -ne 0) { throw "Build failed (exit code $LASTEXITCODE)" }
} finally {
  Pop-Location
}

$version = (Get-Content "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
$installer = Get-Item "$root\src-tauri\target\release\bundle\nsis\*_${version}_x64-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$installed = Join-Path $env:LOCALAPPDATA 'Hourly\hourly.exe'
# The app used to be installed as "Hourly Checkin"; that copy is removed below.
$legacy = Join-Path $env:LOCALAPPDATA 'Hourly Checkin\hourly.exe'

# Only the installed copy is closed, never a development build or the Electron app.
Write-Host '== Closing the running app (if any) ==' -ForegroundColor Cyan
$running = @(Get-Process hourly -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $installed -or $_.Path -eq $legacy })
$popupOpen = $running | Where-Object { [HourlyWindows]::Titles([uint32]$_.Id) -contains 'What did you just do?' }
if ($popupOpen -and -not $Force) {
  throw 'The check-in popup is open. Save or skip it first (or run with -Force to close it and lose what is typed). The new version is built and waiting: run this again.'
}
$running | Stop-Process -Force
Start-Sleep -Milliseconds 500

# The old "Hourly Checkin" install folder: uninstalling it also removes its start-up
# entry and shortcuts. Settings and entries in %APPDATA% are kept.
if (Test-Path $legacy) {
  Write-Host '== Removing the old "Hourly Checkin" install ==' -ForegroundColor Cyan
  $legacyFolder = Split-Path $legacy
  Start-Process (Join-Path $legacyFolder 'uninstall.exe') -ArgumentList '/S', "_?=$legacyFolder" -Wait
  Remove-Item $legacyFolder -Recurse -Force -ErrorAction SilentlyContinue
}

# The installer will not step back to an older version, so a newer copy is removed
# first. Settings, state and the log live in %APPDATA% and are kept. "_?=" makes
# the uninstaller run in place, so that -Wait really waits for it.
if (Test-Path $installed) {
  $current = [version](Get-Item $installed).VersionInfo.FileVersion
  if ($current -gt [version]$version) {
    Write-Host "== Removing the newer installed version $current first ==" -ForegroundColor Cyan
    $folder = Split-Path $installed
    Start-Process (Join-Path $folder 'uninstall.exe') -ArgumentList '/S', "_?=$folder" -Wait
  }
}

Write-Host "== Installing $($installer.Name) ==" -ForegroundColor Cyan
Start-Process $installer.FullName -ArgumentList '/S' -Wait
if (-not (Test-Path $installed)) { throw "The installed app was not found at $installed" }

Start-Process $installed
Write-Host "Done. Hourly $version is installed and running." -ForegroundColor Green
