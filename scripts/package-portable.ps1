# Collects release files into .\release: the NSIS setup, a portable zip (app exe, portctl.exe and
# the "portable" marker), latest.json for the in-app updater and SHA256SUMS.txt. Writes the
# release notes (this version's CHANGELOG section) to .\release-notes.md.
# Run after `pnpm app:build` (with the signing key in TAURI_SIGNING_PRIVATE_KEY, so the setup has
# a .sig) and `pnpm cli:build`. Spaces become dashes so GitHub keeps the file names as they are.
param([string]$Repo = 'yusufky63/port-pilot')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
$exe = Join-Path $root 'target\release\port-pilot.exe'
$cli = Join-Path $root 'target\release\portctl.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "Release exe not found. Run: pnpm app:build" }
if (-not (Test-Path -LiteralPath $cli)) { throw "portctl.exe not found. Run: pnpm cli:build" }
$utf8 = New-Object System.Text.UTF8Encoding $false

$out = Join-Path $root 'release'
$stage = Join-Path $out 'portable'
if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item -LiteralPath $exe -Destination (Join-Path $stage 'PortPilot.exe')
Copy-Item -LiteralPath $cli -Destination (Join-Path $stage 'portctl.exe')
Set-Content -LiteralPath (Join-Path $stage 'portable') -Encoding ascii -Value 'PortPilot keeps its settings in the data folder next to the exe while this file exists.'
Copy-Item -LiteralPath (Join-Path $root 'LICENSE-MIT') -Destination (Join-Path $stage 'LICENSE-MIT.txt')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE-APACHE') -Destination (Join-Path $stage 'LICENSE-APACHE.txt')

$zip = Join-Path $out "PortPilot_${version}_x64_portable.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
Remove-Item -LiteralPath $stage -Recurse -Force

$bundle = Join-Path $root "target\release\bundle\nsis"
$setup = Get-ChildItem -Path (Join-Path $bundle "*_${version}_x64-setup.exe") -ErrorAction SilentlyContinue | Select-Object -First 1
if ($setup) {
  $setupName = $setup.Name -replace ' ', '-'
  Copy-Item -LiteralPath $setup.FullName -Destination (Join-Path $out $setupName)
}

# Release notes: the CHANGELOG section for this version.
$log = Get-Content (Join-Path $root 'CHANGELOG.md') -Raw
$m = [regex]::Match($log, "(?ms)^## \[$([regex]::Escape($version))\][^\n]*\n(.*?)(?=^## \[|\z)")
$notes = if ($m.Success) { $m.Groups[1].Value.Trim() } else { 'See CHANGELOG.md.' }
$verify = "### Verify your download`n`nSHA-256 checksums are in SHA256SUMS.txt. Builds are not code-signed yet, so SmartScreen may warn; installed copies update themselves from signed packages."
[System.IO.File]::WriteAllText((Join-Path $root 'release-notes.md'), "$notes`n`n$verify`n", $utf8)

# Update feed for the in-app updater. It needs the installer's signature, which only a build
# with the signing key produces.
$sig = if ($setup) { Get-Item -LiteralPath "$($setup.FullName).sig" -ErrorAction SilentlyContinue } else { $null }
if ($sig) {
  $entry = [ordered]@{
    signature = (Get-Content -LiteralPath $sig.FullName -Raw).Trim()
    url       = "https://github.com/$Repo/releases/download/v$version/$setupName"
  }
  $feed = [ordered]@{
    version   = $version
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{ 'windows-x86_64' = $entry; 'windows-x86_64-nsis' = $entry }
  }
  [System.IO.File]::WriteAllText((Join-Path $out 'latest.json'), ($feed | ConvertTo-Json -Depth 5), $utf8)
} else {
  Write-Warning 'No installer signature found; latest.json was not written. Set TAURI_SIGNING_PRIVATE_KEY for app:build.'
}

$sums = Get-ChildItem -LiteralPath $out -File | Sort-Object Name | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
}
# LF endings, so `sha256sum -c SHA256SUMS.txt` works in Git Bash, WSL and Linux too.
[System.IO.File]::WriteAllText((Join-Path $out 'SHA256SUMS.txt'), (($sums -join "`n") + "`n"), $utf8)
Get-ChildItem -LiteralPath $out | Format-Table Name, Length -AutoSize
