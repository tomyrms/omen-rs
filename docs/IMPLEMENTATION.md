# Notes d'implémentation — OMEN Hub RS

Ce document relie chaque fonction de l'application au protocole qu'elle utilise et
indique ce qui a été **vérifié sur la machine** (OMEN 17-cm2xxx, 8BB0, BIOS F.10) et ce
qui reste **à vérifier**, avec la commande pour le faire.

## 1. Architecture

```
omen-hub.exe (manifest requireAdministrator)          omen.exe (CLI, pas de manifest)
        │                                                     │
   omen-gui : fenêtre egui, icône de notification,            │
              instance unique, masquage dans la zone          │
        │                                                     │
   omen-core::Controller ─ thread « omen-control » (1 Hz) ─────┤
        │   · BiosClient (WMI)       · PDH (charge, zones)    │
        │   · NVML (GPU NVIDIA)      · détection de jeux      │
        │   · AutoEngine             · réglages JSON          │
        └─ thread « omen-keyboard » : omen-kbd::Keyboard (HID)┘
```

- Les commandes de l'interface passent par un canal ; l'état sort en `Snapshot`.
  Aucun appel matériel n'est fait sur le thread de l'interface.
- Réglages : `%APPDATA%\omen-rs\config.json` (écriture atomique). Journal :
  `%APPDATA%\omen-rs\omen-hub.log` (rotation à 1 Mo).
- Fuite mémoire corrigée dans `omen-wmi` : chaque VARIANT (SAFEARRAY, objets
  embarqués) est libéré par `VariantClear` (`OwnedVariant`), indispensable pour un
  processus qui tourne en continu.

## 2. Interface BIOS (WMI `root\WMI`, `hpqBIntM`, signature `SECU`)

| Fonction | Commande | Charge utile | Statut |
|---|---|---|---|
| Mode thermique | `0x20008 / 0x1A` | `FF mode biosFan 00` | Accepté (code 0). Effet non mesurable à vide (voir 4). |
| Correspondance des modes | `SystemDesignData[3]` (politique V1) | Silencieux→`0x50` (L4), Équilibré→`0x30` (L2), Performance→`0x31` (L7) ; V0 : 3/0/1 | Table du Hub |
| Ventilateurs max | `0x27` / lecture `0x26` | `01|00` | ✓ vérifié (audible, niveaux qui montent) |
| Niveaux ventilateurs | `0x2E` | `cpu gpu fan3 00` (centaines de tr/min, 255 = inchangé) | ✓ vérifié (effet physique) |
| Lecture niveaux | `0x2D` | — | ✓ |
| Capteurs | `0x23` + index | 0 IR, 1 ambiant, 2 PCH, 3 VR | ambiant ✓ (41 °C) ; autres à vérifier |
| Puissance GPU | `0x21` / `0x22` | `TGP PPAB Dstate seuil` | ✓ aller-retour vérifié |
| Types/capacités ventilateurs | `0x2C` | quartets + bitmap | ✓ (`0x21` = CPU+GPU) |
| Table BIOS | `0x2F` | — | ✓ 14 paliers |
| Bridage thermique | `0x35` | — | ✓ lecture |
| Chargeur | famille `0x00001` / 15 | `[0]` état, `[3]×5` W | à vérifier : `omen info` |
| MUX GPU | `0x00001/0x52` lecture, `0x00002/0x52` écriture | — | lecture ✓ (Hybride) ; écriture derrière confirmation + redémarrage |
| ICC max | `0x37` | `00 0F lo hi` | rejeté (code 6) sur ce BIOS → non utilisé par l'interface |
| PL1/PL2/PL4 | `0x29` | `pl1 pl2 pl4 conc` | accepté sans relecture possible → CLI seulement |

Ce que chaque **mode** écrit (`omen-core/src/mode.rs`) :

| Niveau | 0x1A | TGP / PPAB | Max fan |
|---|---|---|---|
| Silencieux | Quiet (`0x50`) | off / off | off |
| Équilibré | Balanced (`0x30`) | on / off | off |
| Performance | Performance (`0x31`) | on / on | off |
| Max | Performance (`0x31`) | on / on | **on** |

Ordre d'écriture : `0x1A` d'abord, `0x27` (max fan) ensuite, comme le Hub. Constaté en
direct : en mode Max, le BIOS est revenu de lui-même à ~2000 tr/min en moins de 3 min.
Tant qu'un niveau demande max fan, l'application relit `0x26` toutes les 2 s, renvoie
`0x27 = 1` dès que la relecture dit « off », et le renvoie de toute façon toutes les 60 s.

L'octet `biosFan` de `0x1A` vaut 1 (le BIOS régule) sauf en courbe perso / vitesse fixe,
où l'application pilote `0x2E` elle-même ; à la fermeture elle rend la main au BIOS.

## 3. Mode Auto (`omen-core/src/auto.rs`)

Entrées chaque seconde : charge CPU (PDH), charge GPU (PDH « GPU Engine », sans réveiller
le GPU), température GPU (NVML, sur secteur), zone thermique ACPI, bridage BIOS,
secteur/batterie, jeu détecté.

1. **Trop chaud** (GPU ≥ 84 °C, zone ≥ 92 °C ou bridage BIOS) → **Max** immédiatement,
   maintenu jusqu'à 8 °C sous le seuil.
2. **Jeu** (option : seulement sur secteur, comme le Hub) → **Performance**.
3. **Forte charge** lissée (CPU ≥ 60 % ou GPU ≥ 45 %) → **Performance**.
4. **Repos** prolongé (CPU < 15 %, GPU < 10 % pendant 60 s) → **Silencieux**.
5. Sinon → **Équilibré**.

Lissage EWMA (montée 0,5 / descente 0,15, même principe que `SwFanControl` du Hub).
Montée après 4 s, descente après 40 s. Trois réactivités : Discret / Normal / Réactif.
Tests unitaires : `cargo test -p omen-core`.

**Détection de jeux** (`games.rs`) : exécutables dans une bibliothèque de jeux (Steam,
Epic, GOG, EA, Ubisoft, Riot, Xbox, Battle.net, Rockstar, Amazon, itch), liste de jeux
connus hors bibliothèque, liste de l'utilisateur, et tout programme Direct3D plein écran
au premier plan. Lanceurs, anti-triche et outils Steam (Wallpaper Engine…) exclus. Le Hub,
lui, s'appuie sur sa bibliothèque en ligne : nous n'utilisons aucun service réseau.

## 4. Clavier RGB (`omen-kbd`)

Contrôleur Primax USB `0461:4E9A` (« Cybug » dans le Hub), collection `MI_02`, rapports
de 65 octets `[0]=00 id, [1]=cmd, [2]=param, [3]=long, [4]=0, [5..]=données`. Le Hub pilote
ce modèle avec le **jeu de commandes « general »** du SDK MCU (`NbPerKeyRgbLightingControl`
→ `McuGeneralHelper`), et non le jeu V1 décrit au départ dans `03-clavier.md` — c'est ce
qui donnait du blanc.

Réponse du MCU à chaque commande : `[1]`,`[2]` en écho ; `[5] [6]` = `EC AC` accepté,
`EC FA` refusé.

| Fonction | Trame | Vérifié |
|---|---|---|
| Infos | `80 01 00 00` → firmware `[5..8]`, type `[9]`, langue, profil, effet `[12]`, luminosité `[13]` | ✓ firmware 0.3.21.0 |
| Allumer / éteindre | `09 00 01 00 01|00` | ✓ `EC AC` |
| Couleurs par touche | `05/06/07` (R/G/B) × pages 0,1,2 ; `[3]=0` ; 180 LED (3 × 60). 144 laissait les flèches et quelques touches éteintes : l'éditeur par touche du Hub pour ce clavier (Starmade/Cybug) utilise des index au-delà de 160 | ✓ 9 × `EC AC`, état relu = mode statique `0x0F` |
| Effet | `03 00 24 00` + effet, mode, nb couleurs, vitesse, luminosité, direction, onde, pluie, …, RGB×6 à partir de `[29]` | ✓ relecture `0x83` : respiration `02`, vague `0A` |
| Lecture effet | `83 00 00 00` | ✓ |
| Sauvegarde flash | `0A 00 02 00 AC 53` | envoyé, pas de réponse (normal ?) |
| Usine | `10 07 04 00 94 10 98 27` | à vérifier : `omen kbd-reset` |
| Luminosité | `0C 00 01 00 val` | **refusé** (`EC FA 01`) pour toutes les valeurs : réglée par la touche Fn ; l'application atténue la couleur à la place |

Codes d'effet : fixe 1, respiration 2, cycle 4, scintillement 7, traînée 8, onde 9,
vague 10, pluie 12, OMEN X 13, confettis 15, soleil 16, balayage 17 ; thèmes
Volcan 2, Jungle 3, Océan 4, Galaxie 5.

Matrice testée sur le firmware 0.3.21.0 (`cargo run -p omen-kbd --example effects`) :
- **fixe (1), soleil (16), balayage (17) refusés** quels que soient les paramètres (balayage
  exhaustif) → retirés ; la couleur fixe passe par les cartes par touche.
- les **thèmes** (`[1]` = code du thème) ne sont acceptés que par **cycle** et **vague** ;
  pour les autres effets, l'application envoie la palette du thème en couleurs explicites.
- les 9 effets restants acceptent 1 à 4 couleurs et les vitesses 0..2 (108/108).

Jamais envoyé : `10 09 …` (passage en mode mise à jour du firmware).

## 5. Capteurs sans pilote

- PDH : `\Processor(_Total)\% Processor Time`, `\GPU Engine(*engtype_3D)\Utilization
  Percentage` (somme par adaptateur), `\Thermal Zone Information(*)\High Precision
  Temperature` (zone ACPI `TZ01`, ~48–52 °C au repos, varie). ✓
- NVML (`nvml.dll` du pilote NVIDIA, chargée dynamiquement) : température, puissance,
  charge. ✓ (RTX 4070 : 44 °C, 2,8 W au repos). Non interrogé sur batterie par défaut
  (interroger le GPU l'empêche de dormir).
- La température exacte des cœurs CPU demande un accès MSR (pilote noyau) : **hors
  périmètre**.

## 5 bis. Touche OMEN et OMEN Gaming Hub

- **Touche OMEN** : le BIOS émet l'événement WMI `root\WMI hpqBEvnt` avec `EventID = 29`,
  `EventData = 8613` (le Hub ne fait qu'ouvrir `omencommandcenter:` sur cet événement).
  L'application s'y abonne (`omen-wmi::BiosEvents`, `ExecNotificationQuery`, fonctionne sans
  droits admin) et exécute l'action choisie dans Réglages. Test : `cargo run -p omen-wmi
  --example events 30` puis appuyer sur la touche.
- **OMEN Gaming Hub** (`omen-core::vendor`) : paquet Store `AD2F1837.OMENCommandCenter`
  (ID Store `9NQDW009T0T5`). Désinstallation = `Remove-AppxPackage` ; option : arrêt et
  désactivation de `HPOmenCap`, `HPSysInfoCap`, `HPNetworkCap`, `HPAppHelperCap` et des
  tâches planifiées HP (auteur HP Inc.) qui lancent un programme de `HP\OmenInstallMonitor`,
  `HP\Overlay` ou `HP\SystemOptimizer*` (dont `SystemOptimizer`, sans « Omen » dans le nom). L'état
  d'origine (type de démarrage, en cours ou non) est sauvegardé dans
  `%APPDATA%\omen-rs\hub-backup.json` avant toute modification ; « Réinstaller comme avant »
  rétablit les services puis installe le paquet par `winget` (repli : page du Store). Les
  pilotes HP ne sont jamais touchés.

## 6. Démarrage automatique

Tâche planifiée « OMEN Hub RS » pour l'utilisateur, déclencheur ouverture de session
(+5 s), **privilèges les plus élevés** (pas d'UAC à chaque démarrage), autorisée sur
batterie, sans limite de durée, argument `--tray`. Créée par `schtasks /Create /XML`
depuis l'application (onglet Réglages) ou `omen autostart on` (admin).

## 7. Vérifications

### Faites automatiquement (cette session, sans droits admin)

- 19 tests unitaires (`.\cargo-vs.cmd test --workspace`) ✓, clippy sans avertissement ✓
- Clavier : infos, couleurs, effets, lecture d'état sur le vrai matériel ✓
- Capteurs PDH + NVML + détection de jeux en direct ✓
- Fenêtre : démarrage caché (`--tray`), instance unique (2e lancement = réaffiche),
  croix = masquage, réouverture ✓ ; captures dans `docs/screenshots/`
- Installation (`install.ps1`) ✓

### À vérifier par vous (droits admin, ~5 min)

| Étape | Attendu |
|---|---|
| Lancer **OMEN Hub RS** (raccourci), accepter l'UAC | puces vertes « Administrateur » et « BIOS connecté », vitesses ventilateurs affichées |
| Cliquer **Max** | ventilateurs audibles à fond, badge MAX |
| Cliquer **Silencieux** puis **Performance** | message « Mode … appliqué », pas d'erreur rouge |
| Lancer un jeu en mode **Auto** | après ~4 s : « Auto (Performance) — jeu détecté : … » |
| Onglet Clavier : **Rouge**, puis **Vague** | le clavier change (vérifié côté MCU, à confirmer à l'œil) |
| Réglages : cocher **Lancer au démarrage** | message vert ; au redémarrage l'icône apparaît près de l'horloge sans UAC |
| `omen sensors` (terminal admin) | capteurs IR/PCH/VR et vitesses ventilateurs |
