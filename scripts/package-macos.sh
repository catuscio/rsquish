#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
"${CARGO:-$HOME/.cargo/bin/cargo}" build --locked --release -p rsquish -p rsquish-desktop
bundle="$project_dir/dist/rsquish.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/rsquish-desktop "$bundle/Contents/MacOS/rsquish"
cp target/release/rsquish dist/rsquish
cp desktop/icons/icon.icns "$bundle/Contents/Resources/rsquish.icns"
cp LICENSE THIRD_PARTY_NOTICES.md "$bundle/Contents/Resources/"
cp LICENSE THIRD_PARTY_NOTICES.md dist/
cp -R LICENSES "$bundle/Contents/Resources/"
cp -R LICENSES dist/
cp crates/raster-metric/LICENSE "$bundle/Contents/Resources/SSIMULACRA2-LICENSE.txt"
cp crates/raster-metric/LICENSE dist/SSIMULACRA2-LICENSE.txt
cp crates/raster-core/IMAGE-LICENSE-MIT "$bundle/Contents/Resources/IMAGE-LICENSE-MIT.txt"
cp crates/raster-core/IMAGE-LICENSE-MIT dist/IMAGE-LICENSE-MIT.txt
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>rsquish</string>
<key>CFBundleIdentifier</key><string>app.rsquish.desktop</string>
<key>CFBundleName</key><string>rsquish</string>
<key>CFBundleDisplayName</key><string>rsquish</string>
<key>CFBundleIconFile</key><string>rsquish.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>26.5</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
otool -L dist/rsquish > dist/cli-linkage.txt
otool -L "$bundle/Contents/MacOS/rsquish" > dist/desktop-linkage.txt
if rg '/opt/homebrew|/usr/local' dist/*-linkage.txt; then
  echo 'Unexpected non-system runtime dependency' >&2
  exit 1
fi
printf 'App: %s\nCLI: %s\n' "$bundle" "$project_dir/dist/rsquish"
