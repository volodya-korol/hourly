# Starts the app with an isolated data folder, screenshots each of its visible
# windows (only the window area, never the whole desktop) and stops it.
#
#   .\scripts\ui-shots.ps1 -AppArgs '--dev','--show=settings' -Name settings
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\debug\hourly.exe",
  [string[]]$AppArgs = @('--dev'),
  [string]$DataDir = "$env:TEMP\hourly-shots\data",
  [string]$OutDir = "$env:TEMP\hourly-shots\out",
  [string]$Name = 'shot',
  [int]$Seconds = 6
)

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class WinShots {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
  public static List<string> Windows(uint pid) {
    var found = new List<string>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid || !IsWindowVisible(h)) return true;
      RECT r; GetWindowRect(h, out r);
      if (r.R - r.L < 50 || r.B - r.T < 50) return true;
      var t = new StringBuilder(256); GetWindowText(h, t, 256);
      found.Add(r.L + "," + r.T + "," + (r.R - r.L) + "," + (r.B - r.T) + "|" + t);
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
[WinShots]::SetProcessDPIAware() | Out-Null

New-Item -ItemType Directory -Force -Path $DataDir, $OutDir | Out-Null
$env:HOURLY_DATA_DIR = $DataDir
$process = Start-Process -FilePath $Exe -ArgumentList $AppArgs -PassThru
try {
  Start-Sleep -Seconds $Seconds
  $index = 0
  foreach ($line in [WinShots]::Windows([uint32]$process.Id)) {
    $rect, $title = $line -split '\|', 2
    $x, $y, $w, $h = $rect -split ',' | ForEach-Object { [int]$_ }
    $bitmap = New-Object System.Drawing.Bitmap $w, $h
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($x, $y, 0, 0, $bitmap.Size)
    $file = Join-Path $OutDir ("{0}-{1}.png" -f $Name, $index++)
    $bitmap.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    "{0}  ({1}x{2}, title '{3}')" -f $file, $w, $h, $title
  }
  if ($index -eq 0) { 'No visible windows found.' }
}
finally {
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
}
