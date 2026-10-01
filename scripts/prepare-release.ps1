# Stages a release for the GitHub repository the app updates from. Nothing is
# uploaded: the script copies the installer, writes latest.json for the
# auto-updater and prints the command that would publish them.
#
# The repository is read from the update address in src-tauri/tauri.conf.json, so
# the address inside the app and the address of the published files cannot differ.
#
#   cargo tauri build          (with TAURI_SIGNING_PRIVATE_KEY set, see DEVELOPMENT.md)
#   .\scripts\prepare-release.ps1 -Notes "What changed"
param([string]$Notes = 'Bug fixes and improvements.')

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..").Path
$config = Get-Content "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
$version = $config.version
$endpoint = $config.plugins.updater.endpoints[0]
if ($endpoint -notmatch 'github\.com/([^/]+/[^/]+)/releases/') { throw "The update address is not a GitHub releases address: $endpoint" }
$repo = $Matches[1]
$bundle = "$root\src-tauri\target\release\bundle\nsis"

$installer = Get-ChildItem $bundle -Filter "*_${version}_x64-setup.exe" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) { throw "No installer for version $version in $bundle. Run: cargo tauri build" }
$signature = "$($installer.FullName).sig"
if (-not (Test-Path $signature)) { throw "Missing $signature. Set TAURI_SIGNING_PRIVATE_KEY before building." }

$out = "$root\release\v$version"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$versioned = "Hourly-Setup-$version.exe"
Copy-Item $installer.FullName "$out\$versioned" -Force
# Same stable name the README download button points to.
Copy-Item $installer.FullName "$out\Hourly-Setup.exe" -Force

$manifest = [ordered]@{
  version  = $version
  notes    = $Notes
  pub_date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = [ordered]@{
    'windows-x86_64' = [ordered]@{
      signature = (Get-Content $signature -Raw).Trim()
      url       = "https://github.com/$repo/releases/download/v$version/$versioned"
    }
  }
}
$json = $manifest | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText("$out\latest.json", $json, (New-Object Text.UTF8Encoding $false))

$sizeMb = [math]::Round($installer.Length / 1MB, 2)
Write-Host "Staged v$version ($sizeMb MB) for $repo in $out"
Write-Host ''
Write-Host 'To publish (not run by this script):'
Write-Host "  gh release create v$version `"$out\$versioned`" `"$out\Hourly-Setup.exe`" `"$out\latest.json`" --repo $repo --title v$version --notes `"$Notes`""
