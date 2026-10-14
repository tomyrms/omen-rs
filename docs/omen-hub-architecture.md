# OMEN Gaming Hub: architecture map (reverse-engineered)

Target: HP OMEN 17-cm2xxx (board 8BB0, BIOS F.10), OMEN Gaming Hub
`AD2F1837.OMENCommandCenter` 1101.2609.11.0 on Windows 11 x64.
Method: REA static analysis (managed IL + Ghidra native) plus live experiments.

## 1. Process and package layout

```
AD2F1837.OMENCommandCenter (Store/UWP package)
├── OmenCommandCenterApp/            ~60 HP.Omen.* .NET modules + native helpers
│   ├── HP.Omen.Core.Common.dll      HSA client: WMI BIOS wrapper (BiosWmiCmd_*)
│   ├── HP.Omen.Core.Model.*         device model, HWID/PID tables, data structures
│   ├── HP.Omen.Background.*         background tasks (fan, perf, lighting, macros…)
│   ├── HP.Omen.Features.Services.*  services layer (profiles, games, login, AI…)
│   ├── HP.Omen.*LightingModule*     keyboard/accessory lighting UIs (Woody2, Winston…)
│   ├── McuSDK2.dll                  accessory MCU protocol SDK (HID + pipe transports)
│   ├── EffectEngine.dll             native: virtual-camera / effect engine (named pipes)
│   ├── omenmqtt.exe, HP.UnifiedLogin, BouncyCastle, OpenCV…  cloud/auth/vision helpers
│   └── runtimes/, x64/, Scripts/ramp-hook.js
└── HP.Omen.Application.Background.Tasks.Container  task container (exe + winmd)

DriverStore packages (cannot be rewritten; we reuse them):
├── hpomencustomcapdriver.inf   → HPOmenCustomCapDriver (ACPI\HPIC0004, "HP Omen Driver")
├── hpcustomcapdriver.inf       → HPCustomCapDriver    (ACPI\HPIC0003)
├── hpomencustomcapcomp.inf     → OmenCap.exe + native service DLLs (HSA services)
├── hpomencustomcapext.inf      → extension
└── (app-side) HpReadHWData.sys → \Device\HPReadHWData (kernel driver, "read HW data")

Running services/processes:
HPOmenCap "HP Omen HSA Service" (OmenCap.exe, native, DriverStore)
OmenCommandCenterBackground (UWP background tasks host)
HPAppHelperCap, HPDiagsCap, HPNetworkCap, HPSysInfoCap (HP HSA services)
```

`OmenCap` native service folder contains: `OmenCap.exe`, `Device.dll`, `Platform.dll`,
`HIDService.dll`, `AmdRyzenOverclockingService.dll`, `IntelXTUOverclockingService.dll`,
`IntelBenchmarkSDK.dll`, `RealtekNetworkingService.dll`, `RpcClient.Native.dll`,
`SdkWrapperForNativeCode.dll`, `HomeInfoService.dll`, `HPAnalyticsNativeClientLib.dll`.

RE note: `SdkWrapperForNativeCode.dll` is a C++/CLI **mixed-mode (managed)** PE -
Ghidra (and REA's Windows native boundary) rejects managed targets; managed
analysis of its IL via ilspycmd exposes mostly C++/CLI plumbing
(`SdkWrapperLibrary::HPClientTuningProposal` = vendor tuning/OC proposals).
The device/EC plumbing for the service is more directly reachable through the
signed drivers (`hpomencustomcapdriver.sys`, `HpReadHWData.sys`).

## 2. Control interfaces (protocols)

### 2.1 HP BIOS WMI — `root\wmi` `hpqBIntM` (implemented in `omen-wmi`)

Method `hpqBIOSInt<0|4|128|1024|4096>`, input `hpqBDataIn`
(`Sign`=`53 45 43 55`, `Command`=0x20008 default family, `CommandType`=op id,
`Size`, `hpqBData`), output `OutData{ Data, rwReturnCode }`.

Decoded + live-verified: system data (0x28), fan count/type (0x10/0x2C),
fan levels get/set (0x2D/0x2E), max fan (0x26/0x27), temperature (0x23),
throttling (0x35), GPU power get/set (0x21/0x22), CPU power (0x29),
power data (0x2A), fan table (0x2F), keyboard legacy registers (family
0x20009: backlight/color table), GPU mode (0x52). Details:
`docs/bios-wmi-commands.md` (incl. raw-COM quirks: doubled backslashes in
instance paths, VT_I4 for uint32 Put, `CoSetProxyBlanket` in GUI processes).

**Limits found on this platform**: fan-mode writes (0x1A) are accepted but
have no effect (real switching is EC-side); keyboard 0x20009 writes
round-trip but do not drive the per-key LEDs (HID is the real path).

### 2.2 Embedded Controller (EC)

Fan modes (`HPCM=0x95`), real CPU/GPU temperatures (e.g. `CPUT=0x57`),
fan RPM reads — all EC offsets per the public OmenMon EC map. No userland
path exists out of the box; the vendor stack reaches the EC through its
signed drivers. Candidate reuse paths (in progress):
- `\Device\HPReadHWData` (kernel driver, running) — likely read-only
  hardware/EC reads (temperature!), IOCTLs to be extracted.
- `HPOmenCustomCapDriver` device interface `{fbd28b66-2802-4476-8237-f8ece47f9b61}`
  (`\\?\ACPI#HPIC0004#...#{...}`) — EC read/write used by OmenCap.
- `SdkWrapperForNativeCode.dll` / `Device.dll` inside OmenCap — the code
  issuing those IOCTLs (Ghidra analysis).

### 2.3 Keyboard/accessory MCU over HID (Primax)

Device: `HID\VID_0461&PID_4E9A&MI_02` ("périphérique fournisseur HID"),
interface `{4d1e55b2-f16f-11cf-88cb-001111000030}`, write + read handles,
no elevation needed. 65-byte reports (`[0]=0, [1]=cmd, [2]=param, [3]=len,
[4]=0, [5..]=payload`). Replies are 65-byte input reports.

- `GetDeviceInfo` (0x80) decoded live: firmware 3.21→bytes, profileNo=1
  (**V1 command template**), effectCode (3 = rainbow), brightness, keyboard
  serial string.
- V1 command set extracted from `McuSDK2.dll` IL: 0x03 SetLightingEffect
  (effect in byte 2!), 0x05/6/7 per-key R/G/B maps (3 pages × 60 bytes),
  0x04 user-mode/keys-changeable, 0x09 lighting on, 0x0C brightness,
  0x0A store-to-flash, 0x10 restore default. Full table + experiments:
  `docs/keyboard-lighting-re.md`.
- Transport variants in the vendor stack: direct HID (`BasicIO_ForHidSDK`)
  and a named-pipe bridge to the `HidMailroom` background task
  (`Hp.Bridge.Client.SDKs.McuSDK2.PipeSDK`, pipes `<name> Command` /
  `<name> Report`, serialized `IoObject {Identity, CommandList, ...}`).

### 2.4 Named pipes between app components

Background tasks expose pipe servers and the UI modules connect as clients
(e.g. `FanControlBg` hosts a pipe server and handles message tags
`SetFanMode`/profiles; `PerformanceControl` sends pipe messages to modules
and the overlay; `SendPipeMessageOIM/PO`, `GetPerformanceMonitorPipeName`).

### 2.5 WMI events

`hpqBEvnt` provides system events (Omen key, hotkeys) — a Rust listener is
possible later for Omen-key integration.

### 2.6 Cloud

`omenmqtt.exe` + `HP.Omen.Core.Mqtt.Background` (OMEN AI / store / deals /
telemetry) — server-side features, out of scope by design.

## 3. Functional subsystems (module groups)

| Subsystem | Modules (examples) | Status |
|---|---|---|
| Performance/power | `Background.PerformanceControl` (111 types: PL1/2/4, MSR, TGP/PPAB, AdaptivePowerControl, DynamicFanCurve, CleanCreek) | decoded; WMI writes validated (TGP) / PL unverifiable without MSR |
| Fans | `FanControlModule`, `Background.FanControlBg`, BIOS fan table | WMI + software curve implemented in `omen-rs` |
| GPU | `GraphicsSwitcherModule` | MUX read via WMI; write gated behind `--yes` |
| Lighting (keyboard) | `AccessoryKeyboardLightingModuleV3`, `Background.KeyboardV3Bg`, `McuSDK2` | protocol extracted; static-color application pending USB capture |
| Accessories | ~40 `Accessory*Module`/`Background.*` (headsets, mice, monitors, mousepads) | same MCU SDK; not started |
| Profiles | `Features.Services.Profiles.*` | identified (storage + profile-changed events) |
| Overclocking | `OmenCap`: Intel XTU / AMD Ryzen services, `*OverclockingModule`; app-side `Auto-OC/` (`OmenAutoOC.exe`, `OmenAutoOcHelper.exe`, `IntelOverclockingSDK.dll`, `XtuCommon.dll`, `HP_OMEN_Undervolting_Task.xml` scheduled task) | native SDKs inside OmenCap + managed helpers; AMD path uses `\\.\AMDRyzenMasterDriverV31` |
| Network | `NetworkBoosterModule`, `RealtekNetworkingService.dll` | not started |
| System optimizations | `SystemOptimizerModule/Bg` | not started |
| Omen key / hotkeys | `Background.OmenKey`, `HotkeyWindow`, WMI events | identified |

## 4. What omen-rs already replicates

`omen-wmi` (BIOS WMI client, all quirks solved) → `omen-cli` (`omen`) +
`omen-gui` (`omen-hub`: dashboard, software fan curve, history, tray).
Live-verified: fan levels, max fan, TGP min/med/max, system data, GPU mode
read, BIOS fan table read, and full monitoring.

## 5. Next investigation steps

1. USB capture (USBPcap installed; needs one reboot to attach) while the
   vendor app applies a keyboard color → exact MCU sequence → finish RGB.
2. Ghidra (REA, Windows provider) on `Device.dll` / `SdkWrapperForNativeCode.dll`
   / `OmenCap.exe` → EC IOCTL codes of `HPIC0004` and `\Device\HPReadHWData`
   → `omen-ec` module in Rust (fan modes, real temperatures, PL read).
3. WMI event listener for the Omen key (`hpqBEvnt`).
