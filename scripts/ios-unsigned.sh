#!/usr/bin/env bash
# Builds the iPhone and iPad app WITHOUT an Apple developer account, on a Mac.
# The result is an unsigned .ipa: it cannot be installed as it is, but a
# sideloading tool (AltStore, Sideloadly, TrollStore where it applies) signs it
# with a free Apple ID while installing. See docs/MOBILE.md.
#
#   npx tauri ios init --ci && node scripts/mobile-setup.mjs ios && scripts/ios-unsigned.sh
#
# Newer Tauri versions can do this themselves (`tauri ios build --no-sign`);
# that is tried first. If this version does not know the option, or produces no
# .ipa, the app is archived with signing switched off and packed by hand.
set -uo pipefail
cd "$(dirname "$0")/.."

apple="src-tauri/gen/apple"
out="$apple/build/unsigned"
[ -d "$apple" ] || { echo "ios-unsigned: $apple is missing. Run 'npx tauri ios init' first." >&2; exit 1; }
rm -rf "$out"
mkdir -p "$out"

find_ipa() { find "$apple/build" -name '*.ipa' -not -path "$out/*" -newer "$out" 2>/dev/null | head -n 1; }

echo "ios-unsigned: trying 'tauri ios build --no-sign'"
if npx tauri ios build --no-sign; then
  ipa="$(find_ipa)"
  if [ -n "$ipa" ]; then
    cp "$ipa" "$out/FAST-File-Explorer-unsigned.ipa"
    echo "ios-unsigned: made $out/FAST-File-Explorer-unsigned.ipa"
    exit 0
  fi
fi

echo "ios-unsigned: packing by hand with code signing switched off"
# Xcode reads this file on top of the project's own settings, for every target.
config="$(mktemp -t fast-nosign).xcconfig"
cat > "$config" <<'XC'
CODE_SIGNING_ALLOWED = NO
CODE_SIGNING_REQUIRED = NO
CODE_SIGN_IDENTITY =
CODE_SIGN_ENTITLEMENTS =
EXPANDED_CODE_SIGN_IDENTITY =
DEVELOPMENT_TEAM =
PROVISIONING_PROFILE_SPECIFIER =
XC
export XCODE_XCCONFIG_FILE="$config"
# The archive step succeeds; the export step after it needs an account and fails. That is expected.
npx tauri ios build --export-method debugging || echo "ios-unsigned: the export step failed, as expected without an account"

app="$(find "$apple/build" "$HOME/Library/Developer/Xcode/Archives" -path '*.xcarchive/Products/Applications/*.app' -maxdepth 8 -newer "$out" -type d 2>/dev/null | head -n 1)"
if [ -z "$app" ]; then
  app="$(find "$apple/build" -name '*.app' -type d -newer "$out" -not -path '*-iphonesimulator/*' 2>/dev/null | head -n 1)"
fi
[ -n "$app" ] || { echo "ios-unsigned: no built .app was found; the build itself failed (see above)." >&2; exit 1; }

stage="$(mktemp -d)"
mkdir "$stage/Payload"
cp -R "$app" "$stage/Payload/"
# Leftovers of a signature would make sideloading tools refuse the app.
find "$stage/Payload" -name '_CodeSignature' -type d -prune -exec rm -rf {} +
find "$stage/Payload" -name 'embedded.mobileprovision' -delete
(cd "$stage" && zip -qry "FAST-File-Explorer-unsigned.ipa" Payload)
cp "$stage/FAST-File-Explorer-unsigned.ipa" "$out/"
echo "ios-unsigned: made $out/FAST-File-Explorer-unsigned.ipa from $app"
