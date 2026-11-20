# Builds the Windows release: the setup.exe installer, a portable .exe and the
# VS Code extension (.vsix), collected in `release/` with their SHA-256 checksums.
#
# The binaries must not contain anything about the machine that built them.
# Rust embeds source paths (used in panic messages), so every local prefix
# (the repository, the Cargo and rustup folders, the user profile) is remapped
# to a neutral name. At the end the uncompressed files are scanned for the
# user name and profile path: if they show up, the build fails.
#
# The installer is also signed for the in-app updater (tauri-plugin-updater):
# TAURI_SIGNING_PRIVATE_KEY (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD, if the
# key has one) must be set, or the build stops. `latest.json`, the file the
# installed apps read to find updates, is written next to the installer with
# the release notes taken from CHANGELOG.md.
#
# Usage, from the repository root:  powershell -File scripts/release.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$userProfile = [Environment]::GetFolderPath('UserProfile')
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $userProfile '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $userProfile '.rustup' }

# Most specific first. CARGO_ENCODED_RUSTFLAGS separates flags with 0x1F,
# so paths with spaces stay intact.
$remaps = @(
    @($root, 'personaily-hub'),
    @($cargoHome, 'cargo'),
    @($rustupHome, 'rustup'),
    @($userProfile, 'user')
)
$flags = foreach ($r in $remaps) { "--remap-path-prefix=$($r[0])=$($r[1])" }
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1F
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue

if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    throw "TAURI_SIGNING_PRIVATE_KEY is not set: the updater needs signed installers (see CONTRIBUTING.md)"
}

$tauriConf = Get-Content apps/desktop/src-tauri/tauri.conf.json -Raw | ConvertFrom-Json
# Without the real public key no installed app could verify the update.
if ($tauriConf.plugins.updater.pubkey -notmatch '^[A-Za-z0-9+/=]{40,}$') {
    throw "plugins.updater.pubkey in tauri.conf.json is not a public key yet (see CONTRIBUTING.md)"
}
$version = $tauriConf.version
# Release notes for the updater: the CHANGELOG section of this version.
$changelog = Get-Content CHANGELOG.md -Raw
$pattern = '(?ms)^## \[' + [regex]::Escape($version) + '\][^\r\n]*\r?\n(.*?)(?=^## \[|\z)'
$section = [regex]::Match($changelog, $pattern)
if (-not $section.Success) { throw "CHANGELOG.md has no section for $version" }
$notes = $section.Groups[1].Value.Trim()

node scripts/build-server.mjs
if ($LASTEXITCODE -ne 0) { throw "app-server build failed" }
npm run tauri -w desktop -- build --config src-tauri/tauri.bundle.conf.json --config src-tauri/tauri.release.conf.json
if ($LASTEXITCODE -ne 0) { throw "app build failed" }

npm run package -w personaily-hub-vscode
if ($LASTEXITCODE -ne 0) { throw "VS Code extension packaging failed" }
$vsix = Join-Path $root 'extensions/vscode/personaily-hub.vsix'
$vsixVersion = (Get-Content extensions/vscode/package.json -Raw | ConvertFrom-Json).version
# The .vsix is a zip: scan what is inside, not the compressed bytes.
$vsixDir = Join-Path ([IO.Path]::GetTempPath()) "personaily-vsix-$PID"
if (Test-Path $vsixDir) { Remove-Item -Recurse -Force $vsixDir }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::ExtractToDirectory($vsix, $vsixDir)

$exe = Join-Path $root 'target/release/PersonAIly Hub.exe'
# The headless core, installed next to the app for the VS Code extension.
$server = Join-Path $root 'target/release/app-server.exe'
if (-not (Test-Path $server)) { throw "app-server.exe not found in target/release" }
$setup = Get-ChildItem (Join-Path $root 'target/release/bundle/nsis') -Filter "*_${version}_x64-setup.exe" | Select-Object -First 1
if (-not $setup) { throw "setup.exe not found in target/release/bundle/nsis" }
$signature = "$($setup.FullName).sig"
if (-not (Test-Path $signature)) { throw "updater signature $signature not found" }

# Look for anything that identifies this machine, in ASCII/UTF-8 and UTF-16.
$needles = @($env:USERNAME, $userProfile, $env:COMPUTERNAME) | Where-Object { $_ -and $_.Length -ge 3 } | Sort-Object -Unique
function Find-Leaks([string]$path) {
    $bytes = [IO.File]::ReadAllBytes($path)
    $ascii = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $utf16 = [Text.Encoding]::Unicode.GetString($bytes)
    foreach ($n in $needles) {
        if ($ascii.IndexOf($n, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
            $utf16.IndexOf($n, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $n }
    }
}
$scanned = @($exe, $server) +
    (Get-ChildItem apps/desktop/dist -Recurse -File | ForEach-Object FullName) +
    (Get-ChildItem $vsixDir -Recurse -File | ForEach-Object FullName)
$leaks = foreach ($f in $scanned) { foreach ($n in Find-Leaks $f) { "$f contains '$n'" } }
Remove-Item -Recurse -Force $vsixDir
if ($leaks) { $leaks; throw "machine-specific data found in the build" }
Write-Host "Scan: no user name, profile path or computer name in $($scanned.Count) files."

$out = Join-Path $root 'release'
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory $out | Out-Null
$setupName = "PersonAIly-Hub_${version}_x64-setup.exe"
Copy-Item $setup.FullName (Join-Path $out $setupName)
Copy-Item $exe (Join-Path $out "PersonAIly-Hub_${version}_x64-portable.exe")
Copy-Item $vsix (Join-Path $out "personaily-hub-vscode_${vsixVersion}.vsix")
Get-ChildItem $out -Include *.exe, *.vsix -Recurse | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content (Join-Path $out 'SHA256SUMS.txt') -Encoding ascii

# What installed apps read from .../releases/latest/download/latest.json.
# The URL points at the installer of this tag; it works once the draft is published.
$repo = 'https://github.com/ThomasCaputo/personaily-hub'
$latest = [ordered]@{
    version   = $version
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
            signature = (Get-Content $signature -Raw).Trim()
            url       = "$repo/releases/download/v$version/$setupName"
        }
    }
}
$utf8 = New-Object Text.UTF8Encoding $false
[IO.File]::WriteAllText((Join-Path $out 'latest.json'), ($latest | ConvertTo-Json -Depth 5), $utf8)
# The same notes for the GitHub release page (outside release/, which is uploaded as is).
[IO.File]::WriteAllText((Join-Path $root 'target/release-notes.md'), $notes, $utf8)

Get-ChildItem $out | Format-Table Name, @{ n = 'MB'; e = { '{0:N1}' -f ($_.Length / 1MB) } }
