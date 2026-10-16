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
