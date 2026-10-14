# 05 — Driver `HpReadHWData.sys` (MSR / PCI / E-S / WMI kernel)

Analyse REA/Ghidra (snapshot `snap-hpreadhwdata`, rapports `hpreadhwdata-decompile-*.txt`).

## 1. Identité & device

- Service `HpReadHWData`, image `C:\Windows\System32\drivers\HpReadHWData.sys` (KMDF).
- Devices : `\Device\HPReadHWData`, symlink `\DosDevices\HPReadHWData`.
- Ouverture côté appli (`KernelDriver` de Core.Common) :
  `CreateFile("\\\\.\\HpReadHWData", GENERIC_READ|WRITE, share=0, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL)`.

## 2. IOCTL

```csharp
// Core.Common (décompilé)
IOControlCode(deviceType: 40001 /*0x9C41*/, function: 2306 /*0x902*/, Access.Any)
→ IOCTL = 0x9C412408 (METHOD_BUFFERED, FILE_ANY_ACCESS)
```

Deux usages :
- **entrée 8 octets (u64)** → `Rdmsr(index)` / `Rdpci(index)` (valeur renvoyée en out u64) ;
- **entrée 4116 octets (buffer)** → commandes composites « SECU » (voir §5).

## 3. Politique d'accès (pourquoi `CreateFile` renvoie err 87)

`NonPnpEvtDeviceFileCreate` → `FUN_140004a24` :

1. `FUN_140004634` : `ZwQueryInformationProcess(GetCurrentProcess(), ProcessImageFileName=27)`
   (résolu dynamiquement via `MmGetSystemRoutineAddress` ; échec → rejet) → chemin complet
   du processus appelant converti en ANSI.
2. `FUN_140004990` : `strstr(chemin, nom)` pour **8 noms** puis, si trouvé,
   `FUN_140004e40(chemin)` → ouvre et **lit le fichier EXE de l'appelant**
   (`FUN_140005bb8` → `FUN_1400026dc` → `FUN_140005cb8`, verdict) — contrôle de contenu
   (très probablement signature Authenticode / empreinte HP).
3. En cas de succès : `STATUS_SUCCESS` ; sinon `0xC000000D` (STATUS_INVALID_PARAMETER
   → erreur Win32 **87**).

> Conséquence pratique : **renommer/relocaliser un exe ne suffit pas** (testé : même nom,
> même dossier `OmenCommandCenterApp`, toujours err 87). Il faut être un vrai binaire HP.

### Les 8 noms autorisés (chaînes du driver)

```
HP.Omen.OmenCommandCenter.exe      HP.OMEN.OMENCommandCenter.exe
OmenCommandCenterBackground.exe    OMENCommandCenterBackground.exe
OMENOverlay.exe                    PerformanceControlTool.exe
HPCC.Bg.BackgroundApp.exe          HP.SystemControl.Background.exe
```

## 4. Dispatcher ARL (`FUN_140003d24`)

Trois commandes ASCII : `0x35`='5' **Monitor**, `0x36`='6' **Read**, `0x37`='7' **Write**,
avec un id + valeur + pointeur. Extraits décodés :

| Famille | Ids → action (via helpers) |
|---------|-----------------------------|
| Monitor | `rdmsr(0x194)`, `rdmsr(0x1B1)`, `rdmsr(0x19C)+rdmsr(0x198)` (thermique/perf combinés), `rdmsr(0x1A2)` (TjMax) |
| Read MSR | `0x1AD`, `0x1AE` (turbo ratios), `0x650`, `0x651`, `0x1FC` |
| Read WDT | port IO Intel `0x1854`, registre watchdog, `ReadIntelWDT` |
| Write | **read-modify-write** sur `0x1AD/0x1AE/0x650/0x651` (masques par champs), port `0x1854`, ARM/désarm du watchdog (`PsCreateSystemThread` de surveillance) |
| Divers | commandes internes `0x8000…` → `FUN_140005628` (WMI/EC via `FUN_140005554`), PCI/SMN par `HalGetBusDataByOffset` |

Helpers élémentaires confirmés :
- `FUN_140001204` = **`rdmsr`** inline + log `[ReadMSR] ID 0x[%X] return …`
- `FUN_1400013a8` = **`wrmsr`** inline + log `[WriteMSR] …`
- `FUN_140001368` / `FUN_1400011b4` = **`out`/`in`** + logs `[WriteIOPort]/[ReadIOPort]`
- Registre lu au boot : `\Registry\Machine\Software\HP\OMEN Ally\WatchDog` → `WatchDogStatus`
  (via `RtlQueryRegistryValuesEx`).

## 5. Protocole « SECU » (WMI BIOS через le driver)

Utilisé par `Core.Common.ExecuteBiosWmiCommandThruDriver` — buffer **4116 octets** :

```
[0..3]   = 'S','E','C','U'                    (0x53 0x45 0x43 0x55)
[4..7]   = command (LE u32)                   (ex: 0x20008)
[8..11]  = commandType (LE u32)               (ex: 40 = sys design data)
[12..15] = inputDataSize (LE u32)
[16..4111]= inputData (4096)
[4112]   = flag (b2)
```

→ envoye tel quel en entrée du IOCTL `0x9C412408` (variante buffer) ; le driver execute
la commande WMI BIOS en **kernel** (`IoWMIExecuteMethod`) et renvoie les 4096 octets de
sortie. C'est le **fallback** de l'appli quand la voie WMI userland échoue.

## 6. Côté client (Core.Common) — vues vérifiées

```csharp
ReadHWData.Rdmsr(1552 /*0x610*/, out eax, out edx)   // PL1/PL2
ReadHWData.Rdmsr(408 /*0x198*/, …)                   // perf status
ReadHWData.Rdpci(0xFF100003, out value)              // SMN/SMU AMD (ex.)
ReadHWData.Rdmsr(0xC0010299, …)                      // MSR AMD RAPL
```
Mutex global `Global\Access_PCI` pris avant chaque opération (sérialisation bus).

## 7. État / limites

- Ouverture depuis un processus non-HP : **rejetée (err 87)** — politique par contenu de
  fichier. Aucun contournement « propre » → notre Rust n'utilise pas ce driver ;
  les lectures MSR restent réservées aux binaires HP.
- Les fonctions du dispatcher sont documentées pour référence (compréhension complète),
  pas réutilisées.
