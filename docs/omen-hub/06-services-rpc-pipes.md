# 06 — Services, RPC et pipes

## 1. Service `HPOmenCap` (le cœur)

- Image : `…\DriverStore\…\hpomencustomcapcomp…\x64\OmenCap\OmenCap.exe`
- Compte : **LocalSystem**, démarrage **AUTO** (ATTENTION : s'il est arrêté, l'appli est
  totalement inopérante — cf. 07).
- Rôle : héberger le **serveur RPC** (`ncalrpc`) et les services matériels
  (HIDService, Device, OC Intel/AMD, HomeInfo, Realtek…).

### Serveur RPC (OmenCap.exe)
Imports observés : `RpcServerUseProtseqEpW`, `RpcServerRegisterIf3`, `RpcServerListen`,
`RpcImpersonateClient`, `RpcServerInqCallAttributes`… — protocole `ncalrpc` (ALPC local).
Endpoints (chaînes du binaire) :
```
Hp-Bridge-Omen            Hp-Bridge-SystemInformation            Hp-Bridge-Network
```

### Sécurité côté serveur
- `Hp::Bridge::Security::RpcSecurityHelper::IsClientRunningFromTrustedLocation`
  (via `RpcServerInqCallAttributes`).
- `Hp::Bridge::Security::HPAuthenticodeHelper::IsPathAllowed` — contrôle de chemin/signature.
- Gestionnaires **Win32** et **UWP** dédiés (`BridgeAccessManager`, `SetWin32Checker`,
  `SetUwpChecker`) avec **listes d'exemption** (`Win32WithNoHPManifestExemptionWhiteList`,
  `UwpWithNoHPManifestExemptionWhiteList`) + critères manifeste/version.
→ Un client tiers hors whitelist est rejeté à la connexion.

### API client native (exportée par `RpcClient.Native.dll`)
```cpp
std::shared_ptr<Native::RpcClient> GetClientForEndpoint(std::wstring endpoint);
int OpenConnection(std::wstring endpoint);   // + surcharge avec ServerInfo
int ExecuteCommand(std::wstring service, unsigned id, std::wstring input, std::wstring& output);
int CloseConnection();
```

### HIDService (module du service)
Requêtes **JSON** : structures `ReadRequestStringParamJson`, `HIDDataRequestStringParamJson`,
`CloseHandleParamJson` ; helpers `GetJsonValue/ParseJsonValue` ; log « Optional Compare
String Not Provided » (sélection du device par compare-string). Écrit le HID avec
`SetupDi… + WriteFile` (65/16 o).

## 2. Pipes nommés internes (appli)

| Pipe | Émetteur → rôle |
|------|-----------------|
| `HP.Omen.Background.HidMailroomBg<SessionId>` (+ ` Command`/` Report`) | McuSDK2 → mailroom → **HID direct** (préféré pour le clavier) |
| `HP.Omen.Background.FanControl<SessionId>` | UI → FanControlBg (`SetFanMode`) |
| `HP.Omen.PerformanceControl.UISync`, `PerformanceControlToAdvancedPerformanceTuning*`, `PowerOptionUiToPerformanceControlBg` | synchronisation UI ↔ BG |
| `SystemVitalsInfoUtility.PerformanceMonitor<pid>` | enregistrement du monitoring perf/CPU/GPU |
| `HP.Omen.Features.Hardware.AccessoryPowerSupplyUnitBg<SessionId>` | PSU enregistrée auprès du BG |
| `HP.Omen.Background.KeyboardV3Bg` (pipeline V3) | clavier via settings |

Sérialisation des messages : classes `PipeClientV2/PipeServerV2`, `PipeDataType.IoObject`
(chaînes + objets IoObject — voir `McuSDK2` : `StreamString`/`IoObjectMessage`).

## 3. Couche WMI/HSA (rappel)

- `OmenHsaClient` (Core.Common) : `BiosWmiCmd_Get/SetSync(command, commandType, data…)`.
- Voie directe : WMI `root\wmi` classe BIOS (base `0x20008`, signature `SECU`) — admin requis.
- Fallback : buffer « SECU » via le driver HpReadHWData (cf. 05 §5).
- « Fusion » : service `HPSysInfoCap` comme passerelle alternative (voir logs
  `Cannot get WMI from driver, try fusion`).

## 4. Pourquoi c'est important pour notre projet Rust

- **Fans** : nous n'avons pas besoin du RPC — les commandes WMI `0x1A/0x2D/0x2E/0x2F`
  suffisent (déjà implémentées, effet physique vérifié).
- **Clavier** : le chemin « mailroom » de l'appli écrit **exactement comme nous** (WriteFile
  direct sur MI_02) — la capture USB dira si nos trames diffèrent.
- **Écrire un client RPC tiers** est possible techniquement (exports disponibles) mais se
  heurtera aux contrôles de confiance du service (whitelist) — non tenté à ce stade.
