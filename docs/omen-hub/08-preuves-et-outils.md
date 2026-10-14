# 08 — Preuves, outils & méthodologie

## 1. Méthodologie

| Étape | Outil | Usage |
|-------|-------|-------|
| Analyse binaire native | **REA v6.4.0** + provider **Ghidra 12.1.4** (JDK 21) | `function`, `xrefs`, `search`, snapshots (`--snapshot`) |
| Assemblies .NET | **ilspycmd** (+ REA quand possible) | décompilation C# intégrale (repli si budget dépassé) |
| PE / imports / exports | **dumpbin** | tables d'import/export (RPC, HID, WMI…) |
| Chaînes | `strings-scan.ps1` (maison) | ASCII + UTF-16 par aiguilles multiples |
| Kernel debug prints | **DebugView CLI** (`dbgviewcli64.exe --accepteula -k -l <fichier>`) | logs `DbgPrint` du driver (filtre registre `Debug Print Filter` posé) |
| Handles | **handle64** (Sysinternals) | qui ouvre quel device |
| USB | **USBPcapCMD** + Wireshark | capture HID clavier (post-reboot) |
| Système | PowerShell + WMI + nos outils Rust | validation live (fans, TGP, MSR…) |

## 2. Où sont les artefacts (dossier local)

`C:\Users\jtoma\AppData\Local\Temp\opencode\omen-rea\`

- **Rapports REA** : `hpreadhwdata-decompile.txt`, `-B/-C/-D/-E/-F.txt`
  (politique d'accès, dispatcher, handlers, check final),
  `native-deepdive-report.txt` (Device.dll, HpReadHWData, HIDService…).
- **Snapshots Ghidra** : `snap-hpreadhwdata`, `snap-hidservice`, …
- **Sources décompilées (ilspycmd)** : `corecommon-src`, `mcusdk2-src`, `modeldevice-src`,
  `keyboardv3-src`, `kbdlightingv3-src`, `coreui-src`, `profiles-src`, `hidmailroom-src`,
  `ilspy-PerformanceControl`, `ilspy-HP.Omen.Background.PerformanceControl`, `autoochelper-src`…
- **Dumps JSON REA** : `*-members.json` (tables de membres managés).
- **Scripts** : `strings-scan.ps1`, `hpreadhwdata-decompile-*.ps1`, `run-handles.cmd`,
  `svc-fix.ps1`, `run-dbgview*.cmd`…
- **Outils téléchargés** : `ghidra_12.1.4_PUBLIC`, `sysinternals*` (handle/dbgview),
  `USBPcapSetup.exe`.
- **Cibles binaires** : copies de `HpReadHWData.sys`, `OmenCap\*.dll`, app UWP.

Repo principal : `C:\Users\jtoma\omen-rs` (crates Rust `omen-wmi`, `omen-cli`, `omen-gui`,
scripts `kbd-*.ps1`, `msr-read.ps1`, `fan-test.cmd`…).

## 3. Repro express (REA)

```powershell
$env:GHIDRA_INSTALL_DIR = 'C:\Users\jtoma\AppData\Local\Temp\opencode\omen-rea\ghidra_12.1.4_PUBLIC'
$env:JAVA_HOME = 'C:\Program Files\Eclipse Adoptium\jdk-21.0.12.101-hotspot'
$env:REA_ANALYSIS_PROVIDER = 'ghidra'
rea xrefs     C:\WINDOWS\system32\drivers\HpReadHWData.sys 0x140004a24 --snapshot <snap>
rea function  C:\WINDOWS\system32\drivers\HpReadHWData.sys 0x140004e40 --snapshot <snap>
rea search    <binaire> "<chaîne>" --kind strings
```

Décompile managée : `ilspycmd <dll> -o <dossier>`.

## 4. Limites & honnêteté

- Le driver MSR reste **inaccessible** à nos processus (politique par contenu de fichier —
  pas de contournement : ce serait un abus de confiance de la protection HP).
- La capture USB clavier est **en attente du reboot** (filtres USBPcap).
- Une partie du pipeline clavier exact (ordering/ACK du mailroom) reste à confirmer
  par capture réseau/USB.
- Les mesures « physique » (fans audibles, LED) sont validées manuellement à chaque étape.

## 5. Crédits & références

- **REA** (Reverse Engineering Assistant) + **Ghidra** — analyse native.
- **OmenMon** — référence factuelle (tables de commandes WMI/EC).
- **Sysinternals** (Handle, DebugView), **USBPcap**, **ILSpy** — outillage.
- Aucun code propriétaire HP n'est redistribué dans ce dossier : uniquement de la
  **documentation d'ingénierie inverse**.
