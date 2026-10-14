# HP BIOS WMI command table (OMEN 17-cm2xxx, 8BB0)

Transport: `root\WMI`, class `hpqBIntM`, method `hpqBIOSInt<n>` (n = out size).
Input: embedded `hpqBDataIn` instance with properties
`Sign` (bytes `53 45 43 55` = "SECU"), `Command` (family), `CommandType`
(operation ID), `Size`, `hpqBData` (payload). Output: `OutData` instance with
`Data` + `rwReturnCode` (0 = OK, 3 = unsupported, 5 = size, -1 = client-side).

Command families: `0x20008` default, `0x20009` keyboard, `0x00001` legacy,
`0x00002` GPU mode switch.

| ID | Op | Direction | Evidence |
|----|----|-----------|----------|
| 0x10 | Get fan count | R | OmenMon + live (`02`) |
| 0x1A | Set fan performance mode | W | OmenMon (payload `FF <mode> 00 00`) |
| 0x21 | Get GPU power state | R | Hub IL (`BiosWmiCmd_Get 0x20008,0x21`) + OmenMon + live (`01 01 01 57`) |
| 0x22 | Set GPU power state | W | Hub IL (`BiosWmiCmd_Set 0x20008,0x22`) + OmenMon |
| 0x23 | Get thermal sensor (°C) | R | OmenMon + live (`0x29` = 41 °C) |
| 0x26 | Get max-fan state | R | OmenMon + live (`00`) |
| 0x27 | Set max-fan state | W | OmenMon (payload `01/00 00 00 00`) |
| 0x28 | Get system design data | R | OmenMon + live (see decoded struct below) |
| 0x29 | Set CPU power limits (PL1/PL2/PL4/concurrent) | W | Hub IL (`SetPowerLimit`, `SetConcurrentTdp`) + OmenMon |
| 0x2A | Get power data (layout TBD) | R | Hub IL (`GetPowerData`) + live (`00 00 00 82 82 00 C8 64 ..`) |
| 0x2B | Get keyboard type | R | OmenMon |
| 0x2C | Get fan type (nibbles) | R | OmenMon + live (`0x21` = CPU+GPU) |
| 0x2D | Get fan levels | R | OmenMon + live (`2E 30`) |
| 0x2E | Set fan levels | W | OmenMon (payload `<cpu> <gpu> 00 00`) |
| 0x2F | Get fan table | R | OmenMon |
| 0x32 | Set fan table | W | OmenMon (desktop-only per notes) |
| 0x35 | Get throttling/overclock state | R | OmenMon + Hub IL + live (`00 04 ..`) |
| 0x37 | Set PL4 (two-byte variant) | W | Hub IL (`SetPL4`, `SetTwoBytePowerLimit`) |

Live `GetSystemData (0x28)` decode on the target (2023 Omen 17):
status flags `0x014A`, thermal policy V1, support flags `0x01` (software fan
control), default PL4 200 W, concurrent limit with GPU 100 W, no BIOS OC,
GPU mode switch flags `0x06`.

## Raw COM notes (hard-won, important)
`omen-wmi` talks to the provider through `IWbemLocator`/`IWbemServices`
directly. Three quirks matter:

1. **Instance paths need doubled backslashes**: the working path is
   `hpqBIntM.InstanceName="ACPI\\PNP0C14\\0_0"` (the `\\` are two literal
   characters in the path string; the parser decodes them to `\`). A single
   `\` returns `WBEM_E_INVALID_OBJECT_PATH` (0x8004103A).
2. **`uint32` properties require a signed VARIANT**: `IWbemClassObject::Put`
   on `Command`/`CommandType`/`Size` must use a `VT_I4` VARIANT with the
   declared type `CIM_UINT32` (19). Passing `VT_UI4` fails with
   `WBEM_E_TYPE_MISMATCH` (0x80041005) on this provider. (Verified by
   brute-force in `omen connect-test`.)
3. **Embedded instance parameter**: the method's `InData` parameter is
   `CIM_OBJECT` (13); put the spawned `hpqBDataIn` instance as a
   `VT_UNKNOWN` VARIANT with type 13.

`Sign` and `hpqBData` are `uint8[]` (`VT_ARRAY|VT_UI1` with type `0x2011`).
Output: `OutData` is a `VT_UNKNOWN` embedded instance carrying `rwReturnCode`
and `Data`.

## Notes

- Writes touch real power/fan limits: keep values within the ranges the
  vendor application uses (PL1/PL2 up to the default PL4, concurrent limit as
  reported by `GetSystemData`).
- The Hub's background services (`OmenCommandCenterBackground`, `HPOmenCap`)
  also drive these interfaces; concurrent control can fight (last write wins).
- The `0x2A` layout: observed `00 00 00 82 82 00 C8 64` on the target, where
  `C8`/`64` match the default PL4 (200 W) and concurrent limit (100 W) from
  `GetSystemData`; `82 82` (130/130) are suspected PL1/PL2 defaults. The bytes
  did not change under CPU load nor after `0x29` writes (accepted, code 0),
  and `0x37` is rejected (code 6) on this BIOS — MSR access is needed to
  confirm live PL values.
- `0x23` (thermal sensor) did not react to 50 s of 12-thread CPU load on the
  target: it is likely the IR/ambient sensor, not the CPU package. CPU/GPU
  temperatures require the Embedded Controller or the HP driver path.

## Verified target observations (2026-10-14)

| Item | Result |
|---|---|
| Fan table `0x2F` | 2 fans, 14 levels, 21 °C → 50 °C, levels up to `0x2E`/`0x30` |
| Keyboard | `PerKeyRgb`; backlight raw `E4` (on) / `64` (off); 4-zone color table read/apply |
| MUX (`0x52` legacy) | `Hybrid` |
| TGP round-trip | `gpu-set 0 0 1 87` → readback `0/0`; `gpu-set 1 1 1 87` → readback `1/1` |
| Max fan | on → fan levels up (0x1C→0x23 CPU), off → down; state readback consistent |
| PL writes | `0x29` code 0, `0x37` code 6, no `0x2A` change |

