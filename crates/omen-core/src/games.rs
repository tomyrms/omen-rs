//! Game detection without any online service:
//! - processes installed in a game library folder (Steam, Epic, GOG, EA,
//!   Ubisoft, Riot, Xbox/Game Pass, Battle.net games...);
//! - a list of well-known game executables, plus the user's own list;
//! - a full-screen Direct3D application in the foreground (Windows reports
//!   it through `SHQueryUserNotificationState`).

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Folders (lower case) that contain installed games.
const LIBRARY_MARKERS: &[&str] = &[
    "\\steamapps\\common\\",
    "\\epic games\\",
    "\\gog games\\",
    "\\gog galaxy\\games\\",
    "\\ea games\\",
    "\\origin games\\",
    "\\ubisoft game launcher\\games\\",
    "\\riot games\\",
    "\\xboxgames\\",
    "\\battle.net\\games\\",
    "\\rockstar games\\",
    "\\amazon games\\library\\",
    "\\itch\\apps\\",
];

/// Launchers, helpers and anti-cheat services living in those folders.
const NOT_GAMES: &[&str] = &[
    "steam.exe",
    "steamwebhelper.exe",
    "steamservice.exe",
    "gameoverlayui.exe",
    "epicgameslauncher.exe",
    "epicwebhelper.exe",
    "galaxyclient.exe",
    "galaxyclient helper.exe",
    "gogalaxy.exe",
    "eadesktop.exe",
    "eabackgroundservice.exe",
    "origin.exe",
    "upc.exe",
    "uplaywebcore.exe",
    "ubisoftconnect.exe",
    "riotclientservices.exe",
    "riotclientux.exe",
    "riotclientuxrender.exe",
    "riotclientcrashhandler.exe",
    "riot client.exe",
    "vgc.exe",
    "vgtray.exe",
    "leagueclient.exe",
    "leagueclientux.exe",
    "leagueclientuxrender.exe",
    "battle.net.exe",
    "agent.exe",
    "rockstarservice.exe",
    "launcher.exe",
    "socialclubhelper.exe",
    "easyanticheat.exe",
    "easyanticheat_eos.exe",
    "beservice.exe",
    "crashreportclient.exe",
    "unrealcefsubprocess.exe",
    "unitycrashhandler64.exe",
    "unitycrashhandler32.exe",
    "cefsharp.browsersubprocess.exe",
    "gamebar.exe",
    "wallpaper32.exe",
    "wallpaper64.exe",
    "webwallpaper32.exe",
    "ui32.exe",
    "vrserver.exe",
    "vrmonitor.exe",
    "vrcompositor.exe",
    "vrdashboard.exe",
    "vrwebhelper.exe",
    "lghub_agent.exe",
    "obs64.exe",
    "discord.exe",
];

/// Steam tools installed like games but which are not games.
const NOT_GAME_FOLDERS: &[&str] = &[
    "\\steamapps\\common\\wallpaper_engine\\",
    "\\steamapps\\common\\steamvr\\",
    "\\steamapps\\common\\steamworks shared\\",
    "\\steamapps\\common\\steam controller configs\\",
    "\\steamapps\\common\\lossless scaling\\",
    "\\steamapps\\common\\soundpad\\",
    "\\steamapps\\common\\obs studio\\",
];

/// Popular games installed outside the usual library folders.
const KNOWN_GAMES: &[&str] = &[
    "league of legends.exe",
    "valorant-win64-shipping.exe",
    "fortniteclient-win64-shipping.exe",
    "minecraft.windows.exe",
    "javaw.exe",
    "r5apex.exe",
    "r5apex_dx12.exe",
    "cs2.exe",
    "dota2.exe",
    "gta5.exe",
    "gta5_enhanced.exe",
    "rdr2.exe",
    "overwatch.exe",
    "cod.exe",
    "wow.exe",
    "diablo iv.exe",
    "rocketleague.exe",
    "eldenring.exe",
    "cyberpunk2077.exe",
    "bg3.exe",
    "bg3_dx11.exe",
    "helldivers2.exe",
    "palworld-win64-shipping.exe",
    "destiny2.exe",
    "rainbowsix.exe",
    "pubg.exe",
    "tslgame.exe",
    "warframe.x64.exe",
    "genshinimpact.exe",
    "starrail.exe",
    "zenlesszonezero.exe",
    "forzahorizon5.exe",
    "forzahorizon4.exe",
    "starfield.exe",
    "hogwartslegacy.exe",
    "marvel-win64-shipping.exe",
    "deltaforce-win64-shipping.exe",
];

#[derive(Debug, Clone)]
pub struct RunningProcess {
    pub pid: u32,
    /// Lower-case file name.
    pub name: String,
    /// Lower-case full path, when readable.
    pub path: Option<String>,
}

pub fn processes() -> Vec<RunningProcess> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let end = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                out.push(RunningProcess {
                    pid: entry.th32ProcessID,
                    name,
                    path: image_path(entry.th32ProcessID),
                });
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn image_path(pid: u32) -> Option<String> {
    if pid == 0 || pid == 4 {
        return None;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase())
    }
}

/// Process id of the foreground window.
pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

/// A full-screen Direct3D application owns the screen.
pub fn fullscreen_d3d() -> bool {
    unsafe { SHQueryUserNotificationState().map(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN).unwrap_or(false) }
}

/// Decides whether a process is a game.
pub fn is_game(p: &RunningProcess, extra: &[String], ignored: &[String]) -> bool {
    let name = p.name.as_str();
    if ignored.iter().any(|g| g.eq_ignore_ascii_case(name)) {
        return false;
    }
    if extra.iter().any(|g| g.eq_ignore_ascii_case(name)) {
        return true;
    }
    if NOT_GAMES.contains(&name) {
        return false;
    }
    if KNOWN_GAMES.contains(&name) {
        // javaw.exe only counts for Minecraft launchers' game directory.
        if name == "javaw.exe" {
            return p.path.as_deref().map(|x| x.contains("minecraft")).unwrap_or(false);
        }
        return true;
    }
    match &p.path {
        Some(path) => {
            LIBRARY_MARKERS.iter().any(|m| path.contains(m))
                && !NOT_GAME_FOLDERS.iter().any(|m| path.contains(m))
        }
        None => false,
    }
}

/// Returns the name of a running game, preferring the foreground one.
pub fn detect(extra: &[String], ignored: &[String]) -> Option<String> {
    detect_in(&processes(), extra, ignored)
}

/// Same as [`detect`] on an existing process list.
pub fn detect_in(procs: &[RunningProcess], extra: &[String], ignored: &[String]) -> Option<String> {
    let fg = foreground_pid();
    let games: Vec<&RunningProcess> = procs.iter().filter(|p| is_game(p, extra, ignored)).collect();
    if let Some(g) = games.iter().find(|p| Some(p.pid) == fg) {
        return Some(g.name.clone());
    }
    if let Some(g) = games.first() {
        return Some(g.name.clone());
    }
    // Unknown game running full screen in the foreground.
    if fullscreen_d3d() {
        if let Some(p) = fg.and_then(|pid| procs.iter().find(|p| p.pid == pid)) {
            if !ignored.iter().any(|g| g.eq_ignore_ascii_case(&p.name)) && !NOT_GAMES.contains(&p.name.as_str()) {
                return Some(p.name.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, path: Option<&str>) -> RunningProcess {
        RunningProcess { pid: 1, name: name.into(), path: path.map(|s| s.into()) }
    }

    #[test]
    fn library_games_are_detected() {
        let g = p("hades.exe", Some("d:\\steamlibrary\\steamapps\\common\\hades\\x64\\hades.exe"));
        assert!(is_game(&g, &[], &[]));
        let launcher = p("steam.exe", Some("c:\\program files (x86)\\steam\\steamapps\\common\\steam.exe"));
        assert!(!is_game(&launcher, &[], &[]));
    }

    #[test]
    fn user_lists_win() {
        let x = p("mygame.exe", Some("c:\\games\\mygame.exe"));
        assert!(!is_game(&x, &[], &[]));
        assert!(is_game(&x, &["MyGame.exe".into()], &[]));
        let g = p("cs2.exe", None);
        assert!(is_game(&g, &[], &[]));
        assert!(!is_game(&g, &[], &["cs2.exe".into()]));
    }

    #[test]
    fn javaw_needs_minecraft_path() {
        assert!(!is_game(&p("javaw.exe", Some("c:\\java\\bin\\javaw.exe")), &[], &[]));
        assert!(is_game(&p("javaw.exe", Some("c:\\users\\a\\appdata\\roaming\\.minecraft\\runtime\\javaw.exe")), &[], &[]));
    }
}
