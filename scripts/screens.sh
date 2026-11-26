#!/usr/bin/env bash
# Screenshots of every route of the UI (running on the dev mock backend) and of
# the re-design frames, for side-by-side comparison.
#
#   npm run dev                       # in another terminal (vite, port 1420)
#   scripts/screens.sh [outdir] [port]
#
# outdir defaults to ./screens (git-ignored), port to $PORT or 1420 (the fixed
# port of apps/desktop/vite.config.ts). Builds nothing and starts nothing.
#
# Output files:
#   <route>-1440.png        1440x960, light
#   <route>-760.png         760x1100, light (narrow window)
#   <route>-dark-1440.png   1440x960, dark theme
#   state-<name>.png        hosts page states (empty, error), host-key dialog
#   frame-N.png             the design frames 1, 2, 3, 4, 5, 8 (re-design/N.svg)
#
# The pages are loaded as http://127.0.0.1:PORT/?mock&demo[&theme=dark]#route.
# See apps/desktop/src/lib/dev/mock-ipc.ts for the flags (empty, error, demo,
# edit, theme, hostkey). Environment: CHROME (default google-chrome), JOBS
# (parallel captures, default 4), BUDGET (virtual time per page in ms,
# default 6000), ONLY (a regex: capture only the matching names, for example
# ONLY='^(hosts|sessions)-1440$'). A full run takes a few minutes (each page
# loads the unbundled dev modules).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-./screens}"
PORT="${2:-${PORT:-1420}}"
CHROME="${CHROME:-google-chrome}"
JOBS="${JOBS:-4}"
BUDGET="${BUDGET:-6000}"
BASE="http://127.0.0.1:${PORT}/"

command -v "$CHROME" >/dev/null || { echo "screens: $CHROME not found (set CHROME)" >&2; exit 1; }
curl -fsS -o /dev/null --max-time 5 "$BASE" \
  || { echo "screens: no dev server on $BASE (run: npm run dev)" >&2; exit 1; }

mkdir -p "$OUT"
PROFILES="$(mktemp -d)"
trap 'rm -rf "$PROFILES"' EXIT

# shot NAME WIDTH HEIGHT URL
shot() {
  local name="$1" width="$2" height="$3" url="$4"
  local profile="$PROFILES/$name"
  mkdir -p "$profile"
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --no-first-run \
    --user-data-dir="$profile" --window-size="${width},${height}" \
    --run-all-compositor-stages-before-draw --virtual-time-budget="$BUDGET" \
    --screenshot="$OUT/$name.png" "$url" >/dev/null 2>&1 \
    && echo "  $OUT/$name.png" \
    || echo "  FAILED $name" >&2
}

# Runs shot in the background, at most $JOBS at a time.
queue() {
  [[ -z "${ONLY:-}" || "$1" =~ $ONLY ]] || return 0
  while (($(jobs -rp | wc -l) >= JOBS)); do wait -n || true; done
  shot "$@" &
}

routes=(hosts sessions folders identities sync preferences host-edit)

echo "app routes:"
for route in "${routes[@]}"; do
  # #host-edit alone bounces back to the hosts page: `edit` opens the editor.
  query="mock&demo"
  [[ "$route" == "host-edit" ]] && query="mock&demo&edit"
  queue "$route-1440" 1440 960 "${BASE}?${query}#${route}"
  queue "$route-760" 760 1100 "${BASE}?${query}#${route}"
  queue "$route-dark-1440" 1440 960 "${BASE}?${query}&theme=dark#${route}"
done

wait
echo "states:"
queue "state-hosts-empty" 1440 960 "${BASE}?mock&empty#hosts"
queue "state-hosts-error" 1440 960 "${BASE}?mock&error#hosts"
queue "state-hostkey-dialog" 1440 960 "${BASE}?mock&demo&hostkey#hosts"

wait
echo "design frames:"
for frame in 1 2 3 4 5 8; do
  queue "frame-$frame" 1440 960 "file://${ROOT}/re-design/${frame}.svg"
done

wait
echo "done: $OUT"
