# 02 — Composants (binaires & assemblies)

## 1. Paquet UWP — `C:\Program Files\WindowsApps\AD2F1837.OMENCommandCenter_<v>\OmenCommandCenterApp\`

### Cœur applicatif (les plus importants)

| Binaire | Rôle |
|---------|------|
| `OmenCommandCenterBackground.exe` | Hôte des modules background (daemons pannes/mailroom/PSU/télémétrie) |
| `HP.Omen.Core.Common.dll` | Cœur : `OmenHsaClient` (WMI+fallback driver), `ReadHWData` (client driver MSR), RPC/JSON, pipes, helpers |
| `HP.Omen.Core.Model.Device.dll` | `PerformanceControlHelper`, `PowerControlHelper`, `SwFanControl` (daemon courbe), enums/plateformes |
| `McuSDK2.dll` | SDK clavier/souris (protocoles V1/V2, templates, transports mailroom/HIDSDK) |
| `HIDSDK.dll` | SDK HID (fallback/service) |
| `PerformanceControl.dll` | Messages + enums (`PerformanceMode`, `ThermalControl`, `FanTable`…) |
| `HP.Omen.Background.FanControlBg.dll` | Pipe `HP.Omen.Background.FanControl<sid>` → `SetFanMode` |
| `HP.Omen.Background.PerformanceControl.dll` | Logique modes/perf (contexte, supervision, thermal) |
| `HP.Omen.Background.KeyboardV3Bg.dll` | Pipeline clavier « V3 » (settings/clavier récents) |
| `HP.Omen.Features.Hardware.HidMailroom.Background.dll` | **Mailroom HID** : pipe ↔ HID direct (`WriteFile` 65 o) |
| `HP.Omen.Features.Hardware.PerformanceMonitorBg.dll` | Télémétrie (temp CPU via driver, fan levels…) |
| `HP.Omen.Core.UI.*` | UI WPF (contrôles, thème, fenêtres) |
| `HP.Omen.*.Module.dll` (≈ 40) | Modules d'onglets (Accelerator, LightStudio, Fan…), plugins overlay |

### Sous-dossiers notables
- `Auto-OC\` — tâches planifiées undervolting/Auto-OC (`OmenAutoOC.exe` à installer dans `C:\Program Files\HP\Auto-OC`).
- `HPReadHWData\` — INF + installeurs du driver MSR (`HpReadHWData.inf`, `OMENDriverInstaller.exe`…).

## 2. Service `HPOmenCap` —
`C:\Windows\System32\DriverStore\FileRepository\hpomencustomcapcomp.inf_amd64_<hash>\x64\OmenCap\`

| Binaire | Rôle |
|---------|------|
| `OmenCap.exe` | **Le service** (LocalSystem) : serveur RPC `ncalrpc` + hôte des modules |
| `HIDService.dll` | Service HID : JSON (`HIDDataRequestStringParamJson`), SetupDi + `WriteFile` |
| `Device.dll` | Cœur matériel côté service (dont AMD OC/SMU ; signatures `CDriverHandler::RDMSR/WRMSR`) |
| `HomeInfoService.dll` | Inventaire système |
| `IntelXTUOverclockingService.dll` / `IntelOverclockingSDK.dll` / `Xtu*` | OC Intel (XTU SDK) |
| `AmdRyzenOverclockingService.dll` | OC AMD (SMU) |
| `RealtekNetworkingService.dll` | Réseau Realtek |
| `RpcClient.Native.dll` | **API client RPC** exportée (`GetClientForEndpoint`, `OpenConnection`, `ExecuteCommand`) |
| `SdkWrapperForNativeCode.dll` | Wrapper C++/CLI des SDK (vulcanisation Intel/AMD, HID…) |
| `Logging.dll`, `Platform.dll`, `ProfileHelperModel.dll`, `HPAnalyticsNativeClientLib.dll` | Support |

## 3. Services Windows

| Service | Binaire | Rôle |
|---------|---------|------|
| `HPOmenCap` | OmenCap.exe | Moteur matériel + RPC (clavier, EC, OC…) |
| `HPSysInfoCap` | — | « Fusion » : passerelle WMI système (fallback des appels BIOS) |
| `HPAppHelperCap` | — | HP App Enabling Services (nvml, SDK AMD, services divers) |
| `HPNetworkCap` | — | Réseau HP |
| `HpReadHWData` | HpReadHWData.sys | Driver MSR/PCI/ports (démarrage manuel/auto) |
| `HPOmenCustomCapDriver` | hpomencustomcapdriver.sys | ACPI HPIC0003/0004 |

## 4. Drivers

| Driver | Device | Notes |
|--------|--------|-------|
| `HpReadHWData.sys` | `\Device\HPReadHWData` (`\\.\HpReadHWData`) | MSR/PCI/IO/WMI kernel ; **politique d'accès 8 exes** (cf. 05) |
| `hpomencustomcapdriver.sys` | ACPI `SWC\HPIC0003`, `SWC\HPIC0004` | EC/plateforme OMEN |
| (tiers) | `AMDRyzenMasterDriverV31` etc. | évoqués par Device.dll (plateformes AMD) |
