# Keyboard lighting reverse-engineering notes (Primax MCU, OMEN 17-cm2 / 2023)

Target: internal RGB keyboard, USB `VID_0461&PID_4E9A` (Primax), vendor HID
collection `MI_02` ("Périphérique fournisseur HID").

Device interface path (open with `CreateFile`):

```
\\?\HID#VID_0461&PID_4E9A&MI_02#...#{4d1e55b2-f16f-11cf-88cb-001111000030}
```

found under `HKLM\SYSTEM\CurrentControlSet\Control\DeviceClasses\{4d1e55b2-f16f-11cf-88cb-001111000030}`.

Requires a **write handle** (GENERIC_WRITE) for reports and a separate
**read handle** (GENERIC_READ) for replies. No elevation needed to open the
vendor collection.

## Report format

Every command is a 65-byte output report:

| offset | meaning |
|---|---|
| 0 | report id (0) |
| 1 | command |
| 2 | parameter 1 (page / effect / zone, command-specific) |
| 3 | payload length |
| 4 | 0 |
| 5..64 | payload (pad with zeros) |

Replies come back as 65-byte input reports on the read handle.

## GetDeviceInfo (0x80)

Command: `[0, 0x80, 1, 12, 0, ...]`. Reply (observed):

```
00 80 01 29 D0 00 | 03 21 | 00 | 01 | 01 | 03 | 04 | ...
fw=03 21         | deviceID=00 | lang=01 | profileNo=01 | effectCode=03 | brightness=04
```

- `profileNo` selects the command template in the vendor SDK
  (`0/1 = KeyboardCommandHelper (V1)`, `2 = KeyboardCommandHelperV2`).
  This keyboard reports **1 → V1**.
- `effectCode=03` while the keyboard shows the default rainbow animation.
- Bytes ~25+ contain an ASCII keyboard identifier (e.g. `6MF35AA#ABL1CV8 24 0001`).

## V1 command set (what this keyboard uses)

`Effect` lives in **byte 2** in V1 (in V2 it lives in the payload byte 0) -
the two templates are wire-incompatible for lighting commands.

| cmd | name | layout |
|---|---|---|
| 0x03 | SetLightingEffect | `[1]=3, [2]=Effect, [3]=22, [5]=AssigningColorIndex, [6]=ShowMode, [7]=ColorNumber, [8..22]=5xRGB, [23]=LedSpeed, [24]=Intensity, [25]=Direction, [26]=RaindropFrequency` |
| 0x05/6/7 | SetStaticLighting (per-key R/G/B maps) | `[2]=page(0..2), [3]=60, [5..64]=60 key values` |
| 0x04 page 0 | SetUserModeEnable | `[3]=2, [5]=0xFC, [6]=0xEA` |
| 0x04 page 1 | SetAllKeyCanBeChange | `[3]=18, [5..22]=0xFF` |
| 0x09 | SetKeyBoardLightingOn | `[3]=1, [5]=0xFF` |
| 0x0C | SetAllKeyboardBrightness | `[3]=1, [5]=level` |
| 0x0A | StoreLightingEffectDataToFlash | `[5]=0xAC, [6]=0x53` |
| 0x10 | RestoreLightingSettingToDefault | `[3]=4, [5..8]=0x94,0x10,0x98,0x27` |

`ShowMode.SINGLE_COLOR = 0`; a `Brightness` enum (100/75/50/25 %) exists for
0x0C; effects are enumerated (rainbow observed as 3).

## Status

- Writes are accepted on the wire (WriteFile OK, 65 bytes each), the device
  interface is correct (GetDeviceInfo reply fully decoded), and every command
  is acknowledged by the MCU.
- **Reply forensics**: replies echo the command (`00 <cmd> <param>`) followed
  by a 4-byte signature. `F0 EC FA xx` for accepted commands,
  `80 EC AC 00` for R-map page 0 writes and restore-default. Whether the
  latter is an error code is still open.
- Application writers (`OmenCommandCenterBackground`, `OmenCap.exe`) were
  killed and a magenta static state was pushed for 12 s: the rainbow
  animation persisted, so it is not streamed by the (user-level) app - it is
  either the MCU firmware default effect (effectCode 3) or driven by the
  `HPOmenCap` service (which could not be stopped with Stop-Service).
- The static-color application sequence is still unresolved; blind attempts
  used the exact V1 layouts with effects 0-3, both with and without the
  user-mode/keys-changeable unlock. No visible change.
- Next planned step: USB traffic capture (USBPcap installed and service
  started; the root-hub filter needs a reboot to attach on this machine)
  while the vendor application changes a color - that yields the definitive
  sequence.

## Sources

- IL reconstructed with REA (`inspect-managed-members`) and ilspycmd
  decompilation of `McuSDK2.dll`, `HP.Omen.AccessoryKeyboardLightingModuleV3`,
  `HP.Omen.Background.KeyboardV3Bg`, `HP.Omen.Features.Hardware.HidMailroom.Background`,
  `HP.Omen.Core.UI.Common` (all shipped with OMEN Gaming Hub 1101.2609.11.0).
- Live experiments on the target: interface discovery, GetDeviceInfo
  round-trip, write tests (see `live-test-result.txt` history in the repo
  working tree).
