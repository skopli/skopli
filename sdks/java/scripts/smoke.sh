#!/usr/bin/env bash
# Build and run the Java SDK smoke test with plain javac/java (no build tool).
#
# When the build host has no Gradle/Maven, the smoke test is compiled and run
# directly. `gradle test` (see build.gradle.kts) is the equivalent for a host
# that has Gradle.
#
# Prereqs:
#   - JDK 22+ on PATH (FFM is stable since 22). Temurin 25 is the dev toolchain:
#       export PATH="/c/Program Files/Eclipse Adoptium/jdk-25.0.4.7-hotspot/bin:$PATH"
#   - The skopli cdylib built at target/release (cargo build --release -p skopli-capi).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sdk_root="$(cd "$here/.." && pwd)"
repo_root="$(cd "$sdk_root/../.." && pwd)"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) libname="skopli.dll"; cpsep=";" ;;
  Darwin) libname="libskopli.dylib"; cpsep=":" ;;
  *) libname="libskopli.so"; cpsep=":" ;;
esac
dll="${SKOPLI_LIBRARY_PATH:-$repo_root/target/release/$libname}"

if [[ ! -f "$dll" ]]; then
  echo "ERROR: cdylib not found at $dll" >&2
  echo "Build it first: cargo build --release -p skopli-capi" >&2
  exit 1
fi

cd "$sdk_root"
classes="build/classes"
test_classes="build/test-classes"
rm -rf "$classes" "$test_classes"
mkdir -p "$classes" "$test_classes"

find src/main/java -name '*.java' > build/sources.txt
javac -d "$classes" @build/sources.txt
javac -d "$test_classes" -cp "$classes" src/test/java/com/skopli/SmokeTest.java

status=0
out="$(java --enable-native-access=ALL-UNNAMED \
  -Dskopli.library.path="$dll" \
  -cp "$classes$cpsep$test_classes" \
  com.skopli.SmokeTest)" || status=$?
echo "$out"
if [[ $status -ne 0 ]]; then
  exit $status
fi

# Guard that the hardening behaviors and per-source matrix actually ran (a
# fixture rename or a silently skipped case must fail the script).
require() {
  if ! grep -q "$1" <<< "$out"; then
    echo "ERROR: expected smoke check missing: $1" >&2
    exit 1
  fi
}
# The two new-reader goldens must run through the production read path (a
# broken options envelope, cwd default, reader registration, or result
# decoding for these fixtures must fail the script rather than pass silently).
require "registry: Harness enum covers id cherrystudio"
require "opencodereview/basic: four events (summary ignored)"
require "trae/basic: four interactions (agent_steps ignored)"
require "cherrystudio/basic: two events (zero-token row dropped)"
require "cherrystudio/basic: legacy-aggregate survives"
require "cherrystudio/basic: invocation cacheWrite"
require "deepseek/basic: four events (chunk + zero-token dropped)"
require "deepseek/basic: zstd session decompressed and read"
require "deepseek/basic: sess-sub is a subagent session"
require "deepseek/basic: truncated .zstd yields a file-level malformed diagnostic"
require "reasonix/basic: one event per session id (dedup)"
require "reasonix/basic: input = cacheMissTokens"
require "reasonix/basic: estimated session is still included"
require "reasonix/basic: absent cache-miss split yields input 0, not promptTokens"
require "reasonix/basic: latest updatedAt instant wins across offset spellings"
require "fetched-empty: cache fetchedAt"
require "cached-empty: cache fetchedAt"
require "live-reload: total fetches"
require "openrouter: wrote pricing-openrouter.json"
require "litellm: wrote pricing-litellm.json"
require "models-dev: wrote pricing-models-dev.json"
require "priceRollups == gold (raw tree equality)"
require "openrouter: cacheFileName formula"
require "litellm: cacheFileName formula"
require "models-dev: cacheFileName formula"
require "constants: defaultTtlMs"
require "constants: fetchTimeoutMs"
require "probe: openrouter-valid"
require "probe: invalid-json"
# Every shared rollup-tz fixture case must have run through the native core;
# a renamed fixture or a partially consumed case set must fail the script.
require "rollup-tz: utc-near-midnight"
require "rollup-tz: tokyo-rolls-forward"
require "rollup-tz: los-angeles-rolls-back"
require "rollup-tz: kolkata-half-hour-offset"
require "rollup-tz: new-york-dst-spring-forward"
require "rollup-tz: new-york-dst-fall-back"
require "rollup-tz: unknown-zone-falls-back-to-utc"
# Every shared rollup-block fixture case must have run through the native core;
# a renamed fixture or a partially consumed case set must fail the script.
require "rollup-block: anchors-to-the-hour"
require "rollup-block: splits-after-window"
require "rollup-block: boundary-event-opens-next-block"
require "rollup-block: idle-gap-larger-than-window"
require "rollup-block: custom-one-hour-width"
require "rollup-block: unsorted-input-is-ordered"
require "rollup-block: kolkata-floors-local-hour"
require "rollup-block: fall-back-first-occurrence-anchors-earlier-hour"
require "rollup-block: fall-back-second-occurrence-anchors-later-hour"
require "rollup-block: fall-back-both-occurrences-separate-blocks"
require "rollup-block: spring-forward-floors-local-hour"
require "rollup-block: three-consecutive-gaps"
require "rollup-block: zero-block-ms-falls-back-to-default"
require "rollup-block: negative-block-ms-falls-back-to-default"
require "rollup-block: non-canonical-slash-timestamp-is-invalid"
require "rollup-block: offsetless-datetime-is-utc-on-both-sides"
# The omitted-blockMs default must be exercised (the fixture always carries an
# explicit width, so a facade that drops the default silently would otherwise
# pass the whole matrix).
require "rollup-block omitted-width: within five hours joins"
require "rollup-block omitted-width: past five hours splits"
# Every shared cache timestamp-boundary fixture case must have run through the
# facade's own cache path; a renamed fixture or a partially consumed case set
# must fail the script.
require "timestamps: integral-ms"
require "timestamps: zero-ms-short-form"
require "timestamps: microseconds-truncated"
require "timestamps: pre-epoch"
require "timestamps: offset-equals-midnight-z"
require "timestamps: garbage"
require "timestamps: write 0"
