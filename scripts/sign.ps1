# Signs one Windows executable with Authenticode, so SmartScreen and
# antivirus software see who published it.
#
# Two ways, chosen by the environment variables that are set:
#
# 1. SignPath (free certificate for open source projects, signing in the cloud):
#      SIGNPATH_API_TOKEN, SIGNPATH_ORGANIZATION_ID,
#      SIGNPATH_PROJECT_SLUG, SIGNPATH_POLICY_SLUG
#    Needs the SignPath PowerShell module (Install-Module SignPath).
# 2. A certificate installed on this computer, used with signtool.exe:
#      WINDOWS_CERT_THUMBPRINT  (SHA-1 thumbprint of the certificate)
#
# release.ps1 calls this script for app-server.exe and passes it to Tauri as
# `bundle.windows.signCommand`, so the app and the installer are signed
# *before* the updater signs the installer (signing afterwards would change
# the file and break the update signature).
#
# Usage:  powershell -File scripts/sign.ps1 <file.exe>

param([Parameter(Mandatory = $true)][string]$Path)

$ErrorActionPreference = 'Stop'
$Path = (Resolve-Path $Path).Path
$timestamp = 'http://timestamp.digicert.com'

if ($env:SIGNPATH_API_TOKEN) {
    foreach ($name in 'SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_SLUG', 'SIGNPATH_POLICY_SLUG') {
        if (-not (Get-Item "Env:$name" -ErrorAction SilentlyContinue)) { throw "$name is not set" }
    }
    Import-Module SignPath
    $signed = "$Path.signed"
    Submit-SigningRequest `
        -InputArtifactPath $Path `
        -ApiToken $env:SIGNPATH_API_TOKEN `
        -OrganizationId $env:SIGNPATH_ORGANIZATION_ID `
        -ProjectSlug $env:SIGNPATH_PROJECT_SLUG `
        -SigningPolicySlug $env:SIGNPATH_POLICY_SLUG `
        -OutputArtifactPath $signed `
        -WaitForCompletion
    Move-Item -Force $signed $Path
}
elseif ($env:WINDOWS_CERT_THUMBPRINT) {
    $signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if (-not $signtool) {
        # signtool comes with the Windows SDK: take the newest x64 one.
        $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending | Select-Object -First 1
    }
    if (-not $signtool) { throw "signtool.exe not found: install the Windows SDK" }
    & $signtool.Source sign /sha1 $env:WINDOWS_CERT_THUMBPRINT /fd sha256 /tr $timestamp /td sha256 /d 'PersonAIly Hub' $Path
    if ($LASTEXITCODE -ne 0) { throw "signtool failed on $Path" }
}
else {
    throw "No Authenticode signing configured: set SIGNPATH_* or WINDOWS_CERT_THUMBPRINT"
}

$check = Get-AuthenticodeSignature $Path
if ($check.Status -ne 'Valid') { throw "signature of $Path is $($check.Status): $($check.StatusMessage)" }
Write-Host "Signed $Path ($($check.SignerCertificate.Subject))"
