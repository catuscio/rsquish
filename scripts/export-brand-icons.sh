#!/usr/bin/env bash
set -euo pipefail
cd "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
master="assets/brand/rsquish.png"
iconset="$(mktemp -d)/rsquish.iconset"
trap 'rm -rf "$(dirname "$iconset")"' EXIT
mkdir -p "$iconset" desktop/ui/brand desktop/icons
sips -z 128 128 "$master" --out desktop/ui/brand/logo.png >/dev/null
sips -z 1024 1024 "$master" --out desktop/icons/icon.png >/dev/null
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$master" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$master" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o desktop/icons/icon.icns
