# OMEN Gaming Hub — Reverse engineering complet (dossier)

Ce dossier documente le fonctionnement interne **de A à Z** de l'OMEN Gaming Hub
(paquet Microsoft Store `AD2F1837.OMENCommandCenter`) tel que reconstruit par
analyse statique (REA + Ghidra) et dynamique sur un **OMEN 17-cm2xxx** (carte 8BB0,
BIOS F.10, Intel 13e gén, Hub 1101.2609.11.0).

## Sommaire

| # | Document | Contenu |
|---|----------|---------|
| 01 | [Architecture](01-architecture.md) | Vue d'ensemble : paquets, processus, services, drivers, flux de données |
| 02 | [Composants](02-composants.md) | Tableau de tous les binaires/assemblies et leur rôle |
| 03 | [Clavier](03-clavier.md) | Protocole MCU V1/V2 complet, séquences exactes, transports, outils |
| 04 | [Fans & thermique](04-fans-thermique.md) | Commandes WMI, modes, courbes, daemon SW fan |
| 05 | [Driver HpReadHWData](05-driver-hpreadhwdata.md) | Driver MSR/PCI/WMI : IOCTL, politique d'accès, dispatcher |
| 06 | [Services, RPC & pipes](06-services-rpc-pipes.md) | OmenCap, endpoints RPC, pipes internes, sécurité |
| 07 | [Incidents & dépannage](07-incidents-depannage.md) | Pannes constatées, procédures, checklists |
| 08 | [Preuves & outils](08-preuves-et-outils.md) | Méthodologie, outillage, où sont les artefacts |

Documents liés (racine `docs/`) :
- [omen-hub-architecture.md](../omen-hub-architecture.md) — carte A→Z initiale (conservée)
- [keyboard-lighting-re.md](../keyboard-lighting-re.md) — premières notes clavier
- [bios-wmi-commands.md](../bios-wmi-commands.md) — table WMI brute

## Résumé express

- L'appli = **coquille UWP** + **processus background** (`OmenCommandCenterBackground.exe`)
  + **service LocalSystem `HPOmenCap`** (`OmenCap.exe`) + **drivers**.
- Le contrôle matériel sérieux (clavier HID, EC, OC) vit **côté service** ;
  l'appli lui parle en **RPC ALPC** (`ncalrpc`, endpoints `Hp-Bridge-*`) et via
  des **pipes nommés** internes.
- Les zones BIOS passent par **WMI hpqBIntM** (base `0x20008`, signature `SECU`).
- Le driver **HpReadHWData.sys** expose MSR/PCI/ports d'E/S/WMI kernel à une
  **liste blanche de 8 exécutables HP** (vérification du chemin + du fichier).
- **Le service `HPOmenCap` arrêté = plus rien ne fonctionne dans l'appli**
  (clavier, modes fan, etc.). Voir [07](07-incidents-depannage.md).
