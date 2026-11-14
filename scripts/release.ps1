# Builds the Windows release: the setup.exe installer and a portable .exe,
# collected in `release/` together with their SHA-256 checksums.
#
# The binaries must not contain anything about the machine that built them.
# Rust embeds source paths (used in panic messages), so every local prefix
# (the repository, the Cargo and rustup folders, the user profile) is remapped
# to a neutral name. At the end the uncompressed files are scanned for the
# user name and profile path: if they show up, the build fails.
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

npm run build
if ($LASTEXITCODE -ne 0) { throw "npm run build failed" }

$version = (Get-Content apps/desktop/src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
$exe = Join-Path $root 'target/release/PersonAIly Hub.exe'
$setup = Get-ChildItem (Join-Path $root 'target/release/bundle/nsis') -Filter "*_${version}_x64-setup.exe" | Select-Object -First 1
if (-not $setup) { throw "setup.exe not found in target/release/bundle/nsis" }

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
$scanned = @($exe) + (Get-ChildItem apps/desktop/dist -Recurse -File | ForEach-Object FullName)
$leaks = foreach ($f in $scanned) { foreach ($n in Find-Leaks $f) { "$f contains '$n'" } }
if ($leaks) { $leaks; throw "machine-specific data found in the build" }
Write-Host "Scan: no user name, profile path or computer name in $($scanned.Count) files."

$out = Join-Path $root 'release'
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory $out | Out-Null
Copy-Item $setup.FullName (Join-Path $out "PersonAIly-Hub_${version}_x64-setup.exe")
Copy-Item $exe (Join-Path $out "PersonAIly-Hub_${version}_x64-portable.exe")
Get-ChildItem $out -Filter *.exe | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content (Join-Path $out 'SHA256SUMS.txt') -Encoding ascii

Get-ChildItem $out | Format-Table Name, @{ n = 'MB'; e = { '{0:N1}' -f ($_.Length / 1MB) } }
