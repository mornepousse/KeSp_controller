# KeSp_controller — claude instructions

## Tests

- Rapide : `cargo test` (parsers protocole, décodage binaire, keycodes)
- Complet : `cargo clippy --all-targets -- -D warnings && cargo build --release`

### Norme TDD — nouvelle logique pure
Toute nouvelle fonction de logique pure (parsers de protocole, décodage/encodage
binaire, mapping keycodes, sérialisation config) : test écrit **d'abord**,
ajouté à la suite de tests de la phase rapide. Le test doit être rouge avant
l'implémentation, vert après, et parallel-safe (pas d'état global muté).

## Release

Le tag git `vX.Y.Z` est la source de vérité de la version. La CI GitLab
(`.gitlab-ci.yml`) se déclenche sur tout tag `v*` : elle compile Linux et
Windows, téléverse les deux binaires dans le registre de paquets générique, et
crée la release. **Rien à construire à la main.**

**Fichiers dupliquant la version, à synchroniser AVANT le tag :**
- `Cargo.toml` — champ `version`. Il était resté à `1.0.0` jusqu'à v2.2.0 alors
  que les tags étaient en 2.x ; c'est ce que le binaire annonce à qui
  l'interroge.

**`flake.nix` se met à jour APRÈS le tag, jamais avant.** Son `version` sert à
bâtir l'URL de l'artefact et son `hash` adresse le contenu — or le hash ne peut
être relevé qu'une fois la CI passée. Les deux changent dans le **même commit**,
après vérification :

```bash
# le flake télécharge depuis la release GITHUB (pas le registre GitLab) :
# créer d'abord la release GitHub avec les binaires de la CI GitLab
nix store prefetch-file --json \
  "https://github.com/mornepousse/KeSp_controller/releases/download/vX.Y.Z/KeSp_controller-linux-x86_64"
# puis: version ET hash dans flake.nix, et `nix build .#default` doit passer
```

Pourquoi cette règle : au bump v2.1.0 (commit `1036005`, juin 2026) la version a
été montée sans le hash. `fetchurl` étant adressé par contenu, nix a servi le
binaire de v2.0.9 sous l'étiquette v2.1.0 pendant cinq mois, sans erreur — et un
poste neuf aurait échoué sur désaccord de hash. Corrigé en `5ca5cb8`.

### Smoke test

Avant le tag, en `cargo run` sous `nix develop` : l'app démarre, se connecte,
lit la keymap, une assignation parvient à l'appareil, un changement de
disposition relabellise les touches.

Après publication et AVANT de toucher au flake : **lancer le binaire
téléversé**. `cargo run` valide le code source dans le dev shell ; la release
livre un binaire construit par la CI puis repatché par `autoPatchelfHook`. Ce
sont deux environnements, et c'est l'écart entre eux qui a laissé passer
l'échec GL de septembre 2026 — `cargo run` marchait, le binaire empaqueté ne
démarrait pas.
