# Support multi-modèles (Niphargus) — plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Faire piloter par le même binaire les cartes KaSe/KeSp existantes et le Niphargus, en lisant ce que l'appareil déclare au lieu de coder son modèle en dur.

**Architecture:** L'appareil se décrit. `FEATURES` donne ses capacités, le layout JSON donne sa géométrie et son nom, l'USB ne sert plus qu'à reconnaître et nommer un port. Les constantes de géométrie deviennent des données portées par un `DeviceProfile` construit une fois au branchement.

**Tech Stack:** Rust 2024, Slint pour l'UI, `serialport`, `serde_json`. Tests unitaires `cargo test` (phase rapide du tripwire).

**Spec:** `docs/superpowers/specs/2026-09-07-niphargus-multimodel-design.md`

## Global Constraints

- **TDD obligatoire** (CLAUDE.md) : toute logique pure a son test écrit d'abord, rouge avant implémentation, vert après, parallel-safe.
- **Chaque tâche finit sur `./scripts/check.sh --fast` vert** avant commit.
- `.tripwire-testcount` monte au fil des tâches ; le committer avec la tâche qui l'incrémente.
- **Zéro régression sur KaSe 5×13** : le chemin existant garde ses valeurs et ses tests.
- `DEFAULT_ROWS = 5`, `DEFAULT_COLS = 13` — les valeurs KaSe filaire, employées uniquement quand l'appareil ne déclare rien.
- Seuil de séparation des mains : `1.5 × UNIT_PX` (`UNIT_PX = 50.0`, `layout.rs:15`).
- Aucune modification du firmware.

## Structure de fichiers

| Fichier | Responsabilité |
|---|---|
| `src/protocol/device.rs` **(créé)** | `DeviceProfile`, parsing de `FEATURES`, résolution de géométrie. Le seul endroit qui sait « quel appareil ». |
| `src/protocol/layout.rs` | Ajout : extraction de `rows`/`cols`/`name` du layout JSON (aujourd'hui parsés puis jetés). |
| `src/protocol/parsers.rs` | `parse_keymap` prend les rangées en paramètre ; `ROWS` renommée. |
| `src/protocol/stats.rs` | Séparation des mains par le trou du layout ; libellés de rangées dérivés du compte. |
| `src/protocol/serial/native.rs` | Un seul matcher USB ; `SerialManager` porte le profil. |
| `src/protocol/binary.rs` | `KrResponse::is_unsupported()`. |
| `src/connection.rs` | Câblage : layout d'abord, géométrie, puis keymap ; diffusion du profil. |
| `src/context.rs` | `BgMsg::Profile`. |

---

### Task 1: `DeviceProfile` et le parsing de FEATURES

**Files:**
- Create: `src/protocol/device.rs`
- Modify: `src/protocol/mod.rs` (déclarer le module)
- Test: dans `src/protocol/device.rs` (`#[cfg(test)] mod tests`, convention du projet)

**Interfaces:**
- Consomme : rien.
- Produit : `DeviceProfile { name: String, rows: usize, cols: usize, features: BTreeSet<String> }`, `DeviceProfile::default()`, `DeviceProfile::has(&self, &str) -> bool`, `parse_features_csv(&str) -> BTreeSet<String>`, `DEFAULT_ROWS: usize`, `DEFAULT_COLS: usize`.

- [ ] **Step 1: Écrire les tests rouges**

Créer `src/protocol/device.rs` avec **uniquement** ce bloc de test :

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_csv_splits_trims_and_drops_empties() {
        let f = parse_features_csv("MT, LT ,,COMBO,");
        assert_eq!(f.len(), 3);
        assert!(f.contains("MT"));
        assert!(f.contains("LT"));
        assert!(f.contains("COMBO"));
    }

    #[test]
    fn features_csv_of_empty_string_is_empty() {
        assert!(parse_features_csv("").is_empty());
    }

    #[test]
    fn default_profile_is_the_wired_kase_geometry() {
        let p = DeviceProfile::default();
        assert_eq!(p.rows, 5);
        assert_eq!(p.cols, 13);
        assert!(p.features.is_empty());
    }

    #[test]
    fn has_reports_tag_membership() {
        let p = DeviceProfile {
            features: parse_features_csv("RF_DONGLE,BATTERY"),
            ..DeviceProfile::default()
        };
        assert!(p.has("RF_DONGLE"));
        assert!(!p.has("TAP_DANCE"));
    }
}
```

Ajouter `pub mod device;` dans `src/protocol/mod.rs`.

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib device::`
Expected: échec de compilation — `parse_features_csv` et `DeviceProfile` n'existent pas.

- [ ] **Step 3: Implémenter le minimum**

En tête de `src/protocol/device.rs`, avant le bloc de test :

```rust
//! Ce que l'appareil connecté dit de lui-même, appris au branchement.
//!
//! Le contrôleur ne devine pas le modèle : il lit les capacités annoncées par
//! `FEATURES` et la géométrie annoncée par le layout JSON. L'USB ne sert qu'à
//! reconnaître et nommer un port.

use std::collections::BTreeSet;

/// Rangées supposées quand l'appareil ne déclare rien — toutes les cartes KaSe.
pub const DEFAULT_ROWS: usize = 5;
/// Colonnes supposées quand l'appareil ne déclare rien — KaSe filaire.
pub const DEFAULT_COLS: usize = 13;

/// Profil de l'appareil connecté. Construit une fois au branchement.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceProfile {
    /// Nom déclaré par le layout JSON, repli sur la chaîne produit USB.
    pub name: String,
    pub rows: usize,
    pub cols: usize,
    /// Tags renvoyés par `FEATURES` (0x02).
    pub features: BTreeSet<String>,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            features: BTreeSet::new(),
        }
    }
}

impl DeviceProfile {
    /// L'appareil annonce-t-il cette capacité ?
    pub fn has(&self, tag: &str) -> bool {
        self.features.contains(tag)
    }
}

/// Découper la réponse CSV de `FEATURES` en tags.
/// Les espaces sont rognés et les entrées vides ignorées : le firmware peut
/// terminer sa liste par une virgule.
pub fn parse_features_csv(csv: &str) -> BTreeSet<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}
```

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib device::`
Expected: 4 tests PASS.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/device.rs src/protocol/mod.rs .tripwire-testcount
git commit -m "feat(device): profil d'appareil et parsing des tags FEATURES"
```

---

### Task 2: Résolution de la géométrie

**Files:**
- Modify: `src/protocol/device.rs`
- Test: `src/protocol/device.rs` (bloc `tests` existant)

**Interfaces:**
- Consomme : `DEFAULT_ROWS`, `DEFAULT_COLS` (Task 1).
- Produit : `resolve_geometry(layout: Option<(usize, usize)>, keystats: Option<(usize, usize)>) -> (usize, usize)`.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter dans le `mod tests` de `src/protocol/device.rs` :

```rust
    #[test]
    fn geometry_prefers_the_layout_over_keystats() {
        assert_eq!(resolve_geometry(Some((4, 7)), Some((5, 13))), (4, 7));
    }

    #[test]
    fn geometry_falls_back_to_keystats_when_layout_is_silent() {
        assert_eq!(resolve_geometry(None, Some((5, 14))), (5, 14));
    }

    #[test]
    fn geometry_falls_back_to_default_when_nothing_is_known() {
        assert_eq!(resolve_geometry(None, None), (DEFAULT_ROWS, DEFAULT_COLS));
    }

    #[test]
    fn geometry_rejects_zero_dimensions_and_tries_the_next_source() {
        assert_eq!(resolve_geometry(Some((0, 7)), Some((5, 13))), (5, 13));
        assert_eq!(resolve_geometry(Some((4, 0)), None), (DEFAULT_ROWS, DEFAULT_COLS));
    }
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib device::geometry`
Expected: échec de compilation — `resolve_geometry` n'existe pas.

- [ ] **Step 3: Implémenter le minimum**

Ajouter dans `src/protocol/device.rs`, après `parse_features_csv` :

```rust
/// Résoudre (rangées, colonnes) depuis ce que l'appareil a offert, de la source
/// la plus fiable à la moins fiable : layout JSON, puis en-tête `KEYSTATS_BIN`,
/// puis les valeurs par défaut. Une dimension nulle est un non-dit, pas une
/// réponse : on passe à la source suivante.
pub fn resolve_geometry(
    layout: Option<(usize, usize)>,
    keystats: Option<(usize, usize)>,
) -> (usize, usize) {
    fn valid(g: &(usize, usize)) -> bool {
        g.0 > 0 && g.1 > 0
    }
    layout
        .filter(valid)
        .or_else(|| keystats.filter(valid))
        .unwrap_or((DEFAULT_ROWS, DEFAULT_COLS))
}
```

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib device::`
Expected: 8 tests PASS.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/device.rs .tripwire-testcount
git commit -m "feat(device): resolve_geometry, layout puis keystats puis defaut"
```

---

### Task 3: Extraire géométrie et nom du layout JSON

**Files:**
- Modify: `src/protocol/layout.rs:20-27` (retirer les `#[allow(dead_code)]` sur `name`, `rows`, `cols`)
- Test: `src/protocol/layout.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consomme : rien.
- Produit : `parse_geometry(json: &str) -> Option<(usize, usize)>`, `parse_name(json: &str) -> Option<String>`.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter à la fin de `src/protocol/layout.rs` :

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const NIPHAR: &str = r#"{"name":"Niphargus Left","rows":4,"cols":7,"keys":[]}"#;

    #[test]
    fn geometry_is_read_from_the_layout_json() {
        assert_eq!(parse_geometry(NIPHAR), Some((4, 7)));
    }

    #[test]
    fn geometry_is_none_when_the_layout_omits_it() {
        assert_eq!(parse_geometry(r#"{"keys":[]}"#), None);
    }

    #[test]
    fn geometry_is_none_on_invalid_json() {
        assert_eq!(parse_geometry("not json"), None);
    }

    #[test]
    fn name_is_read_from_the_layout_json() {
        assert_eq!(parse_name(NIPHAR), Some("Niphargus Left".to_string()));
    }

    #[test]
    fn empty_name_counts_as_absent() {
        assert_eq!(parse_name(r#"{"name":"","keys":[]}"#), None);
    }
}
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib layout::`
Expected: échec de compilation — `parse_geometry` et `parse_name` n'existent pas.

- [ ] **Step 3: Implémenter le minimum**

Dans `src/protocol/layout.rs`, retirer les trois `#[allow(dead_code)]` au-dessus de `name`, `rows` et `cols` dans `struct LayoutJson`, puis ajouter après `parse_json` :

```rust
/// Géométrie déclarée par le layout de l'appareil, quand il en déclare une.
/// Une dimension nulle est traitée comme absente.
pub fn parse_geometry(json: &str) -> Option<(usize, usize)> {
    let layout: LayoutJson = serde_json::from_str(json).ok()?;
    match (layout.rows, layout.cols) {
        (Some(r), Some(c)) if r > 0 && c > 0 => Some((r, c)),
        _ => None,
    }
}

/// Nom déclaré par le layout de l'appareil. C'est la seule source qui dit de
/// quelle main est une demi-matrice — la géométrie ne le dit pas.
pub fn parse_name(json: &str) -> Option<String> {
    let layout: LayoutJson = serde_json::from_str(json).ok()?;
    layout.name.filter(|n| !n.is_empty())
}
```

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib layout::`
Expected: 5 tests PASS.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/layout.rs .tripwire-testcount
git commit -m "feat(layout): exposer rows/cols/name du layout JSON au lieu de les jeter"
```

---

### Task 4: `parse_keymap` paramétré par les rangées

**Files:**
- Modify: `src/protocol/parsers.rs:1-39` (const `ROWS` → `DEFAULT_ROWS`, signature de `parse_keymap`)
- Modify: `src/protocol/parsers.rs:1036-1070` (tests existants suivent la nouvelle signature)
- Modify: `src/protocol/stats.rs:5` (import de `ROWS` — remplacé en Task 6, mettre `DEFAULT_ROWS` en transitoire)
- Modify: `src/protocol/serial/native.rs:419,435` (appelants)

**Interfaces:**
- Consomme : rien.
- Produit : `parse_keymap(data: &[u8], rows: usize) -> Result<Vec<Vec<u16>>, String>`.

- [ ] **Step 1: Écrire le test rouge**

Ajouter dans le `mod tests` de `src/protocol/parsers.rs` :

```rust
    #[test]
    fn keymap_parses_a_four_by_seven_niphargus_half() {
        let payload = make_payload(0, 4, 7);
        let km = parse_keymap(&payload, 4).expect("parse");
        assert_eq!(km.len(), 4);
        assert_eq!(km[0].len(), 7, "niphargus half must parse 7 cols");
        assert_eq!(km[3][6], (3 * 7 + 6) as u16);
    }

    #[test]
    fn keymap_rejects_a_payload_that_is_not_a_row_multiple() {
        let err = parse_keymap(&[0u8; 10], 4).unwrap_err();
        assert!(err.contains("4-row multiple"), "message doit citer les rangées: {}", err);
    }

    #[test]
    fn keymap_rejects_zero_rows() {
        assert!(parse_keymap(&[0u8; 8], 0).is_err());
    }
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib parsers::`
Expected: échec de compilation — `parse_keymap` prend un seul argument.

- [ ] **Step 3: Implémenter**

Dans `src/protocol/parsers.rs`, remplacer la constante et la fonction :

```rust
/// Rangées supposées quand l'appareil ne déclare rien. Les cartes KaSe sont en
/// 5 rangées ; le Niphargus est en 4. La vraie valeur vient du profil de
/// l'appareil (`protocol::device::DeviceProfile`), jamais d'ici.
pub const DEFAULT_ROWS: usize = 5;

/// Parse a KEYMAP_GET / KEYMAP_CURRENT keycode block into `[row][col]`.
///
/// `data` is the response payload. For the v2 binary protocol it is
/// `[layer:u8][keycodes: rows*cols × u16 LE]` (row-major); legacy callers strip
/// their own header first. The keycode block is always even-length, so an odd
/// total length means the leading layer byte is present. `rows` comes from the
/// device profile and `cols` is derived from the payload length, so a 5×13
/// wired board, a 5×14 dongle and a 4×7 Niphargus half all parse correctly.
pub fn parse_keymap(data: &[u8], rows: usize) -> Result<Vec<Vec<u16>>, String> {
    if rows == 0 {
        return Err("Row count must be non-zero".into());
    }
    let has_layer_byte = data.len() % 2 == 1;
    let offset = usize::from(has_layer_byte);
    let kc = &data[offset..];

    if kc.len() < rows * 2 || !kc.len().is_multiple_of(rows * 2) {
        return Err(format!(
            "Keymap data not a {}-row multiple: {} bytes",
            rows,
            kc.len()
        ));
    }
    let cols = kc.len() / 2 / rows;

    let mut keymap = Vec::with_capacity(rows);
    for r in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for c in 0..cols {
            let idx = (r * cols + c) * 2;
            row.push(u16::from_le_bytes([kc[idx], kc[idx + 1]]));
        }
        keymap.push(row);
    }
    Ok(keymap)
}
```

Mettre à jour les appelants et les tests existants :

- `src/protocol/parsers.rs` — dans les tests existants, remplacer `ROWS` par `DEFAULT_ROWS` et `parse_keymap(&payload)` par `parse_keymap(&payload, DEFAULT_ROWS)` (3 occurrences : lignes ~1040, ~1056, ~1065).
- `src/protocol/stats.rs:5` — `use super::parsers::ROWS;` devient `use super::parsers::DEFAULT_ROWS as ROWS;` (transitoire, supprimé en Task 6).
- `src/protocol/serial/native.rs:419` — `return parsers::parse_keymap(&resp.payload, self.profile.rows);`
- `src/protocol/serial/native.rs:435` — `parsers::parse_keymap(data_without_header, self.profile.rows)`

> `self.profile` n'existe qu'en Task 9. **Pour cette tâche uniquement**, utiliser `parsers::DEFAULT_ROWS` aux deux appels de `native.rs` ; la Task 9 les branche sur le profil.

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib`
Expected: tous les tests PASS, dont les 3 nouveaux.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/parsers.rs src/protocol/stats.rs src/protocol/serial/native.rs .tripwire-testcount
git commit -m "feat(parsers): parse_keymap parametre par les rangees (4x7 Niphargus)"
```

---

### Task 5: Séparation des mains par le trou du layout

**Files:**
- Modify: `src/protocol/stats.rs` (ajout de `HandSplit`, `detect_hand_split`, `hand_for_x`)
- Test: `src/protocol/stats.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consomme : `Hand` (`stats.rs:18-22`), `UNIT_PX` (`layout.rs:15`).
- Produit : `enum HandSplit { TwoHands { boundary_x: f32 }, OneHand(Hand) }`, `detect_hand_split(xs: &[f32], name: &str) -> HandSplit`, `hand_for_x(x: f32, split: HandSplit) -> Hand`.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter dans `src/protocol/stats.rs` :

```rust
#[cfg(test)]
mod hand_split_tests {
    use super::*;

    /// Une main : 7 touches espacées d'1 u, aucun trou.
    fn one_cluster() -> Vec<f32> {
        (0..7).map(|i| i as f32 * UNIT_PX).collect()
    }

    /// Deux mains : deux grappes de 7, séparées par 3 u.
    fn two_clusters() -> Vec<f32> {
        let mut v = one_cluster();
        v.extend((0..7).map(|i| (i as f32 + 9.0) * UNIT_PX));
        v
    }

    #[test]
    fn a_gap_wider_than_the_threshold_means_two_hands() {
        match detect_hand_split(&two_clusters(), "Niphargus") {
            HandSplit::TwoHands { boundary_x } => {
                assert!(boundary_x > 6.0 * UNIT_PX, "frontiere dans le trou: {}", boundary_x);
                assert!(boundary_x < 9.0 * UNIT_PX, "frontiere dans le trou: {}", boundary_x);
            }
            other => panic!("attendu TwoHands, obtenu {:?}", other),
        }
    }

    #[test]
    fn a_single_cluster_is_one_hand_named_by_the_device() {
        assert_eq!(detect_hand_split(&one_cluster(), "Niphargus Left"), HandSplit::OneHand(Hand::Left));
        assert_eq!(detect_hand_split(&one_cluster(), "Niphargus Right"), HandSplit::OneHand(Hand::Right));
    }

    #[test]
    fn a_nameless_single_cluster_defaults_to_the_left_hand() {
        assert_eq!(detect_hand_split(&one_cluster(), ""), HandSplit::OneHand(Hand::Left));
    }

    #[test]
    fn an_empty_layout_is_one_hand_and_does_not_panic() {
        assert_eq!(detect_hand_split(&[], ""), HandSplit::OneHand(Hand::Left));
    }

    #[test]
    fn hand_for_x_reads_the_boundary_and_the_lone_hand() {
        let two = HandSplit::TwoHands { boundary_x: 100.0 };
        assert_eq!(hand_for_x(50.0, two), Hand::Left);
        assert_eq!(hand_for_x(150.0, two), Hand::Right);
        assert_eq!(hand_for_x(50.0, HandSplit::OneHand(Hand::Right)), Hand::Right);
    }
}
```

`Hand` ne dérive **pas** `Debug` aujourd'hui (`stats.rs:8` : `#[derive(Clone, Copy, PartialEq)]`). Le rendre :

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hand {
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib stats::hand_split`
Expected: échec de compilation — `HandSplit`, `detect_hand_split`, `hand_for_x` n'existent pas.

- [ ] **Step 3: Implémenter**

Ajouter dans `src/protocol/stats.rs`, après `col_to_hand_finger` :

```rust
use super::layout::UNIT_PX;

/// Écart horizontal minimal séparant deux mains — 1,5 u. En deçà, c'est
/// l'espacement normal entre deux colonnes ; au-delà, c'est le trou du split.
const HAND_GAP_PX: f32 = 1.5 * UNIT_PX;

/// Comment un layout se partage entre les mains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandSplit {
    /// Deux grappes séparées par un trou : les touches à gauche de `boundary_x`
    /// sont de la main gauche.
    TwoHands { boundary_x: f32 },
    /// Une seule grappe — toutes les touches sont de cette main.
    OneHand(Hand),
}

/// Déduire la séparation des mains des positions horizontales des touches.
///
/// Ce qui distingue deux mains d'une demi-matrice, c'est le trou entre les
/// grappes, pas le nombre de colonnes : le milieu d'une demi-matrice tombe au
/// milieu d'une main et la couperait en deux. Quand il n'y a qu'une grappe,
/// *laquelle* n'est pas déductible de la géométrie — seul le nom rapporté par
/// l'appareil le dit.
pub fn detect_hand_split(xs: &[f32], name: &str) -> HandSplit {
    let mut sorted: Vec<f32> = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut widest = 0.0f32;
    let mut boundary = 0.0f32;
    for pair in sorted.windows(2) {
        let gap = pair[1] - pair[0];
        if gap > widest {
            widest = gap;
            boundary = (pair[0] + pair[1]) / 2.0;
        }
    }

    if widest >= HAND_GAP_PX {
        HandSplit::TwoHands { boundary_x: boundary }
    } else if name.to_ascii_lowercase().contains("right") {
        HandSplit::OneHand(Hand::Right)
    } else {
        HandSplit::OneHand(Hand::Left)
    }
}

/// Main d'une touche, selon la séparation détectée.
pub fn hand_for_x(x: f32, split: HandSplit) -> Hand {
    match split {
        HandSplit::TwoHands { boundary_x } => {
            if x < boundary_x { Hand::Left } else { Hand::Right }
        }
        HandSplit::OneHand(h) => h,
    }
}
```

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib stats::`
Expected: 5 nouveaux tests PASS, les tests existants de `stats` inchangés.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/stats.rs .tripwire-testcount
git commit -m "feat(stats): separation des mains par le trou du layout, pas par le compte de colonnes"
```

---

### Task 6: Libellés de rangées dérivés du compte

**Files:**
- Modify: `src/protocol/stats.rs:24` (`ROW_NAMES`), `stats.rs:152-172` (`row_usage`), `stats.rs:5` (retirer l'import transitoire de Task 4)
- Test: `src/protocol/stats.rs`

**Interfaces:**
- Consomme : rien.
- Produit : `row_labels(rows: usize) -> Vec<String>`.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter dans `src/protocol/stats.rs` :

```rust
#[cfg(test)]
mod row_label_tests {
    use super::*;

    #[test]
    fn five_rows_keep_the_historical_kase_names() {
        assert_eq!(row_labels(5), vec!["Number", "Upper", "Home", "Lower", "Thumb"]);
    }

    #[test]
    fn four_rows_drop_the_number_row_the_board_does_not_have() {
        assert_eq!(row_labels(4), vec!["Upper", "Home", "Lower", "Thumb"]);
    }

    #[test]
    fn an_unknown_row_count_gets_numbered_labels() {
        assert_eq!(row_labels(2), vec!["Row 0", "Row 1"]);
    }

    #[test]
    fn row_usage_follows_the_heatmap_height_not_a_constant() {
        let heatmap = vec![vec![1u32; 7], vec![2u32; 7], vec![3u32; 7], vec![4u32; 7]];
        let usage = row_usage(&heatmap);
        assert_eq!(usage.len(), 4);
        assert_eq!(usage[0].name, "Upper");
        assert_eq!(usage[3].count, 4 * 7);
    }
}
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib stats::row_label`
Expected: échec de compilation — `row_labels` n'existe pas ; `row_usage` renvoie 5 rangées.

- [ ] **Step 3: Implémenter**

Dans `src/protocol/stats.rs`, remplacer la constante `ROW_NAMES` par :

```rust
/// Libellés de rangées pour un clavier de `rows` rangées.
/// Cinq rangées gardent les noms KaSe historiques ; quatre rangées perdent
/// « Number », qu'une carte 4 rangées n'a pas. Au-delà, on numérote plutôt que
/// d'inventer une ergonomie.
pub fn row_labels(rows: usize) -> Vec<String> {
    const FIVE: [&str; 5] = ["Number", "Upper", "Home", "Lower", "Thumb"];
    match rows {
        5 => FIVE.iter().map(|s| s.to_string()).collect(),
        4 => FIVE[1..].iter().map(|s| s.to_string()).collect(),
        n => (0..n).map(|r| format!("Row {}", r)).collect(),
    }
}
```

Remplacer `row_usage` (`stats.rs:152-172`) par :

```rust
/// Compute row usage. The row count comes from the heatmap itself, which the
/// firmware sizes from its own matrix (`KEYSTATS_BIN` header).
pub fn row_usage(heatmap: &[Vec<u32>]) -> Vec<RowUsage> {
    let labels = row_labels(heatmap.len());
    let row_counts: Vec<u32> = heatmap.iter().map(|row| row.iter().sum()).collect();
    let total: u32 = row_counts.iter().sum();

    let mut result = Vec::with_capacity(row_counts.len());
    for (r, &count) in row_counts.iter().enumerate() {
        let pct = if total > 0 { count as f32 / total as f32 * 100.0 } else { 0.0 };
        result.push(RowUsage { name: labels[r].clone(), row: r, count, pct });
    }

    result
}
```

Supprimer l'import transitoire `use super::parsers::DEFAULT_ROWS as ROWS;` (`stats.rs:5`) s'il n'a plus d'usage — `cargo clippy -D warnings` le signalera sinon.

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib stats::`
Expected: 4 nouveaux tests PASS.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/stats.rs .tripwire-testcount
git commit -m "feat(stats): libelles et comptes de rangees derives de la heatmap"
```

---

### Task 7: Un seul matcher USB, label = nom produit

**Files:**
- Modify: `src/protocol/serial/native.rs:120-140` (`classify_kase_port`), `native.rs:165-214` (`find_kase_port`)
- Test: `src/protocol/serial/native.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consomme : rien.
- Produit : `is_family_product(product: &str, vid: u16) -> bool`, `port_label(product: &str, port_name: &str) -> String`. Les deux sont des fonctions libres du module (testables sans matériel) ; `classify_kase_port` les appelle.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter à la fin de `src/protocol/serial/native.rs` :

```rust
#[cfg(test)]
mod usb_match_tests {
    use super::*;

    const KEYBOARD_VID: u16 = 0xCAFE;
    const ESPRESSIF_VID: u16 = 0x303A;

    #[test]
    fn the_niphargus_halves_are_recognised_by_product_name() {
        assert!(is_family_product("Niphargus Left", KEYBOARD_VID));
        assert!(is_family_product("Niphargus Right", KEYBOARD_VID));
    }

    #[test]
    fn the_kase_boards_and_the_dongle_stay_recognised() {
        assert!(is_family_product("KaSe V2", KEYBOARD_VID));
        assert!(is_family_product("KaSe V2 Debug", KEYBOARD_VID));
        assert!(is_family_product("KaSe Dongle", ESPRESSIF_VID));
    }

    #[test]
    fn an_unnamed_board_on_our_vid_is_still_ours() {
        assert!(is_family_product("", KEYBOARD_VID));
    }

    #[test]
    fn the_conchodytes_mouse_is_not_a_keyboard() {
        assert!(!is_family_product("Conchodytes", ESPRESSIF_VID));
    }

    #[test]
    fn a_random_serial_adapter_is_not_ours() {
        assert!(!is_family_product("USB2.0-Serial", 0x1A86));
    }

    #[test]
    fn the_label_uses_the_product_string_the_device_reports() {
        assert_eq!(port_label("Niphargus Left", "/dev/ttyACM0"), "Niphargus Left (/dev/ttyACM0)");
    }

    #[test]
    fn a_nameless_device_still_gets_a_usable_label() {
        assert_eq!(port_label("", "/dev/ttyACM1"), "Keyboard (/dev/ttyACM1)");
    }
}
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib serial::native::usb_match`
Expected: échec de compilation — `is_family_product` et `port_label` n'existent pas.

- [ ] **Step 3: Implémenter**

Dans `src/protocol/serial/native.rs`, ajouter au niveau du module (hors du `impl SerialManager`) :

```rust
/// Fragments de chaîne produit qui marquent un appareil comme étant des nôtres.
const FAMILY_PRODUCTS: [&str; 4] = ["KaSe", "KeSp", "Niphargus", "Dongle"];

/// Cet appareil USB est-il des nôtres ?
///
/// On matche sur notre VID propre ou sur un fragment de nom produit — jamais sur
/// le VID Espressif seul : il est partagé avec la souris Conchodytes, qui n'est
/// pas un clavier.
pub fn is_family_product(product: &str, vid: u16) -> bool {
    const KEYBOARD_VID: u16 = 0xCAFE;
    vid == KEYBOARD_VID || FAMILY_PRODUCTS.iter().any(|p| product.contains(p))
}

/// Libellé d'un port dans la liste déroulante : le nom que l'appareil rapporte,
/// pas un nom de famille codé en dur.
pub fn port_label(product: &str, port_name: &str) -> String {
    if product.is_empty() {
        format!("Keyboard ({})", port_name)
    } else {
        format!("{} ({})", product, port_name)
    }
}
```

Remplacer `classify_kase_port` (`native.rs:126-140`) par :

```rust
    /// Classify a serial port as one of our devices. Returns (label, is_dongle).
    ///
    /// C'est le SEUL matcher USB : `list_kase_ports` et `find_kase_port` passent
    /// tous deux par ici, donc ils ne peuvent plus être en désaccord sur ce qui
    /// est à nous — leur désaccord rendait le Niphargus listable mais
    /// introuvable à l'auto-connect.
    fn classify_kase_port(port: &serialport::SerialPortInfo) -> Option<(String, bool)> {
        let serialport::SerialPortType::UsbPort(usb) = &port.port_type else {
            return None;
        };
        let product = usb.product.clone().unwrap_or_default();
        if !is_family_product(&product, usb.vid) {
            return None;
        }
        let is_dongle = product.contains("Dongle");
        Some((port_label(&product, &port.port_name), is_dongle))
    }
```

Remplacer `find_kase_port` (`native.rs:165-214`) par :

```rust
    pub fn find_kase_port() -> Result<String, String> {
        let available = serialport::available_ports();
        let ports = available.unwrap_or_default();
        if ports.is_empty() {
            return Err("No serial ports found".into());
        }

        // Premier passage : le même prédicat que la liste déroulante.
        for port in &ports {
            if Self::classify_kase_port(port).is_some() {
                return Ok(port.port_name.clone());
            }
        }

        // Second passage (Linux) : certains ports n'exposent pas leur descripteur
        // à serialport ; udevadm le voit quand même.
        #[cfg(target_os = "linux")]
        for port in &ports {
            let udevadm_result = std::process::Command::new("udevadm")
                .args(["info", "-n", &port.port_name])
                .output();

            if let Ok(output) = udevadm_result {
                let text = String::from_utf8_lossy(&output.stdout);
                if FAMILY_PRODUCTS.iter().any(|p| text.contains(p)) {
                    return Ok(port.port_name.clone());
                }
            }
        }

        let scanned_count = ports.len();
        Err(format!("No keyboard found ({} port(s) scanned)", scanned_count))
    }
```

Supprimer les constantes `KEYBOARD_VID` / `ESPRESSIF_VID` de l'`impl` si elles n'ont plus d'usage (`cargo clippy -D warnings` le signalera).

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib serial::native::`
Expected: 7 tests PASS.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/serial/native.rs .tripwire-testcount
git commit -m "fix(usb): un seul matcher pour la liste et l auto-connect, label = nom produit"
```

---

### Task 8: `ERR_UNKNOWN` distingué d'un échec

**Files:**
- Modify: `src/protocol/binary.rs:246-250` (`impl KrResponse`)
- Test: `src/protocol/binary.rs`

**Interfaces:**
- Consomme : `status::ERR_UNKNOWN` (`binary.rs:100`).
- Produit : `KrResponse::is_unsupported(&self) -> bool`.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter dans le `mod tests` de `src/protocol/binary.rs` (le créer s'il n'existe pas) :

```rust
    #[test]
    fn err_unknown_is_a_capability_statement_not_a_failure() {
        let r = KrResponse { cmd: 0x13, status: status::ERR_UNKNOWN, payload: vec![] };
        assert!(r.is_unsupported());
        assert!(!r.is_ok());
    }

    #[test]
    fn a_real_error_is_not_reported_as_unsupported() {
        let r = KrResponse { cmd: 0x13, status: status::ERR_RANGE, payload: vec![] };
        assert!(!r.is_unsupported());
        assert!(!r.is_ok());
    }

    #[test]
    fn a_successful_response_is_neither() {
        let r = KrResponse { cmd: 0x13, status: status::OK, payload: vec![] };
        assert!(r.is_ok());
        assert!(!r.is_unsupported());
    }
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib binary::`
Expected: échec de compilation — `is_unsupported` n'existe pas.

- [ ] **Step 3: Implémenter**

Dans `src/protocol/binary.rs`, ajouter à l'`impl KrResponse` :

```rust
    /// L'appareil n'implémente pas cette commande. Ce n'est pas un échec, c'est
    /// une déclaration de capacité : le dongle du Niphargus répond ça à toutes
    /// les commandes keymap, parce qu'il n'a pas de moteur keymap. L'appelant
    /// doit griser la fonction, pas afficher une erreur.
    pub fn is_unsupported(&self) -> bool {
        self.status == status::ERR_UNKNOWN
    }
```

Retirer `#[allow(dead_code)]` sur `pub mod status` si clippy signale qu'il n'est plus nécessaire.

- [ ] **Step 4: Vérifier que ça passe**

Run: `cargo test --lib binary::`
Expected: 3 tests PASS.

- [ ] **Step 5: Check et commit**

```bash
./scripts/check.sh --fast
git add src/protocol/binary.rs .tripwire-testcount
git commit -m "feat(protocol): is_unsupported, ERR_UNKNOWN n est pas un echec"
```

---

### Task 9: Câblage — profil porté, layout avant keymap

**Files:**
- Modify: `src/protocol/serial/native.rs:17-35` (champ `profile` sur `SerialManager`), `native.rs:88-97` (stocker les features), `native.rs:419,435` (rangées depuis le profil)
- Modify: `src/connection.rs:14-42` (`connect_and_load` : ordre layout → géométrie → keymap)
- Modify: `src/context.rs:32-72` (`BgMsg::Profile`)

**Interfaces:**
- Consomme : `DeviceProfile`, `parse_features_csv`, `resolve_geometry` (Tasks 1-2) ; `layout::parse_geometry`, `layout::parse_name` (Task 3) ; `parse_keymap(data, rows)` (Task 4).
- Produit : `SerialManager::profile: DeviceProfile` (public) ; `BgMsg::Profile(DeviceProfile)`.

- [ ] **Step 1: Écrire le test rouge**

Le câblage traverse du matériel ; ce qui est testable sans matériel, c'est que le profil se construise correctement depuis les trois réponses de l'appareil. Ajouter dans `src/protocol/device.rs` :

```rust
    /// Construire un profil comme le fait `connect_and_load`, sans matériel.
    #[test]
    fn a_niphargus_profile_is_built_from_its_own_answers() {
        let features = parse_features_csv("MT,LT,COMBO,TAP_DANCE");
        let (rows, cols) = resolve_geometry(Some((4, 7)), None);
        let p = DeviceProfile {
            name: "Niphargus Left".to_string(),
            rows,
            cols,
            features,
        };
        assert_eq!(p.rows, 4);
        assert_eq!(p.cols, 7);
        assert!(p.has("TAP_DANCE"));
        assert!(!p.has("RF_DONGLE"), "une moitie n est pas un dongle");
    }

    #[test]
    fn a_dongle_profile_keeps_its_rf_tags_and_the_default_geometry() {
        let features = parse_features_csv("MT,LT,RF_DONGLE,RF_STATUS,BATTERY");
        let (rows, cols) = resolve_geometry(None, None);
        let p = DeviceProfile { name: "KaSe Dongle".to_string(), rows, cols, features };
        assert!(p.has("RF_DONGLE"));
        assert!(p.has("BATTERY"));
        assert_eq!((p.rows, p.cols), (DEFAULT_ROWS, DEFAULT_COLS));
    }
```

- [ ] **Step 2: Vérifier que ça échoue puis passe**

Run: `cargo test --lib device::`
Expected: PASS immédiatement — ces deux tests n'utilisent que des briques des Tasks 1-2. Ils verrouillent le contrat que le câblage doit respecter ; s'ils échouent, c'est une régression des Tasks 1-2, à corriger avant de câbler.

- [ ] **Step 3: Porter le profil sur `SerialManager`**

`src/protocol/serial/native.rs` — ajouter le champ à la struct (`native.rs:17-24`) :

```rust
    /// Ce que l'appareil a déclaré de lui-même au branchement.
    pub profile: crate::protocol::device::DeviceProfile,
```

et dans `new()` (`native.rs:27-35`) :

```rust
            profile: crate::protocol::device::DeviceProfile::default(),
```

Dans `connect()` (`native.rs:93-96`), remplacer le bloc FEATURES par :

```rust
            if let Ok(resp) = self.send_binary(bp::cmd::FEATURES, &[]) {
                let csv = String::from_utf8_lossy(&resp.payload);
                self.profile.features = crate::protocol::device::parse_features_csv(&csv);
                self.is_dongle = self.profile.has("RF_DONGLE");
            }
```

Dans `disconnect()`, remettre le profil à zéro :

```rust
        self.profile = crate::protocol::device::DeviceProfile::default();
```

Aux deux appels de keymap, remplacer `parsers::DEFAULT_ROWS` (posé en Task 4) par `self.profile.rows` :
- `native.rs:419` — `return parsers::parse_keymap(&resp.payload, self.profile.rows);`
- `native.rs:435` — `parsers::parse_keymap(data_without_header, self.profile.rows)`

- [ ] **Step 4: Réordonner `connect_and_load`**

`src/connection.rs` — remplacer le corps du bras `Ok(port_name)` (`connection.rs:19-40`) par :

```rust
        Ok(port_name) => {
            let fw = ser.get_firmware_version().unwrap_or_default();
            let _ = tx.send(BgMsg::RfModeChanged(ser.is_dongle));
            let _ = tx.send(BgMsg::ConnectionMode(ser.connection_kind()));

            // Le layout AVANT la keymap : c'est lui qui porte la géométrie, et
            // `get_keymap` en a besoin pour découper le payload en rangées.
            match ser.get_layout_json() {
                Ok(json) => {
                    let (rows, cols) = protocol::device::resolve_geometry(
                        protocol::layout::parse_geometry(&json),
                        None,
                    );
                    ser.profile.rows = rows;
                    ser.profile.cols = cols;
                    if let Some(name) = protocol::layout::parse_name(&json) {
                        ser.profile.name = name;
                    }
                    match protocol::layout::parse_json(&json) {
                        Ok(keys) => {
                            let _ = tx.send(BgMsg::LayoutJson(keys));
                        }
                        Err(e) => eprintln!("Layout parse error: {}", e),
                    }
                }
                // Pas de layout : le profil garde ses valeurs par défaut, et la
                // keymap se parsera avec elles. Un dongle répond ERR_UNKNOWN ici.
                Err(e) => eprintln!("get_layout_json error: {}", e),
            }

            let _ = tx.send(BgMsg::Profile(ser.profile.clone()));

            let names = ser.get_layer_names().unwrap_or_default();
            let km = ser.get_keymap(0).unwrap_or_default();
            let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
        }
```

Vérifier que `protocol::device` est accessible depuis `connection.rs` (l'import `use crate::protocol;` existe déjà en `connection.rs:2`).

- [ ] **Step 5: Ajouter `BgMsg::Profile`**

`src/context.rs` — ajouter dans l'`enum BgMsg`, à côté de `RfModeChanged` :

```rust
    /// Ce que l'appareil a déclaré de lui-même : géométrie, nom, capacités.
    Profile(crate::protocol::device::DeviceProfile),
```

`BgMsg` n'a **qu'un** site de match exhaustif : `src/dispatch.rs:144`. Y ajouter le bras, à côté de `BgMsg::RfModeChanged` (`dispatch.rs:505`). Pour ce premier câblage, journaliser sans toucher à l'UI :

```rust
        BgMsg::Profile(p) => {
            eprintln!("device: {} — {}x{}, {} capacites", p.name, p.rows, p.cols, p.features.len());
        }
```

- [ ] **Step 6: Vérifier**

Run: `./scripts/check.sh` (complet : clippy `-D warnings` + build release)
Expected: vert. Clippy signalera tout import ou constante devenu inutile aux Tasks 4, 6 et 7 — les retirer.

- [ ] **Step 7: Commit**

```bash
git add src/protocol/serial/native.rs src/connection.rs src/context.rs src/protocol/device.rs .tripwire-testcount
git commit -m "feat(connect): profil d appareil porte par SerialManager, layout avant keymap"
```

---

### Task 10: Les appelants distinguent « pas supporté » de « échoué »

La Task 8 ajoute `is_unsupported()` mais aucun appelant ne s'en sert : sans cette
tâche, brancher un dongle journalise toujours `get_layout_json error`, et la
vérification manuelle n°4 ne peut pas passer.

**Files:**
- Modify: `src/protocol/binary.rs` (ajout de `CmdError`)
- Modify: `src/protocol/serial/native.rs` (`get_layout_json`, `get_keymap`)
- Modify: `src/connection.rs:26,29,36`, `src/layout.rs:62`, `src/keymap.rs:60`, `src/config.rs:35,210`
- Test: `src/protocol/binary.rs`

**Interfaces:**
- Consomme : `KrResponse::is_unsupported()` (Task 8).
- Produit : `enum CmdError { Unsupported, Failed(String) }` avec `impl std::fmt::Display`. `get_layout_json() -> Result<String, CmdError>`, `get_keymap(layer: u8) -> Result<Vec<Vec<u16>>, CmdError>`.

- [ ] **Step 1: Écrire les tests rouges**

Ajouter dans le `mod tests` de `src/protocol/binary.rs` :

```rust
    #[test]
    fn an_unsupported_command_reads_as_a_capability_not_an_error() {
        let e = CmdError::Unsupported;
        assert_eq!(e.to_string(), "commande non supportee par cet appareil");
        assert!(e.is_unsupported());
    }

    #[test]
    fn a_failed_command_carries_its_reason() {
        let e = CmdError::Failed("timeout".to_string());
        assert_eq!(e.to_string(), "timeout");
        assert!(!e.is_unsupported());
    }
```

- [ ] **Step 2: Vérifier que ça échoue**

Run: `cargo test --lib binary::`
Expected: échec de compilation — `CmdError` n'existe pas.

- [ ] **Step 3: Implémenter `CmdError`**

Ajouter dans `src/protocol/binary.rs` :

```rust
/// Échec d'une commande v2. Un appareil qui n'implémente pas une commande n'est
/// pas en panne : le dongle du Niphargus répond `ERR_UNKNOWN` à toute commande
/// keymap parce qu'il n'a pas de moteur keymap. L'appelant grise la fonction au
/// lieu d'afficher une erreur.
#[derive(Debug, Clone, PartialEq)]
pub enum CmdError {
    /// L'appareil n'implémente pas cette commande (`ERR_UNKNOWN`).
    Unsupported,
    /// Tout le reste : transport, CRC, statut d'erreur réel.
    Failed(String),
}

impl CmdError {
    pub fn is_unsupported(&self) -> bool {
        matches!(self, CmdError::Unsupported)
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmdError::Unsupported => write!(f, "commande non supportee par cet appareil"),
            CmdError::Failed(m) => write!(f, "{}", m),
        }
    }
}

impl From<String> for CmdError {
    fn from(m: String) -> Self {
        CmdError::Failed(m)
    }
}
```

- [ ] **Step 4: Faire remonter le statut dans les deux commandes concernées**

`src/protocol/serial/native.rs` — dans `get_keymap`, remplacer le bras v2 :

```rust
        if self.v2 {
            let resp = self.send_binary(bp::cmd::KEYMAP_GET, &[layer])
                .map_err(bp::CmdError::Failed)?;
            if resp.is_unsupported() {
                return Err(bp::CmdError::Unsupported);
            }
            return parsers::parse_keymap(&resp.payload, self.profile.rows)
                .map_err(bp::CmdError::Failed);
        }
```

et changer sa signature en `-> Result<Vec<Vec<u16>>, bp::CmdError>`. Les erreurs `String` restantes du chemin legacy passent par `.into()`.

Appliquer le même traitement à `get_layout_json` (`GET_LAYOUT_JSON`), signature `-> Result<String, bp::CmdError>`.

- [ ] **Step 5: Mettre à jour les 7 appelants**

Trois n'ont rien à changer — ils écrasent l'erreur : `src/config.rs:35`, `src/config.rs:210`, `src/connection.rs:26` (`.unwrap_or_default()`).

`src/connection.rs:36` — ne plus crier sur un appareil qui ne fait pas ça :

```rust
                Err(e) if e.is_unsupported() => {
                    // Un dongle n'a pas de layout : ce n'est pas une erreur.
                }
                Err(e) => eprintln!("get_layout_json error: {}", e),
```

`src/layout.rs:62` et `src/keymap.rs:60` — dans leurs bras `Err(e)`, remplacer le message affiché par `e.to_string()` ; `Display` produit déjà la bonne phrase dans les deux cas.

- [ ] **Step 6: Vérifier**

Run: `./scripts/check.sh`
Expected: vert, clippy compris.

- [ ] **Step 7: Commit**

```bash
git add src/protocol/binary.rs src/protocol/serial/native.rs src/connection.rs src/layout.rs src/keymap.rs .tripwire-testcount
git commit -m "feat(protocol): CmdError, un appareil qui ne sait pas faire n est pas en panne"
```

---

## Hors périmètre de ce plan

**Le gating d'onglets par capacité.** La spec (§5) veut qu'un onglet Tap Dance se
grise sur l'absence du tag `TAP_DANCE` plutôt que sur « c'est un dongle ». Ce
plan livre la **donnée** nécessaire — `DeviceProfile.features` est peuplé et
diffusé par `BgMsg::Profile` — mais pas le câblage UI : le gating actuel passe
par un unique booléen Slint `AppState.is-dongle` (`ui/globals.slint:32`,
`dispatch.rs:505-508`), et l'exposer par capacité demande une propriété Slint par
tag, plus la reprise de chaque onglet. C'est un travail d'UI distinct, à faire
dans son propre plan une fois celui-ci en place.

---

## Vérification manuelle, au premier branchement

Le tripwire ne peut pas prouver ceci — il n'y a pas de matériel. À faire quand une carte Niphargus existe :

1. Brancher une moitié Niphargus. Elle doit **apparaître dans la liste** étiquetée `Niphargus Left (/dev/ttyACMx)` et **l'auto-connect doit la trouver** — les deux, c'est le bug 1+2 fermé.
2. Vérifier la ligne `device:` en console : elle dit la géométrie réellement rapportée. **C'est la réponse à la question ouverte de la spec** (`4×7` ou `4×14`).
3. L'onglet keymap doit afficher une matrice, pas une erreur de parsing.
4. Brancher le dongle : les onglets keymap/stats doivent se griser sur `ERR_UNKNOWN`, sans message d'erreur.
5. Brancher une KaSe V2 : rien ne doit avoir changé — 5 rangées, noms de rangées historiques, split des mains identique.
