; OMEN Hub RS installer (Inno Setup 6).
;
; Build:  installer\build.ps1            (compiles the release, then this script)
; Manual: ISCC.exe /DAppVersion=0.2.0 /DBinDir=..\target\release installer\omen-hub-rs.iss
;
; Installs for all users in Program Files (the app needs administrator rights
; anyway: HP's BIOS WMI interface requires them), creates shortcuts, can add
; `omen` to PATH and enable the start with Windows. Uninstalling removes the
; startup task, the PATH entry and, on request, the settings.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
#endif

#define AppName "OMEN Hub RS"
#define AppExe "omen-hub.exe"

[Setup]
AppId={{6E0F2B7C-3C1D-4D6A-9B6E-2F8A51C4D3E9}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=omen-rs
AppPublisherURL=https://github.com/tomyrms/omen-rs
AppSupportURL=https://github.com/tomyrms/omen-rs/issues
AppUpdatesURL=https://github.com/tomyrms/omen-rs/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=OMEN-Hub-RS-Setup-{#AppVersion}
SetupIconFile=..\assets\omen-hub-rs.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesEnvironment=yes
CloseApplications=no
; Removing the copy left by install.ps1 (per user) is intended.
UsedUserAreasWarning=no
LicenseFile=..\LICENSE
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} - installation

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
fr.TaskAutostart=Lancer {#AppName} au démarrage de Windows (réduit près de l'horloge)
en.TaskAutostart=Start {#AppName} with Windows (minimized to the notification area)
fr.TaskPath=Ajouter la commande « omen » au PATH (ligne de commande)
en.TaskPath=Add the "omen" command to PATH (command line)
fr.LaunchApp=Lancer {#AppName}
en.LaunchApp=Launch {#AppName}
fr.RemoveSettings=Supprimer aussi vos réglages et journaux (%APPDATA%\omen-rs) ?
en.RemoveSettings=Also delete your settings and logs (%APPDATA%\omen-rs)?
fr.Other=Options :
en.Other=Options:

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "autostart"; Description: "{cm:TaskAutostart}"; GroupDescription: "{cm:Other}"
Name: "addpath"; Description: "{cm:TaskPath}"; GroupDescription: "{cm:Other}"

[Files]
Source: "{#BinDir}\omen-hub.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\omen.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
; Copy installed earlier by install.ps1 (per user).
Type: filesandordirs; Name: "{localappdata}\Programs\{#AppName}"
Type: files; Name: "{userdesktop}\{#AppName}.lnk"
Type: files; Name: "{userprograms}\{#AppName}.lnk"

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Modes, ventilateurs et clavier RGB des PC OMEN"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addpath; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\omen.exe"; Parameters: "autostart on"; Flags: runhidden waituntilterminated; Tasks: autostart
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM omen-hub.exe"; Flags: runhidden; RunOnceId: "KillApp"
Filename: "{app}\omen.exe"; Parameters: "autostart off"; Flags: runhidden; RunOnceId: "Autostart"

[Code]
const
  EnvKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

function NeedsAddPath(Dir: string): Boolean;
var
  Paths: string;
begin
  if not RegQueryStringValue(HKLM, EnvKey, 'Path', Paths) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') = 0;
end;

procedure RemoveFromPath(Root: Integer; Key, Dir: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(Root, Key, 'Path', Paths) then
    exit;
  P := Pos(';' + Uppercase(Dir), ';' + Uppercase(Paths));
  if P = 0 then
    exit;
  { P is 1-based in ';'+Paths: the entry starts at P in Paths (after its ';'). }
  if P = 1 then
    Delete(Paths, 1, Length(Dir) + 1)
  else
    Delete(Paths, P - 1, Length(Dir) + 1);
  RegWriteExpandStringValue(Root, Key, 'Path', Paths);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  { The running app (elevated, in the notification area) locks its exe. }
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM omen-hub.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(1500);
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    { PATH entry left by install.ps1 for the old per-user copy. }
    RemoveFromPath(HKCU, 'Environment', ExpandConstant('{localappdata}\Programs\{#AppName}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemoveFromPath(HKLM, EnvKey, ExpandConstant('{app}'));
    if (not UninstallSilent) and
       (MsgBox(CustomMessage('RemoveSettings'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(ExpandConstant('{userappdata}\omen-rs'), True, True, True);
  end;
end;
