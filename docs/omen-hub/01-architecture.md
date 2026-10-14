# 01 — Architecture d'ensemble

## Vue schématique

```
┌─────────────────────────── Paquet UWP AD2F1837.OMENCommandCenter ───────────────────────────┐
│                                                                                              │
│  HP.Omen.OmenCommandCenter.exe (UI WPF)  ──pipes──▶  OmenCommandCenterBackground.exe (BG)   │
│      │                                                      │                                │
│      │  modules UI (PerformanceControlModule, KeyboardV3…)   │ modules BG (FanControlBg,     │
│      │  HP.Omen.Core.* (Model, UI, Common…)                  │ HidMailroomBg, Performance-    │
│      │                                                      │ Control, KeyboardV3Bg, …)      │
│      └──────────────┬───────────────────────────────────────┘                                │
│                     │  RPC ALPC (ncalrpc)  « Hp-Bridge-Omen »                              │
└─────────────────────┼────────────────────────────────────────────────────────────────────────┘
                      ▼
      ┌───────── Service LocalSystem  HPOmenCap  (OmenCap.exe) ─────────┐
      │  serveur RPC + modules :  Device.dll   HIDService.dll           │
      │  HomeInfoService.dll  IntelXTUOC…  AmdRyzenOC…  RealtekNet…    │
      │  RpcClient.Native.dll (client API)  SdkWrapperForNativeCode…    │
      └───────┬────────────────────────────┬───────────────────────────┘
              │                            │
              ▼                            ▼
   WMI hpqBIntM (BIOS)          Drivers HP
   base 0x20008, sig « SECU »    • HpReadHWData.sys      (MSR/PCI/ports/WMI kernel)
                                 • hpomencustomcapdriver (ACPI HPIC0003/HPIC0004, EC)
                                 • services : HPSysInfoCap (Fusion), HPAppHelperCap, HPNetworkCap
```

## Composants et flux à retenir

- **UI UWP** (`HP.Omen.OmenCommandCenter`) : XAML/WPF + modules chargés par onglet.
- **Processus background** : héberge les daemons (mailroom HID, courbes fan, télémétrie,
  PSU…) et expose des **pipes nommés** consommés par l'UI.
- **Service `HPOmenCap`** (LocalSystem, autostart) : le vrai moteur matériel côté service ;
  expose un **RPC `ncalrpc`** aux endpoints `Hp-Bridge-Omen`, `Hp-Bridge-SystemInformation`,
  `Hp-Bridge-Network`. Sans lui : plus de clavier, plus de modes fan (cf. 07).
- **WMI BIOS `hpqBIntM`** (namespace `root\wmi`, classe binaire) : tous les réglages
  BIOS — base `0x20008`, signature `SECU`, commandes 0x1A/0x2D/0x2E/0x2F/0x28… (cf. 04).
  Accès : `ExecMethod` (admin) ou via le driver (fallback).
- **Driver `HpReadHWData.sys`** : MSR (rdmsr/wrmsr inline), PCI (`HalGetBusDataByOffset`),
  ports d'E-S (`in`/`out`), WMI kernel (`IoWMIExecuteMethod`), watchdog Intel
  (port 0x1854)… Accès réservé par politique à une liste de 8 exécutables HP (cf. 05).
- **Driver `hpomencustomcapdriver`** : appareils ACPI `SWC\HPIC0003` / `SWC\HPIC0004`
  (contrôle EC/fan custom côté BIOS).
- **Pipes internes appli** (cf. 06) : `HP.Omen.Background.HidMailroomBg<sid>`,
  `HP.Omen.Background.FanControl<sid>`, `HP.Omen.PerformanceControl.*`,
  `SystemVitalsInfoUtility.PerformanceMonitor<pid>`…

## Flux principaux

| Fonction | Flux |
|----------|------|
| Monitoring | WMI `0x20008` (fan levels 0x2D, temp IR…), perf counters, driver MSR (PL1/PL2, énergie), NVML/LHM |
| Modes fan | UI → pipe FanControl → 0x1A `{FF, mode, 0, 0}` ; courbe custom → daemon SW → 0x2E `{cpu,gpu,fan3}` |
| Clavier | UI → McuSDK2 → pipe HidMailroomBg → WriteFile HID (MI_02) ; fallbacks HIDSDK/service (cf. 03) |
| TGP/PPAB | WMI 0x22 + SysDesignData (0x28) pour les capacités |
| MUX GPU | WMI (0x52/0x53 get/set observés) + redémarrage |
| Backlight | WMI registre dédié (0x20009 famille) — les écritures « registre » ne pilotent PAS le MCU clavier |

## Détails complets
- Composants binaires : [02-composants.md](02-composants.md)
- Clavier : [03-clavier.md](03-clavier.md) — Fans/thermique : [04](04-fans-thermique.md)
- Driver MSR : [05](05-driver-hpreadhwdata.md) — RPC/pipes : [06](06-services-rpc-pipes.md)
