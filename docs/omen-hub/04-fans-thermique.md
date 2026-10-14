# 04 — Ventilateurs & modes thermiques

## 1. Chaîne applicative

```
UI (PerformanceControlModule)
  → pipe interne "HP.Omen.Background.FanControl<sid>"          (mode hérité Quiet/Normal/Turbo)
  → HP.Omen.Background.FanControlBg  → PerformanceControlHelper.SetFanMode(mode)
  → OmenHsaClient.BiosWmiCmd_SetSync(0x20008, 26, {0xFF, mode, 0, 0})   ← 0x1A = SetFanMode
```

Daemon **SW fan** (courbe personnalisée) : classe `SwFanControl` (Model.Device) calcule
les niveaux (EWMA des températures CPU/GPU/IR + `PlatformSettings.FanTable`) puis appelle
en continu :

```
SetSwFanControlLevel(cpu, gpu, fan3) → SetSync(0x20008, 46, {cpu, gpu, fan3})   ← 0x2E = SetFanLevel
```

## 2. Table WMI (base `0x20008` / méthode `131080`, signature `SECU`)

| Cmd | Type | Payload | Rôle |
|-----|------|---------|------|
| `0x1A` | 26 | `{0xFF, mode, fanControlByBios, 0}` | **SetFanMode** (modes thermiques) |
| `0x2D` | 45 | — (lecture 128 o) | **GetFanLevel** (niveaux courants, octets jusqu'au 1er 0) |
| `0x2E` | 46 | `{cpu, gpu, fan3}` | **SetFanLevel** (contrôle direct — effet physique vérifié) |
| `0x2F` | 47 | — | GetFanTable (table dynamique BIOS) |
| `0x2C` | 44 | — | GetFanType : octets 0..7 = types (nibbles), 8..9 = capacités (bitmap) |
| `0x26`/`0x27` | 38/39 | flag | Get/SetMaxFan |
| `0x37` | 55 | `{0, 15, ampères}` | SetIccMax |
| `0x01` | 1/15 | — | GetAdapterStatus (wattage adaptateur, `data[3]*5` W) |
| `0x28` | 40 | — (128 o) | **SystemDesignData** ← clés des capacités |

### SystemDesignData — octets clés
- `[0..1]` wattage adaptateur (>= 200/280 → PPAB, etc.)
- `[3]` **ThermalPolicyVersion** (V0/V1/V2)
- `[4]` bit0 = **SW fan control** supporté — bit1 = Extreme — bit2 = Extreme unlock

## 3. Modes (`PerformanceMode`, valeurs littérales)

```
Default=0  Performance=1  Cool=2  Quiet=3  Extreme=4  Eco=256
L0=0x10 L5=0x11 L1=0x20 L6=0x21 L2=0x30 L7=0x31 L3=0x40 L4=0x50
```

Mapping appliqué par l'appli si **ThermalPolicyVersion = V1** :

| Mode UI | Valeur écrite |
|---------|---------------|
| Default / Eco / Balance | **L2 = 0x30** |
| Performance | **L7 = 0x31** |
| Cool / Quiet | **L4 = 0x50** |

Si **V0** : seul `Eco→Default` est remappé (les valeurs 0..4 passent telles quelles).
Certains SystemID (8607, 8746–874A) sont forcés en V0.

> Lire la version réelle sur la machine : `omen raw 40 128` → octet [3].

## 4. Vérification physique

| Commande (admin requis) | Effet |
|--------------------------|-------|
| `omen fans` | Lecture niveaux (0x2D) + état max-fan |
| `omen fan-mode performance` | Écrit 0x1A `{FF, 0x31, 0, 0}` |
| `omen fan-mode default` | Écrit 0x1A `{FF, 0x30, 0, 0}` |
| `omen max-fan on|off` | Effet **audible** vérifié en live |
| `omen fan-level <cpu> <gpu>` | Écrit 0x2E (niveaux bruts, plage observée 0x15..0x39) |

⚠️ Hors admin : `ExecMethod: Impersonation failed – Access denied (0x80041003)`.
Le GUI (`omen-hub.exe`) embarque un manifest *requireAdministrator*.

## 5. Notes

- La courbe « software fan » du GUI réimplémente `SwFanControl` (EWMA + tables) et
  pousse des niveaux 0x2E ~1×/s.
- Les modes 0x1A ne changent pas les niveaux à l'arrêt (0) : leur effet est une
  **courbe** appliquée par l'EC sous charge — vérification avec charge CPU si besoin.
- `WitmodFanLibrary.dll` et `HP.Omen.Background.DaisyChainFanBg.dll` concernent les
  ventilateurs externes/daisy-chain (pas ce portable).
