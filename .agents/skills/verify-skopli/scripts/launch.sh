#!/usr/bin/env bash
set -euo pipefail

root=$(git rev-parse --show-toplevel)
here=$(cd "$(dirname "$0")" && pwd)
run="$root/.verify/run"
site_port=${SKOPLI_SITE_PORT:-4321}
docs_port=${SKOPLI_DOCS_PORT:-4322}

if [ -f "$run/state.sh" ]; then
  echo "launch: $run/state.sh exists; run scripts/cleanup.sh first" >&2
  exit 1
fi
for port in "$site_port" "$docs_port"; do
  if ss -ltn | grep -q ":$port "; then
    echo "launch: port $port is already bound by a process this run did not start" >&2
    exit 1
  fi
done

cd "$root"
pnpm install --frozen-lockfile
pnpm run build:native
pnpm --filter @skopli/docs build
pnpm --filter @skopli/site build

mkdir -p "$run"
trap 'status=$?; if [ $status -ne 0 ]; then bash "$here/cleanup.sh"; fi; exit $status' EXIT
(cd web/site && exec node_modules/.bin/astro preview --host 127.0.0.1 --port "$site_port") >"$run/site.log" 2>&1 &
site_pid=$!
(cd web/docs && exec node_modules/.bin/astro preview --host 127.0.0.1 --port "$docs_port") >"$run/docs.log" 2>&1 &
docs_pid=$!
cat >"$run/state.sh" <<EOF
SITE_PID=$site_pid
SITE_PORT=$site_port
DOCS_PID=$docs_pid
DOCS_PORT=$docs_port
STARTED=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF

ready() { [ "$(curl -s -o /dev/null -w '%{http_code}' "$1")" = 200 ]; }
deadline=$((SECONDS + 60))
until ready "http://127.0.0.1:$site_port/" &&
  ready "http://127.0.0.1:$docs_port/skopli/" &&
  ready "http://127.0.0.1:$docs_port/skopli/pagefind/pagefind-ui.js"; do
  if [ $SECONDS -ge $deadline ]; then
    echo "launch: servers not ready after 60s; stopping them" >&2
    cat "$run/site.log" "$run/docs.log" >&2
    exit 1
  fi
  sleep 1
done

echo "site  http://127.0.0.1:$site_port/          pid $site_pid"
echo "docs  http://127.0.0.1:$docs_port/skopli/   pid $docs_pid"
echo "state $run/state.sh"
