#!/bin/sh
# The one derivation of chordpro-live's version, after switchblade's
# packaging/version.sh. Prints shell-eval'able lines:
#
#   CPL_VERSION=0.1.212   major.minor from Cargo.toml and the commit count
#                         as the patch: CFBundleShortVersionString.
#   CPL_BUILD=212         the commit count alone: CFBundleVersion, which
#                         LaunchServices compares, so it only ever rises.
#   CPL_COMMIT=c44aef9    the commit, `-dirty` when the tree differs from it.
#
# Outside a git checkout the Cargo version stands, the build is 0 and the
# commit "unknown".
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE="$(awk -F'"' '/^version/{print $2; exit}' "$ROOT/Cargo.toml")"

if COUNT="$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null)"; then
  COMMIT="$(git -C "$ROOT" rev-parse --short HEAD)"
  [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ] &&
    COMMIT="$COMMIT-dirty"
  VERSION="${BASE%.*}.$COUNT"
else
  COUNT=0
  COMMIT=unknown
  VERSION="$BASE"
fi

echo "CPL_VERSION=$VERSION"
echo "CPL_BUILD=$COUNT"
echo "CPL_COMMIT=$COMMIT"
