# KeSp_controller — claude instructions

## Workflow anti-régression (OBLIGATOIRE)

Source unique de vérité : `scripts/check.sh`.
- `./scripts/check.sh --fast` — cargo test (parsers protocole, décodage binaire, keycodes) (~secondes)
- `./scripts/check.sh` — fast + clippy -D warnings + cargo build --release

**Activation des hooks git (une fois par clone)** :
```bash
./scripts/install-hooks.sh   # ou: git config core.hooksPath scripts/hooks
```
`pre-push` lance le check complet et bloque le push si rouge. WIP : `git push --no-verify`.

**Hooks Claude Code** (`.claude/settings.json`, automatiques). Échelle de
gravité : **pendant → informe, à la conclusion → bloque, au push → bloque.**
- Plus de hook par édition (tripwire v0.17.0) : lancer `./scripts/check.sh --fast` soi-même en cours de route ; le `Stop` bloque une fois par état rouge.
- `Stop` → `check.sh --fast` (~1 s) et il **bloque** : on ne conclut pas un tour
  sur du rouge. Le build complet (clippy -D warnings + cargo build --release) n'est PAS relancé à
  chaque fin de tour, il reste garanti au pre-push git.
- `pre-push` → check complet, **bloquant**.

Un rouge de Stop ou de pre-push ne s'ignore jamais et ne se contourne pas par
`--no-verify` sans raison écrite.

**Divergences déclarées** : `.tripwire-divergences` (committé) liste les écarts
assumés au scaffold standard — mode maison, dégradation d'environnement, alias
de dialecte. Une ligne `fichier<TAB>motif<TAB>pourquoi` ; `check.sh` rend rouge
la disparition d'un motif déclaré. Le fichier hôte d'une divergence doit être
**suivi par git** : un fichier gitignoré ne change pas l'empreinte du
skip-si-déjà-vert, donc sa perte peut passer sous un « déjà vert — skip » — il
n'est pas protégé de façon fiable. **Limite** : un écart non déclaré n'est
protégé par rien et le prochain re-scaffold l'effacera — toute divergence
délibérée se déclare au moment où on l'introduit.

### Norme TDD — nouvelle logique pure
Toute nouvelle fonction de logique pure (parsers de protocole, décodage/encodage
binaire, mapping keycodes, sérialisation config) : test écrit **d'abord**,
ajouté à la suite de tests de la phase rapide. Le test doit être rouge avant
l'implémentation, vert après, et parallel-safe (pas d'état global muté).

### Économie de modèles (subagents)
Le pipeline check.sh permet de descendre en gamme SANS risque d'hallucination,
mais seulement là où un oracle rattrape l'erreur :
- **Modèle économique (haiku) OK** : transcription de code déjà spécifié,
  refactors mécaniques, extraction citée (`fichier:ligne` obligatoire) — le
  check, la compilation ou le recoupement des citations attrapent la dérive.
- **Jamais en dessous de sonnet** : review, audit, debug, **et l'écriture
  d'assertions de test** — une assertion tautologique ou un verdict halluciné
  passent l'oracle mécanique au vert. Le jugement ne descend pas en gamme.
- Toute tâche économique DOIT finir par `./scripts/check.sh --fast` vert, et
  un test rewiré/écrit DOIT prouver qu'il mord (bug transitoire → rouge → revert).

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
nix store prefetch-file --json \
  "https://gitlab.com/api/v4/projects/harrael%2FKeSp_controller/packages/generic/kesp-controller/vX.Y.Z/KeSp_controller-linux-x86_64"
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
