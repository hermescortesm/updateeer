#!/bin/zsh
# Builds Updateeer.app and a .dmg for one target and leaves them in dist/.
#   tools/package-macos.sh aarch64-apple-darwin   ->  dist/Updateeer-<version>-macos-arm64.dmg
#   tools/package-macos.sh x86_64-apple-darwin    ->  dist/Updateeer-<version>-macos-x86_64.dmg
# Needs the rust target (`rustup target add <target>`). The app is ad-hoc signed, not notarized.
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

codesign --force --deep --sign - "$app"

mkdir -p dist
stage="$work/dmg"
mkdir "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
out="dist/Updateeer-$version-macos-$label.dmg"
rm -f "$out"
hdiutil create -quiet -volname "Updateeer $version" -srcfolder "$stage" -ov -format UDZO "$out"
echo "$out"
