#!/usr/bin/env bash
# Builds ChordProLive.app with only the Command Line Tools, after
# switchblade's packaging/build-app.sh:
#
#   1. cargo builds the release binary
#   2. assembles Contents/{MacOS,Resources}
#   3. copies the app icon to Contents/Resources/AppIcon.icns
#   4. renders Info.plist from Info.plist.in and packaging/version.sh
#   5. ad-hoc codesigns the bundle (an unsigned arm64 binary will not run)
#   6. re-registers it with LaunchServices, so an in-place rebuild shows
#      the current icon rather than a cached one
#
# Flags:
#   --debug            build the debug profile instead of release
#   --sign <identity>  codesign with a real identity instead of ad-hoc ("-")
#   --install          copy the app to /Applications, replacing any there
#   --open             launch the app once it is built (or installed)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROFILE=release
CARGO_PROFILE_FLAG=--release
APP_NAME="ChordProLive"
BUNDLE_ID="com.chordprolive.app"
SIGN_IDENTITY="-"
OPEN_AFTER=0
INSTALL_AFTER=0
# The app icon: a complete .icns (16 to 1024 px). To change it, point this
# at another .icns in assets/icons/.
ICON="$ROOT/assets/icons/icon-01.icns"

while [ $# -gt 0 ]; do
  case "$1" in
    --debug) PROFILE=debug; CARGO_PROFILE_FLAG= ;;
    --sign) SIGN_IDENTITY="$2"; shift ;;
    --open) OPEN_AFTER=1 ;;
    --install) INSTALL_AFTER=1 ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
  shift
done

[ "$(uname)" = Darwin ] || { echo "error: macOS app bundles build only on macOS" >&2; exit 1; }
[ -f "$ICON" ] || { echo "error: no app icon at $ICON" >&2; exit 1; }

eval "$(sh "$ROOT/packaging/version.sh")"

echo "==> building chordpro-live ($PROFILE, v$CPL_VERSION build $CPL_BUILD, $CPL_COMMIT)"
cargo build $CARGO_PROFILE_FLAG --bin chordpro-live
BIN="$ROOT/target/$PROFILE/chordpro-live"
[ -x "$BIN" ] || { echo "error: no binary at $BIN" >&2; exit 1; }

APP="$ROOT/target/$PROFILE/bundle/$APP_NAME.app"
CONTENTS="$APP/Contents"
echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
cp "$BIN" "$CONTENTS/MacOS/chordpro-live"
cp "$ICON" "$CONTENTS/Resources/AppIcon.icns"
sed \
  -e "s/{{VERSION}}/$CPL_VERSION/g" \
  -e "s/{{BUILD}}/$CPL_BUILD/g" \
  -e "s/{{COMMIT}}/$CPL_COMMIT/g" \
  -e "s/{{BUNDLE_ID}}/$BUNDLE_ID/g" \
  "$ROOT/packaging/Info.plist.in" > "$CONTENTS/Info.plist"
plutil -lint "$CONTENTS/Info.plist" >/dev/null

echo "==> codesigning (identity: $SIGN_IDENTITY)"
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --deep --strict "$APP"
echo "built: $APP"

if [ "$INSTALL_AFTER" = 1 ]; then
  DEST="/Applications/$APP_NAME.app"
  echo "==> installing to $DEST"
  rm -rf "$DEST"
  ditto "$APP" "$DEST"
  APP="$DEST"
fi

# An in-place replacement keeps the old bundle's icon in Finder and the
# Dock until LaunchServices re-reads it: touch marks the cached image stale
# and lsregister -f re-reads the plist.
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
touch "$APP"
[ -x "$LSREGISTER" ] && "$LSREGISTER" -f "$APP" || true

if [ "$OPEN_AFTER" = 1 ]; then
  open "$APP"
else
  echo "run with:  open \"$APP\""
fi
