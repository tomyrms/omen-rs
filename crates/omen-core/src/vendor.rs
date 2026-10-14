//! Removing and restoring HP's OMEN Gaming Hub.
//!
//! Uninstall removes the Store package and, optionally, stops and disables the
//! HP background services the Hub uses. The services' previous start types are
//! saved to `%APPDATA%\omen-rs\hub-backup.json` so "reinstall" can put
//! everything back as it was (services first, then the Store package).
//! Drivers are never touched: Windows Update owns them and our app needs the
//! BIOS WMI interface they expose.

use std::os::windows::process::CommandExt;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config::{data_dir, log_line};

/// Store package of OMEN Gaming Hub.
pub const PACKAGE: &str = "AD2F1837.OMENCommandCenter";
/// Microsoft Store product id of OMEN Gaming Hub.
pub const STORE_ID: &str = "9NQDW009T0T5";

/// HP services that only serve the Hub (OMEN, system info, network booster,
/// app helper). Diagnostics/analytics services belong to other HP tools.
pub const SERVICES: &[&str] = &["HPOmenCap", "HPSysInfoCap", "HPNetworkCap", "HPAppHelperCap"];

/// Scheduled tasks HP installs with the Hub, matched by author "HP Inc." and
/// by the program they start (`...\HP\<folder>\...`): install watchdog,
/// in-game overlay, "Optimizer" (system cleaner). Task names vary (some carry
/// the user SID, `SystemOptimizer*` has no "Omen" in it), the programs do not.
pub const TASK_PROGRAMS: &[&str] = &["OmenInstallMonitor", "Overlay", "SystemOptimizer"];

/// Processes started by those tasks.
const TASK_PROCESSES: &str = "OmenInstallMonitor,OverlayHelper,SystemOptimizer";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskState {
    pub name: String,
    pub enabled: bool,
    pub running: bool,
}

/// Groups of HP tasks, one per program: (label, description, process name).
pub const TASK_GROUPS: &[(&str, &str, &str)] = &[
    ("Optimizer (nettoyage système)", "« optimisation » du Hub, inutile ici", "SystemOptimizer"),
    ("Overlay en jeu", "affichage FPS/températures du Hub en jeu", "OverlayHelper"),
    ("Surveillance de l'installation", "répare/met à jour le Hub", "OmenInstallMonitor"),
];

impl TaskState {
    pub fn label(&self) -> &'static str {
        if self.name.contains("Overlay") {
            TASK_GROUPS[1].0
        } else if self.name.contains("Optimizer") {
            TASK_GROUPS[0].0
        } else {
            TASK_GROUPS[2].0
        }
    }

    /// Event-triggered twins (`...CustomEvent...`) only run on demand.
    fn starts_at_logon(&self) -> bool {
        !self.name.contains("CustomEvent")
    }
}

/// One thing the user can switch on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Service(String),
    /// A task group label from [`TASK_GROUPS`].
    Tasks(&'static str),
}

/// What each HP service does, for the interface.
pub fn service_info(name: &str) -> (&'static str, &'static str) {
    match name {
        "HPOmenCap" => ("Service HP Omen", "commandes BIOS du Hub (ventilateurs, modes) — remplacé par OMEN Hub RS"),
        "HPSysInfoCap" => ("Service HP System Info", "infos système pour le Hub — remplacé par OMEN Hub RS"),
        "HPNetworkCap" => ("Service HP Network", "« boost réseau » du Hub"),
        "HPAppHelperCap" => ("Service HP App Helper", "aide au fonctionnement des applis HP"),
        _ => ("Service HP", ""),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceState {
    pub name: String,
    pub display: String,
    /// Registry start value: 2 automatic, 3 manual, 4 disabled.
    pub start: u32,
    pub delayed: bool,
    pub running: bool,
}

impl ServiceState {
    pub fn start_label(&self) -> &'static str {
        match (self.start, self.delayed) {
            (2, true) => "automatique (différé)",
            (2, false) => "automatique",
            (3, _) => "manuel",
            (4, _) => "désactivé",
            _ => "autre",
        }
    }

    fn sc_start(&self) -> &'static str {
        match (self.start, self.delayed) {
            (2, true) => "delayed-auto",
            (2, false) => "auto",
            (4, _) => "disabled",
            _ => "demand",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HubStatus {
    /// Installed version, if the Store package is present.
    pub version: Option<String>,
    pub services: Vec<ServiceState>,
    #[serde(default)]
    pub tasks: Vec<TaskState>,
    /// A backup from an earlier uninstall exists (reinstall restores it).
    pub has_backup: bool,
}

impl HubStatus {
    /// Something of the Hub is still installed or active.
    pub fn anything_left(&self) -> bool {
        self.version.is_some() || self.services.iter().any(|s| s.start != 4) || self.tasks.iter().any(|t| t.enabled)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Backup {
    version: Option<String>,
    services: Vec<ServiceState>,
    #[serde(default)]
    tasks: Vec<TaskState>,
}

fn backup_path() -> std::path::PathBuf {
    data_dir().join("hub-backup.json")
}

fn load_backup() -> Option<Backup> {
    std::fs::read_to_string(backup_path()).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Runs a PowerShell script hidden; returns stdout, or stderr/stdout on failure.
fn powershell(script: &str) -> Result<String, String> {
    let full = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8;$ErrorActionPreference='Stop';{script}");
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &full])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell introuvable : {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or(&stdout).to_string();
        Err(first)
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{program} : {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{program} a échoué (code {:?}) : {}", out.status.code(), text.lines().last().unwrap_or("")))
    }
}

fn read_services() -> Vec<ServiceState> {
    let names = SERVICES.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(",");
    let script = format!(
        "$r=@(); foreach($n in @({names})){{ $s=Get-Service -Name $n -ErrorAction SilentlyContinue; if($s){{ \
         $k=Get-ItemProperty \"HKLM:\\SYSTEM\\CurrentControlSet\\Services\\$n\" -ErrorAction SilentlyContinue; \
         $r+=[pscustomobject]@{{name=$n;display=$s.DisplayName;start=[uint32]$k.Start;delayed=([int]$k.DelayedAutostart -eq 1);running=($s.Status -eq 'Running')}} }} }}; \
         ConvertTo-Json -InputObject $r -Compress"
    );
    powershell(&script)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<ServiceState>>(&s).ok())
        .unwrap_or_default()
}

fn read_tasks() -> Vec<TaskState> {
    // `.` stands for the path separator (avoids escaping backslashes twice).
    let pattern = TASK_PROGRAMS.join("|");
    let script = format!(
        "$r=@(Get-ScheduledTask | Where-Object {{ $_.Author -like 'HP*' -and (($_.Actions.Execute -join ' ') -match 'HP.({pattern})') }} | \
         ForEach-Object {{ [pscustomobject]@{{name=$_.TaskName;enabled=($_.State -ne 'Disabled');running=($_.State -eq 'Running')}} }}); \
         ConvertTo-Json -InputObject $r -Compress"
    );
    powershell(&script)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<TaskState>>(&s).ok())
        .unwrap_or_default()
}

/// Current state of the Hub package and its services.
pub fn status() -> HubStatus {
    let version = powershell(&format!(
        "$p=Get-AppxPackage -Name {PACKAGE}; if($p){{ $p.Version }}"
    ))
    .ok()
    .filter(|v| !v.is_empty());
    HubStatus {
        version,
        services: read_services(),
        tasks: read_tasks(),
        has_backup: backup_path().exists(),
    }
}

/// Removes OMEN Gaming Hub. With `disable_services`, also stops and disables
/// the HP services listed in [`SERVICES`]. `progress` receives French status lines.
/// Saves the original state before the first change. The oldest backup is
/// kept (it describes the original installation); services or tasks it does
/// not know yet are added with their current state.
fn ensure_backup(before: &HubStatus) -> Result<(), String> {
    let mut changed = false;
    let mut b = load_backup().unwrap_or_else(|| {
        changed = true;
        Backup { version: before.version.clone(), ..Default::default() }
    });
    for s in &before.services {
        if !b.services.iter().any(|o| o.name == s.name) {
            b.services.push(s.clone());
            changed = true;
        }
    }
    for t in &before.tasks {
        if !b.tasks.iter().any(|o| o.name == t.name) {
            b.tasks.push(t.clone());
            changed = true;
        }
    }
    if changed {
        let _ = std::fs::create_dir_all(data_dir());
        std::fs::write(backup_path(), serde_json::to_string_pretty(&b).unwrap_or_default())
            .map_err(|e| format!("sauvegarde de l'état impossible : {e}"))?;
    }
    Ok(())
}

fn disable_service(s: &ServiceState) -> Result<(), String> {
    let _ = run("sc.exe", &["stop", &s.name]);
    run("sc.exe", &["config", &s.name, "start=", "disabled"]).map(|_| ()).map_err(|e| format!("service {} : {e}", s.name))
}

fn disable_task(t: &TaskState) -> Result<(), String> {
    powershell(&format!(
        "Stop-ScheduledTask -TaskName '{0}' -ErrorAction SilentlyContinue; Disable-ScheduledTask -TaskName '{0}' | Out-Null",
        t.name
    ))
    .map(|_| ())
    .map_err(|e| format!("tâche {} : {e}", t.name))
}

fn kill_group_process(label: &str) {
    if let Some((_, _, process)) = TASK_GROUPS.iter().find(|g| g.0 == label) {
        let _ = powershell(&format!("Get-Process {process} -ErrorAction SilentlyContinue | Stop-Process -Force"));
    }
}

/// Switches one service or one task group on or off. Switching on restores
/// the original start type from the backup (automatic when unknown) and
/// starts it.
pub fn set_enabled(item: &Item, enabled: bool) -> Result<String, String> {
    let before = status();
    ensure_backup(&before)?;
    let backup = load_backup().unwrap_or_default();
    log_line(&format!("vendor: set {item:?} -> {enabled}"));
    match item {
        Item::Service(name) => {
            let s = before.services.iter().find(|s| &s.name == name).ok_or("service introuvable")?;
            let label = service_info(name).0;
            if enabled {
                let start = backup
                    .services
                    .iter()
                    .find(|o| &o.name == name)
                    .filter(|o| o.start != 4)
                    .map(|o| o.sc_start())
                    .unwrap_or("auto");
                run("sc.exe", &["config", name, "start=", start]).map_err(|e| format!("service {name} : {e}"))?;
                let _ = run("sc.exe", &["start", name]);
                Ok(format!("{label} réactivé."))
            } else {
                disable_service(s)?;
                Ok(format!("{label} désactivé."))
            }
        }
        Item::Tasks(label) => {
            for t in before.tasks.iter().filter(|t| t.label() == *label) {
                if enabled {
                    powershell(&format!("Enable-ScheduledTask -TaskName '{}' | Out-Null", t.name))
                        .map_err(|e| format!("tâche {} : {e}", t.name))?;
                    if t.starts_at_logon() {
                        let _ = powershell(&format!("Start-ScheduledTask -TaskName '{}'", t.name));
                    }
                } else {
                    disable_task(t)?;
                }
            }
            if !enabled {
                kill_group_process(label);
            }
            Ok(format!("{label} : {}.", if enabled { "réactivé" } else { "désactivé" }))
        }
    }
}

pub fn uninstall(disable_services: bool, progress: &mut dyn FnMut(&str)) -> Result<(), String> {
    let before = status();
    ensure_backup(&before)?;
    log_line(&format!("vendor: uninstall (services: {disable_services}), before: {before:?}"));

    if before.version.is_some() {
        progress("Fermeture d'OMEN Gaming Hub…");
        let _ = powershell(&format!(
            "$p=Get-AppxPackage -Name {PACKAGE}; if($p){{ Get-Process | Where-Object {{ $_.Path -and $_.Path.StartsWith($p.InstallLocation) }} | Stop-Process -Force -ErrorAction SilentlyContinue }}"
        ));
        progress("Désinstallation du paquet…");
        powershell(&format!("Get-AppxPackage -Name {PACKAGE} | Remove-AppxPackage"))
            .map_err(|e| format!("désinstallation refusée : {e}"))?;
    } else {
        progress("OMEN Gaming Hub n'est pas installé.");
    }

    if disable_services {
        for s in &before.services {
            progress(&format!("Arrêt : {}…", service_info(&s.name).0));
            disable_service(s)?;
        }
        for t in &before.tasks {
            progress(&format!("Désactivation de la tâche HP « {} »…", t.label()));
            disable_task(t)?;
        }
        // Instances started before the tasks were stopped.
        let _ = powershell(&format!("Get-Process {TASK_PROCESSES} -ErrorAction SilentlyContinue | Stop-Process -Force"));
    }

    let after = status();
    log_line(&format!("vendor: after uninstall: {after:?}"));
    if after.version.is_some() {
        return Err("le paquet est toujours présent".into());
    }
    Ok(())
}

/// Puts back what [`uninstall`] changed: service start types, then the Store
/// package (winget, or the Store page as a fallback).
pub fn reinstall(progress: &mut dyn FnMut(&str)) -> Result<String, String> {
    if let Some(b) = load_backup() {
        for s in &b.services {
            progress(&format!("Rétablissement du service {}…", s.display));
            run("sc.exe", &["config", &s.name, "start=", s.sc_start()])
                .map_err(|e| format!("service {} : {e}", s.name))?;
            if s.running {
                let _ = run("sc.exe", &["start", &s.name]);
            }
        }
        for t in b.tasks.iter().filter(|t| t.enabled) {
            progress(&format!("Réactivation de la tâche HP « {} »…", t.label()));
            powershell(&format!("Enable-ScheduledTask -TaskName '{}' | Out-Null", t.name))
                .map_err(|e| format!("tâche {} : {e}", t.name))?;
            if t.running {
                let _ = powershell(&format!("Start-ScheduledTask -TaskName '{}'", t.name));
            }
        }
    }

    if status().version.is_none() {
        progress("Téléchargement d'OMEN Gaming Hub depuis le Microsoft Store (quelques minutes)…");
        let winget = run(
            "winget.exe",
            &[
                "install", "--id", STORE_ID, "--source", "msstore", "--accept-package-agreements",
                "--accept-source-agreements", "--disable-interactivity", "--silent",
            ],
        );
        if let Err(e) = &winget {
            log_line(&format!("vendor: winget failed: {e}"));
        }
        if status().version.is_none() {
            let _ = Command::new("explorer.exe")
                .arg(format!("ms-windows-store://pdp/?ProductId={STORE_ID}"))
                .spawn();
            return Ok("Services rétablis. L'installation automatique n'a pas abouti : la page du Microsoft Store est ouverte, cliquez sur « Installer ».".into());
        }
    }

    let _ = std::fs::remove_file(backup_path());
    log_line(&format!("vendor: after reinstall: {:?}", status()));
    Ok("OMEN Gaming Hub est réinstallé et ses services sont rétablis comme avant.".into())
}
