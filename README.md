# omen-rs

Rust rewrite of the OMEN Gaming Hub **hardware control path** on OMEN laptops,
built from reverse-engineering evidence gathered on a real machine
(OMEN by HP Laptop 17-cm2xxx, board 8BB0, BIOS F.10, OMEN Gaming Hub 1101.2609.11.0).

The proprietary UI, store, cloud and analytics layers are out of scope; this
project reimplements the local hardware control that actually matters:
fans, thermal readings, power limits (PL1/PL2/PL4, concurrent), GPU power
(custom TGP / PPAB), and fan performance modes.

## Why this works

The OMEN Gaming Hub talks to the hardware through the HP BIOS WMI interface
(`root\WMI`, class `hpqBIntM`, methods `hpqBIOSInt0/4/128/1024/4096`).
The protocol was established from:

- **REA static analysis** of the Hub's .NET assemblies on the target machine
  (command IDs observed in the IL of `HP.Omen.Background.PerformanceControl.dll`
  and friends, e.g. `BiosWmiCmd_Set(0x20008, 0x29)` for CPU power limits);
- the **public protocol notes of OmenMon** (GPL-3.0, used strictly as factual
  interface documentation: parameter classes, the `SECU` sign, command IDs);
- **live read-only verification** against the machine (see
  `docs/bios-wmi-commands.md`).

This crate is an original implementation and contains no OmenMon code.

The interface requires an **elevated token** (Administrator), exactly like the
vendor application and OmenMon.

## Layout

```
crates/omen-wmi   Protocol client: hpqBIntM / hpqBIOSInt<n> over COM (windows crate)
crates/omen-cli   "omen" CLI: info, fans, temp, power, gpu-power, and setters
docs/             Command table with provenance (IL evidence / OmenMon / live test)
```

## Build

The machine's PATH has MSYS2's GNU `link.exe` ahead of MSVC's, so plain
`cargo build` can pick the wrong linker. Use the wrapper:

```powershell
.\cargo-vs.cmd build --release      # loads vcvars64, then runs cargo
.\cargo-vs.cmd check --workspace
```

(Requirements: rustup msvc toolchain, Visual Studio C++ build tools,
Windows 11 SDK — all already present on the development machine.)

## Usage (from an elevated terminal)

```powershell
.\target\debug\omen.exe info          # capabilities, fans, GPU power, thermal policy
.\target\debug\omen.exe fans          # current fan levels + max-fan state
.\target\debug\omen.exe temp          # thermal sensor + throttling state
.\target\debug\omen.exe power         # raw 0x2A power data
.\target\debug\omen.exe fan-table     # BIOS fan table (0x2F)
.\target\debug\omen.exe kbd           # keyboard type, backlight (raw), color zones
.\target\debug\omen.exe backlight on|off|status
.\target\debug\omen.exe color4 RRGGBB:RRGGBB:RRGGBB:RRGGBB   # apply zone colors
.\target\debug\omen.exe gpu-mode [hybrid|discrete|optimus --yes]  # MUX (reboot)
.\target\debug\omen.exe watch --interval 2                        # monitoring loop
.\target\debug\omen.exe raw 0x21 4                               # raw command experiments
.\target\debug\omen.exe maxfan on     # write path: max fan speed on/off
.\target\debug\omen.exe fan-mode default|performance|cool|l0..l6|legacy-*
.\target\debug\omen.exe fan-level <cpu> <gpu>     # raw units, observed 0x15..0x39
.\target\debug\omen.exe gpu-set <custom_tgp> <ppab> <d_state> <peak_temp>
.\target\debug\omen.exe cpu-set <pl1> <pl2> <pl4> <with_gpu>   # 255 = leave unchanged
```

Or run the full test script: `run-omen-tests.ps1` (self-elevate via
`run-omen-tests-as-admin.cmd`).

## Status (live-verified on the target, 2026-10-14)

- [x] Protocol + command table extracted from the Hub IL (REA) and validated live
- [x] Rust client (`omen-wmi`): reads (system/fans/thermal/power/keyboard/MUX), writes (max fan, fan levels, fan mode, TGP, backlight, colors)
- [x] CLI (`omen`) + GUI (`omen-hub`: dashboard, fan curve editor, history graph, keyboard colors, tray)
- [x] Live round-trip: `max-fan on` ramps the fans; TGP min/max round-trips with readback; backlight toggles with raw readback (`E4`/`64`)
- [x] BIOS fan table read (`0x2F`, 14 points); MUX read (Hybrid); keyboard `PerKeyRgb`, 4-zone color table
- [ ] CPU/GPU temperatures: `0x23` does not track CPU load on this model (IR/ambient sensor); needs the EC or the HP driver path (`HPReadHWData.sys`)
- [ ] PL1/PL2 writes: accepted by `0x29` (code 0) but not reflected in the `0x2A` readback; `0x37` rejected (code 6). Needs MSR (`0x610`) verification
- [ ] Fan table write (`0x32`, desktop-only per vendor docs) and per-key RGB via HID (separate RE)
- [ ] Performance-mode sync with the Hub background service (named pipes)
- [ ] GPU mode write (`0x52`, reboot required) — implemented in the CLI, gated behind `--yes`
- [ ] Long-running ownership fix for SAFEARRAY lifetimes in `omen-wmi`

## GUI

```powershell
.\omen-hub.cmd        # self-elevating launcher (double-click friendly)
omen                  # no subcommand: same GUI
omen gui              # explicit
```

Dashboard (temperatures, fan levels, max-fan, fan modes, GPU power presets),
history graph (temp + fan %), software fan curve editor (14+ editable points,
applied every 2 s via `0x2E`), keyboard zone colors, MUX status, and a system
tray (show, max fan on/off, fan modes, curve toggle, quit). Curve and mode are
persisted in `%APPDATA%\omen-rs\config.json`. When not elevated, the GUI shows
a banner with a one-click "relaunch as admin".
