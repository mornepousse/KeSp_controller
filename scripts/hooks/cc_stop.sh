#!/usr/bin/env bash
# Hook Claude Code Stop — garde-fou léger : check.sh --fast (~1 s) avant de conclure.
# Le build/e2e complet n'est PAS lancé ici (pas à chaque fin de tour) : il reste
# garanti au pre-push git. Rouge fast -> bloque la conclusion et remonte le détail.
set -uo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO" || exit 1
# Débrayage : TRIPWIRE_OFF=1 (par session) ou un fichier .tripwire-off à la racine
# (par dépôt, à ne pas committer) coupe ce hook net, sans rien désinstaller.
[ "${TRIPWIRE_OFF:-0}" = "1" ] && exit 0
[ -e .tripwire-off ] && exit 0
# python3 requis pour la garde anti-boucle ; sans lui le hook est inactif (signalé).
command -v python3 >/dev/null 2>&1 || { echo "tripwire: python3 absent, hook Stop inactif" >&2; exit 0; }
# Anti-boucle : si on est déjà dans une continuation de Stop hook, ne pas re-bloquer.
IN="$(cat 2>/dev/null || true)"
META="$(printf '%s' "$IN" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(int(bool(d.get("stop_hook_active"))), d.get("session_id") or "-")' 2>/dev/null)"
[ "${META%% *}" = "1" ] && exit 0
SID="${META#* }"; SID="${SID:--}"
# Un rouge donné ne bloque qu'une fois par session : l'agent finit un tour à
# chaque attente (subagent, tâche de fond), lui rejouer le même rouge ne lui
# apprend rien. Même session, même état -> rien relancé. Le pre-push refuse
# toujours. Empreinte recopiée de check.sh : ce hook marche avec toute version.
etat() {
  { git rev-parse HEAD; git diff HEAD
    git ls-files -o --exclude-standard | LC_ALL=C sort | git hash-object --stdin-paths
    for f in scripts/check.sh scripts/tripwire.d/*.sh COMPORTEMENTS.md .tripwire-divergences; do [ -f "$f" ] && git hash-object "$f" || true; done
  } 2>/dev/null | git hash-object --stdin 2>/dev/null
}
RED="$(git rev-parse --git-dir 2>/dev/null || echo .git)/tripwire/stop-red"
if [ "${TRIPWIRE_FORCE:-0}" != "1" ] && [ "$(cat "$RED" 2>/dev/null)" = "$SID $(etat)" ]; then
  exit 0
fi
OUT="$("$REPO/scripts/check.sh" --fast 2>&1)"
rc=$?
if [ "$rc" -eq 0 ]; then rm -f "$RED"; else
  { mkdir -p "$(dirname "$RED")" && printf '%s %s\n' "$SID" "$(etat)" > "$RED"; } 2>/dev/null
fi
if [ "$rc" -ne 0 ]; then
  echo "check.sh --fast est ROUGE avant de conclure (le build/e2e complet tourne au pre-push) :" >&2
  echo "$OUT" | tail -10 >&2
  exit 2
fi
exit 0
