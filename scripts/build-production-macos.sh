#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS production builds require macOS" >&2
  exit 1
fi
if [[ -z "${INKSAY_CODESIGN_IDENTITY:-}" ]]; then
  echo "Set INKSAY_CODESIGN_IDENTITY to an Apple Development signing identity" >&2
  exit 1
fi

cd "$(dirname "$0")/.."
node scripts/check-release.mjs
export INKSAY_BUILD_NUMBER="${INKSAY_BUILD_NUMBER:-$(date +%s)}"
npm run tauri build -- --bundles app --config src-tauri/tauri.production.conf.json

bundle="src-tauri/target/release/bundle/macos/InkSay.app"
sidecar="$bundle/Contents/MacOS/whisper-server"
identifier=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$bundle/Contents/Info.plist")
if [[ "$identifier" != "com.supersoloer.inksay.macos" ]]; then
  echo "Unexpected production bundle identifier: $identifier" >&2
  exit 1
fi

codesign --force --sign "$INKSAY_CODESIGN_IDENTITY" --options runtime "$sidecar"
codesign --force --sign "$INKSAY_CODESIGN_IDENTITY" --options runtime --entitlements src-tauri/Entitlements.plist "$bundle"
entitlements=$(codesign -d --entitlements :- "$bundle" 2>/dev/null)
if [[ "$entitlements" != *"com.apple.security.device.audio-input"* ]]; then
  echo "Signed production bundle is missing the audio-input entitlement" >&2
  exit 1
fi
codesign --verify --strict --deep --verbose=2 "$bundle"
echo "Signed production bundle: $PWD/$bundle"

node scripts/package-release.mjs macos "$bundle"
