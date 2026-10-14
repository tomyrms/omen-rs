# 03 — Clavier : protocole MCU et chaînes de contrôle

**Matériel :** clavier OMEN 17-cm2xxx → MCU USB **VID `0x0461` / PID `0x4E9A`**,
collection de contrôle **MI_02**.

```
Chemin d'ouverture : \\?\HID#VID_0461&PID_4E9A&MI_02#7&163dbf4&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}
Rapport sortant     : 65 octets — [0]=0 (report id), [1]=cmd, [2]=param/effet, [3]=len, [4]=0, [5..64]=data
Réponses            : F0 EC FA xx = OK     |     80 EC AC 00 = rejet
```

## 1. Identification du MCU (commande 0x80 = GetDeviceInfo)

Lancée sans données, la réponse contient (offsets dans la réponse du MCU) :

| Offset | Champ |
|--------|-------|
| 6–7 | version firmware |
| 8–9 | device ID |
| … | `_profileNo` (choix du template), effectCode, langue (`_languageCode`) |

Sur cette machine : **profileNo = 1** → protocole **V1** (`KeyboardCommandHelper`) ;
profil 2 → `KeyboardCommandHelperV2`. Firmware `03 21`, effet par défaut 3 (rainbow),
numéro de série `6MF35AA#ABL1CV8 24 0001`.

## 2. Table des commandes V1 (extraite de `McuSDK2.KeyboardCommandHelper`)

| [1] cmd | [2] | [3] len | Données ([5..]) | Rôle |
|---------|-----|---------|------------------|------|
| `0x03` | `Effect` | 22 | `AssigningColorIndex, ShowMode, ColorNumber, 5×(R,G,B), LedSpeed, Intensity, Direction, RaindropFrequency` | Effet d'éclairage (arc-en-ciel, vague…) |
| `0x03` | `Effect` | 8 | `RIn GIn BIn BrightIn ROut GOut BOut BrightOut` | Effet audio |
| `0x03` | `stream[0]` | 22 | stream | Effet « stream » |
| `0x03` | `1` | 22 | zéros | **Éclairage OFF** |
| `0x04` | `0` | 2 | `FC EA` | **User mode enable** |
| `0x04` | `1` | 18 | 18 octets (`FF` = toutes les touches modifiables) | **All keys can be changed** |
| `0x05` | page | 60 | 60×R | **Keymap Rouge** (3 pages : [2]=0,1,2) |
| `0x06` | page | 60 | 60×G | **Keymap Vert** |
| `0x07` | page | 60 | 60×B | **Keymap Bleu** |
| `0x09` | `0` | 1 | `FF` | **Éclairage ON** |
| `0x0A` | — | — | `AC 53` | Store en flash |
| `0x0C` | — | — | niveau | Luminosité |
| `0x10` | — | 4 | `94 10 98 27` | Restore to default |
| `0x80` | `0` | 0 | — | GetDeviceInfo |

Enums : `SetColorType { SET_KEY_R = 5, SET_KEY_G = 6, SET_KEY_B = 7 }` —
`ColorPaging { IC_LED_PAGE_1 = 0, IC_LED_PAGE_2 = 1, IC_LED_PAGE_3 = 2 }`.
Chaque keymap fait **180 octets** (3 pages × 60 touches) ; pour une couleur unie,
remplir chaque page avec la composante.

## 3. Séquence EXACTE de l'appli pour une couleur statique

Source : `HP.Omen.Core.Common` → `McuKBLightingHSAClient.SetKeyColor(handle, r, g, b, enableUserMode:true)` :

```
1. SetKeyBoardLightingOn            (0x09, data FF)
2. ~50 ms
3. SetUserModeEnable                (0x04/0, data FC EA)
4. ~50 ms
5. SetAllKeyCanBeChange(true)       (0x04/1, 18×FF)
6. ~50 ms
7. SetStaticLighting(R,G,B)         → 9 commandes : R(0x05)×page0..2, G(0x06)×page0..2, B(0x07)×page0..2
8. ~50 ms
9. SetAllKeyCanBeChange(false)      (V1 ignore le paramètre et re-envoie 18×FF)
```

`SetKeyOff` = étape `0x03/1` (off). Les effets passent par `0x03` avec le layout ci-dessus.

## 4. Les trois transports possibles (côté appli)

1. **HidMailroomBg (préféré)** — pipe `\\.\pipe\HP.Omen.Background.HidMailroomBg<SessionId>`
   (messages sérialisés `IoObject`). Le processus background ouvre le HID
   (SetupDi + `HidD_*`) et écrit avec **`WriteFile` direct** sur l'interface MI_02.
2. **HIDSDK (fallback)** — `BasicIO_ForHidSDK` (SDK HID de HP).
3. **Service OmenCap / HIDService** — service LocalSystem qui fait la même chose
   côté service (requêtes JSON `HIDDataRequestStringParamJson`, `GetJsonValue`…).

> ⚠️ **Sans le service `HPOmenCap`, plus aucune couleur ne fonctionne dans l'appli.**
> Voir [07 — Incidents](07-incidents-depannage.md).

## 5. État des tests (2026-10-14)

- Écritures HID **directes** avec la séquence applicative exacte : toutes **acceptées**
  (WriteFile 65 octets OK) mais **aucun changement visible** — cause racine en cours
  d'investigation (capture USB au prochain reboot).
- L'appli HP était également HS : le service `HPOmenCap` était **arrêté** ; il a été
  redémarré (voir 07). Test de l'appli post-redémarrage en cours.
- Prochain jalon : **capture USBPcap** (reboot requis pour attacher les filtres) pendant
  un changement de couleur via l'appli → comparaison octet à octet avec nos trames.

## 6. Outils locaux (repo `omen-rs`)

| Script | Rôle |
|--------|------|
| `kbd-info.ps1` | GetDeviceInfo / identification du MCU |
| `kbd-set.ps1 -Mode full|keymaps|effect|off|default` | Écritures de test |
| `kbd-exact.ps1` | Séquence appli complète + lecture des réponses (4 phases R/V/B/M) |
| `kbd-red.ps1` | Idem réduit au rouge, laisse le clavier dans cet état |

Toutes écrivent en 65 octets sur `MI_02` (handle partagé, non exclusif).
