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
}
