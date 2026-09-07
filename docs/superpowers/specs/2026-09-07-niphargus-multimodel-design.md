# Support multi-modèles — le Niphargus, et ce qu'il révèle

Design validé le 2026-09-07. Cible : `KeSp_controller`.
Firmware de référence : `KeSp_firmware-gitlab`, branche `niphargus-bringup-2026-09`.

## Le problème

Le contrôleur suppose qu'il n'existe qu'un seul modèle de clavier. Cette
hypothèse est répartie en constantes dans quatre fichiers, et le Niphargus la
casse sur les trois axes à la fois : identité USB, géométrie de matrice, jeu de
commandes.

Le clavier doit **cohabiter** avec les cartes KaSe/KeSp existantes : le même
binaire pilote les deux, détectés au branchement.

## Faits établis, côté firmware

Identités USB (`boards/*/board.h`) :

| Carte | VID | PID | `PRODUCT_NAME` |
|---|---|---|---|
| kase_v1 / v2 / v2_debug | `0xCAFE` | `0x4001` | `KaSe V1` / `KaSe V2` / `KaSe V2 Debug` |
| niphar_left | `0xCAFE` | `0x4003` | `Niphargus Left` |
| niphar_right | `0xCAFE` | `0x4004` | `Niphargus Right` |
| kase_dongle | `0x303A` | `0x4001` | `KaSe Dongle` |
| conchodytes (souris) | `0x303A` | `0x4002` | `Conchodytes` |

Géométrie : `niphar_left/board.h:59-60` — `MATRIX_ROWS 4`, `MATRIX_COLS 7`.
Les cartes KaSe sont en 5 rangées (13 colonnes filaire, 14 via dongle).

Le dongle n'a plus de moteur keymap : `cdc_binary_cmds.c:1110-1117` n'enregistre
pas les commandes clavier, qui répondent `KS_STATUS_ERR_UNKNOWN`. Le firmware
documente l'intention : « un rejet muet serait pire que le défaut ».

Découverte de capacités : `KS_CMD_FEATURES` (0x02) renvoie une liste CSV. Le
dongle y ajoute `RF_DONGLE,RF_STATUS,RF_PAIR,BATTERY`. Le firmware écrit
explicitement (`cdc_binary_cmds.c:49-50`) que *le contrôleur utilise `RF_DONGLE`
comme discriminant*.

Géométrie déjà transmise, en deux endroits :
- le layout JSON déclare `"rows"` et `"cols"` (`boards/niphar_layout.inc:17-18`) ;
- `KEYSTATS_BIN` (0x40) préfixe sa réponse de `[rows:u8][cols:u8]`
  (`cdc_binary_cmds.c:389`).

## Ce qui casse aujourd'hui

1. `native.rs:165-190` — `find_kase_port()` exige VID `0xCAFE` **et** PID
   `0x4001`, sinon un produit contenant `KaSe`/`KeSp`. Le Niphargus ne satisfait
   ni l'un ni l'autre : **l'auto-connect ne le trouve pas.**
2. `native.rs:126-140` — `classify_kase_port()` matche sur le VID `0xCAFE` seul :
   le Niphargus **apparaît** dans la liste, étiqueté `KaSe Keyboard`. Les deux
   fonctions sont en désaccord.
3. `parsers.rs:7` — `ROWS: usize = 5` en dur. Un payload keymap 4×7 fait 57
   octets ; `28 % 5 != 0` → `parse_keymap` renvoie `Err`. **La keymap ne se parse
   pas.**
4. `stats.rs:32-48` — `col_to_hand_finger` code en dur le découpage
   0-5 / 6 / 7-12 et retombe sur `(Left, Pinky)` au-delà de la colonne 12. La
   colonne 13 du dongle 5×14 est donc **déjà** mal attribuée, avant le Niphargus.
5. `FEATURES` est bien envoyé (`native.rs:93-95`, commit `0f5ab12`) et
   `is_dongle` en est déjà dérivé via le tag `RF_DONGLE` — conformément à ce que
   le firmware attend. Mais la réponse est consommée puis **jetée** : seul le bit
   dongle en est extrait, alors qu'elle porte tout le jeu de capacités
   (`TAP_DANCE`, `COMBO`, `LEADER`, `RF_STATUS`, `BATTERY`…) sur lequel le gating
   d'onglets devrait s'appuyer.
6. `connection.rs:11-13` affirme en commentaire que le clavier et le dongle
   exposent le même jeu de commandes. C'est devenu faux.

## Décision

**L'appareil se décrit ; le contrôleur ne devine pas.** Pas de table de modèles
indexée par VID/PID : elle imposerait une modification du contrôleur à chaque
nouvelle carte, et mentirait dès que le firmware bouge sans elle.

L'USB ne sert plus qu'à reconnaître un port et à le nommer. Tout comportement
découle de ce que l'appareil déclare : capacités par `FEATURES`, géométrie par le
layout JSON.

## Design

### 1. `DeviceProfile` — ce que l'appareil dit de lui-même

Nouveau `src/protocol/device.rs` :

```rust
pub struct DeviceProfile {
    pub name: String,               // layout JSON "name", repli sur le produit USB
    pub rows: usize,
    pub cols: usize,
    pub features: BTreeSet<String>, // FEATURES CSV découpé
}
```

Construit une fois dans `connect_and_load` (`connection.rs:14`), diffusé par un
`BgMsg::Profile`. Seul porteur de la réponse à « quel clavier ». Aucun autre
module ne renifle l'USB.

### 2. La géométrie est une donnée, pas une constante

- `parse_keymap(data, rows)` prend les rangées en paramètre. La const `ROWS = 5`
  est **renommée** `DEFAULT_ROWS` (pas dupliquée) et n'est plus employée que
  lorsque rien n'est connu ; les appelants et les tests existants suivent le
  renommage.
- Résolution des rangées, dans l'ordre : `rows`/`cols` du layout JSON (déjà
  parsés puis jetés en `#[allow(dead_code)]`, `layout.rs:22-26`) → en-tête
  `KEYSTATS_BIN` → `DEFAULT_ROWS`.
- `stats.rs` : `ROW_NAMES` et `[0u32; ROWS]` dérivent du compte réel. Cinq
  rangées gardent les noms actuels ; quatre rangées perdent « Number ».

Deux appelants réels sont touchés : `native.rs:419` et `native.rs:435`.

### 3. Le split des mains vient du layout

Une règle « `col < cols/2` → gauche » est fausse dans le cas Niphargus : si le
maître ne rapporte que sa moitié 4×7, tout appartient à la main gauche et la
règle répondrait 3/4. Un simple « `x` par rapport au milieu du layout » a le même
défaut : le milieu d'une demi-matrice tombe au milieu de cette moitié, et
couperait une main en deux.

Ce qui distingue réellement deux mains d'une demi-matrice, c'est le **trou** —
la discontinuité horizontale entre les grappes. Les positions `x` sont déjà
disponibles (`KeycapPos`, `layout.rs:5-13`).

**Règle** : trier les touches par `x`, chercher le plus grand écart entre deux
touches consécutives. Au-delà de `1.5 u` (`1.5 × UNIT_PX`), le layout a deux
mains et la frontière passe dans ce trou. En deçà, il n'a qu'une main — et
*laquelle* n'est pas déductible d'une géométrie : elle se lit dans le nom
rapporté par l'appareil (`Niphargus Left` / `Niphargus Right`), avec la main
gauche par défaut si le nom ne dit rien.

Cette règle est correcte que le maître rapporte `4×7` ou `4×14` : une matrice
fusionnée exhibe le trou, une demi-matrice non.

L'attribution des **doigts** n'est pas dérivable d'une géométrie. La table KaSe
actuelle est conservée quand `cols == 13` (zéro régression). Sinon : les touches
d'une main sont ordonnées par `x`, la plus extérieure reçoit `Pinky`, puis
`Ring`, `Middle`, `Index` ; toute colonne supplémentaire vers l'intérieur reste
`Index`, et la rangée du bas est `Thumb`. C'est une approximation ergonomique,
documentée comme telle dans le code — elle ne prétend pas connaître la vraie
main de l'utilisateur.

### 4. L'USB nomme le port

Une seule fonction de reconnaissance, partagée par `classify_kase_port` et
`find_kase_port` — leur désaccord est la cause des bugs 1 et 2.

L'USB ne détermine plus aucun comportement, à une exception déclarée : le repli
de `is_dongle` quand `FEATURES` échoue (§5). Ce repli est une compatibilité
descendante avec les firmwares antérieurs à `FEATURES`, pas une source de vérité
— il est nommé comme tel dans le code.

Prédicat : VID `0xCAFE`, ou produit contenant `KaSe` / `KeSp` / `Niphargus` /
`Dongle`. Label : la chaîne produit telle que rapportée, p. ex.
`Niphargus Left (/dev/ttyACM0)`.

La souris Conchodytes (`0x303A:0x4002`) reste exclue — par intention désormais,
et non par accident : on ne matche plus sur le VID Espressif seul.

### 5. Conserver le jeu de capacités, pas seulement le bit dongle

**Déjà en place** : `native.rs:88-96` envoie `FEATURES` et dérive `is_dongle` du
tag `RF_DONGLE`, avec l'heuristique USB en repli si l'appel échoue (firmware
ancien). Rien à refaire.

**Ce qui reste** : la réponse est jetée après lecture d'un seul tag. Elle est
conservée dans `DeviceProfile.features`, et le gating d'onglets introduit au
commit `0f5ab12` interroge les tags concernés plutôt que le seul `is_dongle` —
un onglet Tap Dance se grise sur l'absence de `TAP_DANCE`, pas sur « c'est un
dongle ».

### 6. `ERR_UNKNOWN` n'est pas une erreur

`KrResponse` expose déjà `status` et `is_ok()` (`binary.rs:239-249`). Il gagne
`is_unsupported()` (`status == status::ERR_UNKNOWN`), et les appelants qui
peuvent tomber sur une commande non enregistrée distinguent « l'appareil ne fait
pas ça » de « l'appel a échoué ». C'est le contrat que le firmware annonce.

## Tests

Logique pure : test écrit d'abord, rouge avant implémentation, dans la phase
rapide (`cargo test`), parallel-safe. Fonctions concernées :

| Fonction | Ce que le test doit mordre |
|---|---|
| `parse_features_csv` | découpage, trim, entrées vides ignorées |
| `parse_keymap(data, rows)` | 4×7, 5×13, 5×14 ; octet de layer présent ou non ; longueur non multiple → `Err` |
| `resolve_geometry` | priorité layout > en-tête keystats > défaut |
| `hand_for_position` | moitié gauche seule ; clavier complet ; absence de layout |
| `row_labels(rows)` | 5 rangées inchangées ; 4 rangées sans « Number » |

Le ratchet `.tripwire-testcount` monte à partir de 23.

## Question ouverte, côté firmware

`KEYMAP_GET` sur le Niphargus renverra-t-il `4×7` (le maître seul) ou `4×14`
(les deux moitiés fusionnées) ? La keymap par défaut du maître est `4×7`, 26
touches, le reste en `K_NO` (`niphar_left/board_keymap.c:17-22`) — soit sa propre
moitié. Aucune carte n'existe encore.

Ce design n'a pas besoin de la réponse : il lit ce que l'appareil déclare. La
question reste ouverte pour le firmware.

## Hors périmètre

Aucune modification du firmware. Aucun refactor non lié. Aucune décision sur la
géométrie effective du Niphargus.
