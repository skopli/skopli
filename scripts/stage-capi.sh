#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: scripts/stage-capi.sh <target-triple>

Packages skopli.h plus the shared and static libraries from target/<target>/release
into dist-capi/skopli-capi-<version>-<target>.tar.gz (.zip for Windows targets)
and prints the archive path. Run after
  cargo build -p skopli-capi --release --target <target-triple>

example: scripts/stage-capi.sh x86_64-unknown-linux-gnu
EOF
}

case "${1:-}" in
  -h | --help) usage; exit 0 ;;
  "") usage >&2; exit 2 ;;
esac
target=$1
cd "$(dirname "${BASH_SOURCE[0]}")/.."

pkgid="$(cargo pkgid -p skopli-capi)"
stem="skopli-capi-${pkgid##*[@#]}-$target"
stage="dist-capi/$stem"
rm -rf "$stage" "$stage".tar.gz "$stage".zip
mkdir -p "$stage"

cp crates/skopli-capi/include/skopli.h "$stage/"
found=0
for name in libskopli.so libskopli.dylib libskopli.a skopli.dll skopli.dll.lib skopli.lib; do
  f="target/$target/release/$name"
  [ -f "$f" ] || continue
  cp "$f" "$stage/"
  found=1
done
if [ "$found" = 0 ]; then
  echo "no skopli library under target/$target/release; build it first" >&2
  exit 1
fi

cd dist-capi
case "$target" in
  *-windows-*) archive="$stem.zip"; 7z a -tzip -bso0 "$archive" "$stem" ;;
  *) archive="$stem.tar.gz"; tar -czf "$archive" "$stem" ;;
esac
rm -r "$stem"
echo "dist-capi/$archive"
