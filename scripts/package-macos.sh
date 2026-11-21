#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
cargo_bin="${CARGO:-}"
if [[ -z "$cargo_bin" ]]; then
  cargo_bin="$(command -v cargo || true)"
fi
if [[ -z "$cargo_bin" ]]; then
  cargo_bin="$HOME/.cargo/bin/cargo"
fi
"$cargo_bin" build --locked --release -p rsquish -p rsquish-desktop
version="$(target/release/rsquish --version)"
version="${version#rsquish }"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid app version.' >&2; exit 1; }
config_version="$(plutil -extract version raw desktop/tauri.conf.json)"
[[ "$version" == "$config_version" ]] || { echo 'CLI and desktop versions differ.' >&2; exit 1; }
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
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>rsquish</string>
<key>CFBundleIdentifier</key><string>app.rsquish.desktop</string>
<key>CFBundleName</key><string>rsquish</string>
<key>CFBundleDisplayName</key><string>rsquish</string>
<key>CFBundleIconFile</key><string>rsquish.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>26.5</string>
<key>CFBundleLocalizations</key><array><string>en</string><string>ko</string></array>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
otool -L dist/rsquish > dist/cli-linkage.txt
otool -L "$bundle/Contents/MacOS/rsquish" > dist/desktop-linkage.txt
if grep -E '/opt/homebrew|/usr/local' dist/*-linkage.txt; then
  echo 'Unexpected non-system runtime dependency' >&2
  exit 1
fi
printf 'App: %s\nCLI: %s\n' "$bundle" "$project_dir/dist/rsquish"
