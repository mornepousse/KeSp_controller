# Key Position Editor — Layout Preview (Tools tab)

**Date:** 2026-06-05
**Status:** Approved

## Objectif

Ajouter un éditeur de position de touches par champs numériques à la carte
**Layout Preview** de l'onglet Tools. L'utilisateur clique une touche dans le
rendu pour la sélectionner, puis édite ses champs (X, Y, rotation, row, col).
Le résultat s'exporte en JSON via le bouton **Export...** existant. Pas
d'écriture vers le firmware (aucune commande protocole pour ça).

## Décisions (issues du brainstorming)

| Sujet | Choix |
|-------|-------|
| Interaction | Champs numériques uniquement (pas de drag) |
| Sélection | Clic sur le keycap dans l'aperçu |
| Éditable | Position (x/y), Rotation, Matrice (row/col) — **pas** w/h |
| Add/Delete | Oui : bouton Add Key (1u à l'origine) + Delete Key |
| Sortie | Export fichier JSON (pas d'écriture firmware) |
| Unités des champs | Unités clavier (ex. `1.5`), pas pixels |

## Architecture

### Source de vérité & isolation
- Buffer éditable dédié `Vec<KeycapPos>`, **séparé de `ctx.keys`** (géométrie
  du clavier réel utilisée par keymap/heatmap). Éditer ici ne touche pas ces
  onglets.
- Chargement (fichier/clavier) → remplit le buffer. Toute édition mute le
  buffer. Le modèle de rendu `LayoutBridge.keycaps` et le `json-text` sont
  régénérés depuis le buffer après chaque changement.

### Approche : modèle possédé côté Rust (recommandée)
Slint émet des callbacks d'intention ; Rust possède le buffer, fait la
conversion unité↔pixel, recalcule les bornes, re-sérialise le JSON, reconstruit
le modèle en préservant la sélection. Source de vérité unique, serde reste en
Rust.

## Composants & flux

### `ui/globals.slint` — `LayoutBridge`
Ajouts :
```
in property <int> selected-index: -1;   // -1 = aucune
in property <float> sel-x;               // unités
in property <float> sel-y;
in property <float> sel-rotation;        // degrés
in property <int> sel-row;
in property <int> sel-col;
callback select-key(int);
callback update-key(string /*field*/, string /*value*/);
callback add-key();
callback delete-key();
```

### `ui/tabs/tab_tools.slint`
- Chaque keycap de l'aperçu enveloppé dans un `TouchArea` → `select-key(index)`.
- Touche sélectionnée : bordure accent via `keycap.selected`.
- Nouveau panneau d'édition sous l'aperçu : 5 `DarkLineEdit` liés à `sel-*`
  (commit sur `edited`/`accepted` → `update-key(field, value)`) + boutons
  **Add Key** / **Delete Key**. Champs activés si une touche est sélectionnée ;
  Add toujours activé.

### `src/protocol/layout.rs`
- `serialize_json(&[KeycapPos]) -> String` : produit un `{ "keys": [...] }` plat
  avec `{row, col, x, y, w, h, r}`, x/y/w/h en unités (px / 50), r en degrés.

### `src/layout.rs`
- Câble `select-key`, `update-key`, `add-key`, `delete-key`.
- Possède le buffer (`Rc<RefCell<Vec<KeycapPos>>>`) + index sélectionné
  (`Rc<Cell<isize>>`, -1 = aucune).
- Chaque édition : parse la chaîne → met à jour le buffer (unité→px) →
  recalcule bornes → reconstruit modèle + `sel-*` + `json-text`.

### `src/models.rs`
- `populate_layout_preview` remplit le buffer.
- Helper `rebuild_layout_model(window, buffer, selected_index)` reconstruit le
  modèle keycaps (marque `selected`), met à jour content-width/height,
  `sel-*`, et `json-text`.

## Cas limites & comportement connu
- **`groups` aplatis au round-trip** : un JSON avec `groups` est aplati en
  touches absolues (comportement actuel du parser) ; l'export écrit un tableau
  `keys` plat. Identique visuellement/électriquement ; seul l'imbriquement est
  perdu. Signalé dans la ligne de statut à la première édition.
- Saisie non numérique → ignorée, champ revient à la valeur courante à la
  reconstruction suivante.
- Buffer vide → panneau d'édition masqué ; Add Key reste disponible.
- Delete → efface la sélection (`selected-index = -1`).
