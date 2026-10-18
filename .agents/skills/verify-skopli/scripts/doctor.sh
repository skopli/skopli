#!/usr/bin/env bash
set -uo pipefail

root=$(git rev-parse --show-toplevel)
state="$root/.verify/run/state.sh"
ok=0
fail() { echo "FAIL $*"; ok=1; }
pass() { echo "ok   $*"; }

[ -f "$state" ] || { echo "FAIL no $state; nothing launched by this skill"; exit 1; }
# shellcheck source=/dev/null
. "$state"

server() {
  local name=$1 pid=$2 port=$3
  if kill -0 "$pid" 2>/dev/null; then pass "$name pid $pid alive"; else fail "$name pid $pid not running"; fi
  if ss -ltnp | grep ":$port " | grep -q "pid=$pid,"; then
    pass "$name port $port owned by pid $pid"
  else
    fail "$name port $port not owned by pid $pid"
  fi
}
server site "$SITE_PID" "$SITE_PORT"
server docs "$DOCS_PID" "$DOCS_PORT"

check() {
  local label=$1 url=$2 needle=$3 body
  body=$(curl -s --max-time 5 "$url") || { fail "$label unreachable at $url"; return; }
  if grep -q -- "$needle" <<<"$body"; then pass "$label serves expected content"; else fail "$label missing '$needle' at $url"; fi
}
check "site" "http://127.0.0.1:$SITE_PORT/" '<title>Skopli'
check "docs" "http://127.0.0.1:$DOCS_PORT/skopli/" '<title>Skopli docs</title>'
check "docs theme script" "http://127.0.0.1:$DOCS_PORT/skopli/" 'localStorage.getItem("skopli-theme")'
check "docs search bundle" "http://127.0.0.1:$DOCS_PORT/skopli/pagefind/pagefind-ui.js" 'PagefindUI'
check "docs llms.txt" "http://127.0.0.1:$DOCS_PORT/skopli/llms.txt" '^# Skopli'

for built in "$root/web/docs/dist/skopli/index.html" "$root/web/site/dist/index.html"; do
  name=${built#"$root"/}
  if [ ! -f "$built" ]; then
    fail "$name missing; rebuild via launch"
    continue
  fi
  stale=$(find "$root/web/docs/src" "$root/web/ui/src" "$root/web/site/src" -type f -newer "$built" | head -1)
  if [ -n "$stale" ]; then fail "$name is older than ${stale#"$root"/}; rebuild via launch"; else pass "$name newer than site sources"; fi
done

if [ -f "$root/native/index.cjs" ] && ls "$root/native/"*.node >/dev/null 2>&1; then
  pass "native addon present in native/"
else
  fail "native addon missing; launch runs pnpm run build:native"
fi
[ -f "$root/dist/index.js" ] && pass "TypeScript facade built in dist/" || fail "dist/index.js missing; pnpm install builds it"

[ $ok -eq 0 ] && echo "doctor: ok" || echo "doctor: not healthy"
exit $ok
