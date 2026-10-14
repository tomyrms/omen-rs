# 07 — Incidents & dépannage

## Incident #1 — « Rien ne fonctionne dans l'appli » (2026-10-14)

**Symptôme** : aucune couleur clavier, aucun mode fan, rien, même avec l'appli HP.

**Cause** : le service `HPOmenCap` était **arrêté** (start type AUTO, mais Status=Stopped).
Sans lui, l'appli ne peut plus piloter le matériel (le service héberge HIDService,
Device, OC… et le serveur RPC).

**Diagnostic** :
```powershell
Get-Service HPOmenCap            # Status: Stopped → SUSPECT
Get-Process OmenCap              # doit exister quand tout va bien
```

**Correctif** (admin) :
```powershell
Start-Service HPOmenCap
# vérifier : Get-Service HPOmenCap → Running ; OmenCap.exe présent
```

**Vérification post-fix** : relancer l'appli OMEN (le background apparaît :
`OmenCommandCenterBackground.exe`) puis retester clavier/fans.

**Prévention / check rapide** :
```powershell
Get-Service HPOmenCap,HPSysInfoCap,HPAppHelperCap,HPNetworkCap | Format-Table Name,Status
```

---

## Incident #2 — Clavier : écritures acceptées, pas d'effet visuel (EN COURS)

**Constat** : nos écritures HID directes (séquence applicative exacte, cf. 03) sont
acceptées (`WriteFile` = 65 octets OK) mais **aucun changement de couleur**.

**Hypothèses** :
1. État MCU/flux (init requise côté service/mailroom absent au moment des tests) ;
2. Luminosité/état user-mode limbo ;
3. Trame fine différente (ordering/ACK) — la capture USB tranchera.

**Prochaine étape décisive — capture USBPcap** (nécessite UN reboot pour attacher les
filtres) :
```powershell
# après reboot :
& "C:\Program Files\USBPcap\USBPcapCMD.exe" -I        # lister les hubs
& "C:\Program Files\USBPcap\USBPcapCMD.exe" -d \\.\USBPcap<N> -o C:\Users\jtoma\kbd-capture.pcap
#   → pendant la capture : changer la couleur dans l'appli (ou lancer kbd-exact.ps1)
#   → Ctrl+C ; analyser avec Wireshark (filtre : le device 0461:4e9a)
```
Comparer octet à octet nos trames vs celles de l'appli/mailroom.

---

## Incident #3 — WMI en non-admin : `0x80041003`

`ExecMethod: Impersonation failed – Access denied (0x80041003)` pour tout write WMI
(fan-mode, max-fan, TGP…) lancé depuis un shell non élevé.

- Le GUI `omen-hub.exe` embarque un **manifest requireAdministrator** → OK.
- La CLI `omen.exe` : relancer le shell en admin (ou utiliser le bouton « relaunch »
  du GUI).

---

## Incident #4 — `\\.\HpReadHWData` → err 87 (même en admin)

Ce n'est **pas** un bug : la politique d'accès du driver rejette les processus hors
liste (8 noms HP + contrôle de contenu de l'exe). Documenté dans [05](05-driver-hpreadhwdata.md).
Tester avec un process nommé `OmenCommandCenterBackground.exe` ne suffit pas (testé).

---

## Incident #5 — REA « metadata budget » sur assemblies obfusqués

Certains assemblies (Core.Common, McuSDK2, Model.Device) dépassent le budget de
métadonnées managées de REA (`Managed metadata exceeds the 67108864 byte representation
budget`). Contournement : **`ilspycmd`** (décompile parfaitement). Issue amont :
https://github.com/morluto/rea/issues/3449

---

## Checklist de dépannage express

```powershell
# 1. Services
Get-Service HPOmenCap,HPSysInfoCap,HPAppHelperCap | Format-Table Name,Status
# 2. Drivers
Get-Service HpReadHWData,HPOmenCustomCapDriver | Format-Table Name,Status
# 3. Devices
Get-PnpDevice | Where-Object { $_.InstanceId -match 'HPIC0003|HPIC0004' } | Format-Table Status,FriendlyName
# 4. Processus appli
Get-Process | Where-Object ProcessName -match 'Omen' | Select Id,ProcessName
# 5. Clavier (chemin HID contrôle)
Get-ChildItem 'Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\DeviceClasses\{4d1e55b2-f16f-11cf-88cb-001111000030}' | Where-Object PSChildName -match 'VID_0461&PID_4E9A&MI_02'
```
