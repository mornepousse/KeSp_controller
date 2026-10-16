#!/usr/bin/env bash
# Hook Claude Code SessionStart — auto-installe les hooks git (une fois par clone)
# et rappelle la variante courante. Ne bloque jamais la session (exit 0).
set -uo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO" || exit 0
# Débrayage : TRIPWIRE_OFF=1 (par session) ou un fichier .tripwire-off à la racine
# (par dépôt, à ne pas committer) coupe ce hook net, sans rien désinstaller.
[ "${TRIPWIRE_OFF:-0}" = "1" ] && exit 0
[ -e .tripwire-off ] && exit 0
CTX=""
# core.hooksPath n'est posé que s'il ne court-circuite rien : des hooks d'équipe
# dans .git/hooks, ou un core.hooksPath déjà choisi, restent maîtres.
HP="$(git config --get core.hooksPath 2>/dev/null)"
HD="$(git rev-parse --git-path hooks 2>/dev/null || echo .git/hooks)"
if [ "$HP" = "scripts/hooks" ] || grep -qs 'scripts/check.sh' "${HP:-$HD}/pre-push"; then
  :
elif [ -n "$HP" ] || ls "$HD" 2>/dev/null | grep -qv '\.sample$'; then
  CTX="tripwire: hooks git déjà en place (${HP:-$HD}), core.hooksPath non posé — pre-push tripwire inactif ; /tripwire:init (cohabitation)."
elif git config core.hooksPath scripts/hooks 2>/dev/null; then
  CTX="tripwire: hooks git installés (core.hooksPath=scripts/hooks)."
else
  CTX="tripwire: installation des hooks git impossible — lancer ./scripts/install-hooks.sh."
fi
[ -z "$CTX" ] && exit 0
# Forme documentée : additionalContext en JSON ; repli texte brut sans python3.
if command -v python3 >/dev/null 2>&1; then
  python3 -c 'import json,sys; print(json.dumps({"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":sys.argv[1]}}, ensure_ascii=False))' "$CTX"
else
  printf '%s\n' "$CTX"
fi
exit 0
