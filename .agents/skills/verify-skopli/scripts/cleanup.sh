#!/usr/bin/env bash
set -uo pipefail

root=$(git rev-parse --show-toplevel)
run="$root/.verify/run"
[ -f "$run/state.sh" ] || { echo "cleanup: nothing to stop"; exit 0; }
# shellcheck source=/dev/null
. "$run/state.sh"

left=0
for pid in "$SITE_PID" "$DOCS_PID"; do
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "cleanup: pid $pid already gone"
    continue
  fi
  if ! tr '\0' ' ' <"/proc/$pid/cmdline" | grep -q "astro.* preview --host 127.0.0.1"; then
    echo "cleanup: pid $pid is not an astro preview we started; leaving it alone" >&2
    left=1
    continue
  fi
  kill "$pid"
  for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
  kill -0 "$pid" 2>/dev/null && kill -9 "$pid"
  echo "cleanup: stopped pid $pid"
done
if [ $left -ne 0 ]; then
  echo "cleanup: kept $run/state.sh because a pid was left running" >&2
  exit 1
fi
rm -rf "$run"
echo "cleanup: done; evidence in $root/.verify/evidence/ was kept"
