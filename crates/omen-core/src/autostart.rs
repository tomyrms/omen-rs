//! Launch at Windows logon.
//!
//! The app needs administrator rights (BIOS WMI writes). A `Run` registry key
//! would show a UAC prompt at every logon, so this registers a Task Scheduler
//! task for the current user with "run with highest privileges" instead —
//! the usual way to auto-start an elevated tray app silently. The task is
//! allowed to start on battery and has no time limit.

use std::os::windows::process::CommandExt;
use std::process::Command;

pub const TASK_NAME: &str = "OMEN Hub RS";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schtasks(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

/// Whether the logon task exists.
pub fn is_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn current_user() -> String {
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();
    if domain.is_empty() {
        user
    } else {
        format!("{domain}\\{user}")
    }
}

fn task_xml(exe: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Lance OMEN Hub RS (contrôle ventilateurs, modes et clavier) à l'ouverture de session.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT5S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>6</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe}"</Command>
      <Arguments>--tray</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        user = xml_escape(user),
        exe = xml_escape(exe),
    )
}

/// Creates (or replaces) the logon task for the running executable.
pub fn enable() -> Result<(), String> {
    let mut exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // From the command line (omen.exe), start the window app next to it.
    let hub = exe.with_file_name("omen-hub.exe");
    if hub.exists() {
        exe = hub;
    }
    let xml = task_xml(&exe.to_string_lossy(), &current_user());
    // schtasks expects the XML file in UTF-16 with a byte order mark.
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let path = std::env::temp_dir().join("omen-hub-rs-task.xml");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let out = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &path.to_string_lossy(), "/F"])
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if out.status.success() {
        Ok(())
    } else {
        Err(decode(&out.stderr).trim().to_string())
    }
}

pub fn disable() -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }
    let out = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(decode(&out.stderr).trim().to_string())
    }
}

/// Console output of schtasks is in the OEM code page; keep it readable.
fn decode(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap_or_else(|_| b.iter().map(|c| *c as char).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn xml_has_elevation_and_battery_settings() {
        let x = super::task_xml("C:\\a&b\\omen-hub.exe", "PC\\me");
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(x.contains("<DisallowStartIfOnBatteries>false"));
        assert!(x.contains("C:\\a&amp;b\\omen-hub.exe"));
        assert!(x.contains("<Arguments>--tray</Arguments>"));
    }
}
