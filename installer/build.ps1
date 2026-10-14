# Builds the release binaries and the installer (dist\OMEN-Hub-RS-Setup-<version>.exe).
#   .\installer\build.ps1            build + installer
#   .\installer\build.ps1 -NoBuild   installer only (binaries already in target\release)
# Needs Inno Setup 6: winget install JRSoftware.InnoSetup
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value

if (-not $NoBuild) {
    # Local machines: cargo-vs.cmd loads the MSVC environment; CI already has it.
    $cargoVs = Join-Path $root 'cargo-vs.cmd'
    Push-Location $root
    try {
        if (Test-Path $cargoVs) { & $cargoVs build --release } else { cargo build --release }
        if ($LASTEXITCODE -ne 0) { throw "cargo build a échoué ($LASTEXITCODE)" }
    } finally { Pop-Location }
}

$iscc = @(
    (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source,
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 introuvable (winget install JRSoftware.InnoSetup)' }

& $iscc "/DAppVersion=$version" "/DBinDir=$root\target\release" (Join-Path $PSScriptRoot 'omen-hub-rs.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC a échoué ($LASTEXITCODE)" }
Write-Host "Installateur : $root\dist\OMEN-Hub-RS-Setup-$version.exe"
