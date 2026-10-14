# Installs OMEN Hub RS for the current user (no admin needed):
#   - copies omen-hub.exe (window) and omen.exe (command line) to
#     %LOCALAPPDATA%\Programs\OMEN Hub RS
#   - creates Start menu and Desktop shortcuts
#   - adds the folder to the user PATH (so `omen` works in any terminal)
#
#   .\install.ps1             build must exist: .\cargo-vs.cmd build --release
#   .\install.ps1 -Uninstall  removes files, shortcuts and PATH entry
param([switch]$Uninstall)

$ErrorActionPreference = 'Stop'
$name = 'OMEN Hub RS'
$dest = Join-Path $env:LOCALAPPDATA "Programs\$name"
$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) "$name.lnk"
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) "$name.lnk"

function Remove-FromPath {
    $p = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($p) {
        $parts = $p.Split(';') | Where-Object { $_ -and ($_.TrimEnd('\') -ne $dest) }
        [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
    }
}

# The running app is elevated; a normal shell may not be allowed to stop it.
$running = Get-Process omen-hub -ErrorAction SilentlyContinue
if ($running) {
    try { $running | Stop-Process -Force; Start-Sleep 1 }
    catch { Write-Warning "Fermez OMEN Hub RS (icône près de l'horloge > Quitter) puis relancez ce script."; exit 1 }
}

if ($Uninstall) {
    Remove-Item $startMenu, $desktop -ErrorAction SilentlyContinue
    Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
    Remove-FromPath
    schtasks.exe /Query /TN $name *> $null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Démarrage automatique encore actif : décochez-le dans l'application avant de désinstaller, ou lancez en admin : schtasks /Delete /TN `"$name`" /F"
    }
    Write-Host "$name désinstallé."
    exit 0
}

$src = Join-Path $PSScriptRoot 'target\release'
foreach ($f in 'omen-hub.exe', 'omen.exe') {
    if (-not (Test-Path (Join-Path $src $f))) {
        Write-Error "$f introuvable : compilez d'abord avec .\cargo-vs.cmd build --release"
    }
}

New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item (Join-Path $src 'omen-hub.exe'), (Join-Path $src 'omen.exe') $dest -Force

$shell = New-Object -ComObject WScript.Shell
foreach ($lnk in $startMenu, $desktop) {
    $s = $shell.CreateShortcut($lnk)
    $s.TargetPath = Join-Path $dest 'omen-hub.exe'
    $s.WorkingDirectory = $dest
    $s.Description = 'Modes, ventilateurs et clavier RGB des PC OMEN'
    $s.Save()
}

$p = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not ($p -split ';' | Where-Object { $_.TrimEnd('\') -eq $dest })) {
    [Environment]::SetEnvironmentVariable('Path', (($p, $dest) -join ';').TrimStart(';'), 'User')
}

Write-Host "$name installé dans $dest"
Write-Host "Raccourcis : Bureau et menu Démarrer. Commande : omen --help (nouveau terminal)."
