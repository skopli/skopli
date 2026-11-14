#!/usr/bin/env bash
# Prints the environment that points a harness at an isolated home, one `export` per line.
# usage: os-home.sh [--plain] <cell-home>    eval "$(os-home.sh "$CELL_HOME")"
# --plain prints unquoted NAME=value lines for programs that set the environment themselves.
set -eu
plain=0; [ "${1:-}" = --plain ] && { plain=1; shift; }
[ $# -eq 1 ] || { echo "usage: os-home.sh [--plain] <cell-home>" >&2; exit 2; }
mkdir -p "$1"
home=$(cd "$1" && pwd)
q() { if [ $plain = 1 ]; then printf "%s=%s\n" "$1" "$2"; else printf "export %s=%q\n" "$1" "$2"; fi; }
case "$(uname -s | tr '[:upper:]' '[:lower:]')" in
  linux*)
    q HOME "$home"
    q XDG_CONFIG_HOME "$home/.config"
    q XDG_DATA_HOME "$home/.local/share"
    q XDG_STATE_HOME "$home/.local/state"
    q XDG_CACHE_HOME "$home/.cache";;
  darwin*)
    mkdir -p "$home/Library/Application Support" "$home/Library/Preferences" "$home/Library/Caches"
    q HOME "$home";;
  msys*|mingw*|cygwin*)
    mkdir -p "$home/AppData/Roaming" "$home/AppData/Local"
    win=$(cygpath -w "$home")
    # Native Windows programs cannot resolve an MSYS path such as /c/Users, so HOME is mixed form.
    q HOME "$(cygpath -m "$home")"
    q USERPROFILE "$win"
    q APPDATA "$win\\AppData\\Roaming"
    q LOCALAPPDATA "$win\\AppData\\Local";;
  *) echo "os-home.sh: unsupported platform $(uname -s)" >&2; exit 1;;
esac
