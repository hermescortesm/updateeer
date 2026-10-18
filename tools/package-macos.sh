#!/bin/zsh
# Builds Updateeer.app and a .dmg for one target and leaves them in dist/.
#   tools/package-macos.sh aarch64-apple-darwin   ->  dist/Updateeer-<version>-macos-arm64.dmg
#   tools/package-macos.sh x86_64-apple-darwin    ->  dist/Updateeer-<version>-macos-x86_64.dmg
# Needs the rust target (`rustup target add <target>`).
#
# Signing (optional): by default the app is ad-hoc signed and not notarized. With an Apple
# Developer ID certificate in the keychain:
#   SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"   signs the app and the dmg with the hardened runtime
#   NOTARY_PROFILE=updateeer      notarizes the dmg and staples the ticket; the profile is created once with
#                                 `xcrun notarytool store-credentials updateeer --apple-id ... --team-id ...`
set -euo pipefail
cd "${0:A:h}/.."

target=${1:?usage: package-macos.sh <rust target>}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
case $target in
  aarch64-apple-darwin) label=arm64 ;;
  x86_64-apple-darwin) label=x86_64 ;;
  *) echo "unsupported target $target" >&2; exit 1 ;;
esac

cargo build --release --locked --target "$target"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/Updateeer.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/release/updateeer" "$app/Contents/MacOS/updateeer"

# .icns from the 256px icon
iconset="$work/icon.iconset"
mkdir "$iconset"
for s in 16 32 64 128 256; do
  sips -z $s $s assets/icon.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
done
cp "$iconset/icon_32x32.png" "$iconset/icon_16x16@2x.png"
cp "$iconset/icon_64x64.png" "$iconset/icon_32x32@2x.png"
cp "$iconset/icon_256x256.png" "$iconset/icon_128x128@2x.png"
rm "$iconset/icon_64x64.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Updateeer.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Updateeer</string>
  <key>CFBundleDisplayName</key><string>Updateeer</string>
  <key>CFBundleIdentifier</key><string>com.hermescortes.updateeer</string>
  <key>CFBundleExecutable</key><string>updateeer</string>
  <key>CFBundleIconFile</key><string>Updateeer</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

sign=${SIGN_IDENTITY:--}
if [[ $sign == - ]]; then
  codesign --force --deep --sign - "$app"
else
  codesign --force --options runtime --timestamp --sign "$sign" "$app"
  codesign --verify --strict --verbose=2 "$app"
fi

mkdir -p dist
out="dist/Updateeer-$version-macos-$label.dmg"
rm -f "$out"

# Classic installer window: the app on the left, an arrow, the Applications folder on the right.
# dmgbuild writes the window layout itself (no Finder scripting): `pip install dmgbuild`.
dmgbuild=${DMGBUILD:-$(command -v dmgbuild || true)}
if [[ -z $dmgbuild ]]; then
  echo "dmgbuild not found: pip install dmgbuild (or set DMGBUILD=/path/to/dmgbuild)" >&2
  exit 1
fi
"$dmgbuild" -s packaging/macos/dmg-settings.py -D app="$app" -D icon="$app/Contents/Resources/Updateeer.icns" "Updateeer" "$out" >/dev/null
if [[ $sign != - ]]; then
  codesign --force --timestamp --sign "$sign" "$out"
  if [[ -n ${NOTARY_PROFILE:-} ]]; then
    xcrun notarytool submit "$out" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$out"
    spctl --assess --type open --context context:primary-signature --verbose "$out"
  fi
fi
echo "$out"
