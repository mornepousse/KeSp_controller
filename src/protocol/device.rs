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
