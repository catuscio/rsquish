#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
[[ "$(uname -m)" == arm64 ]] || { echo 'Release packaging requires Apple Silicon.' >&2; exit 1; }
bash scripts/package-macos.sh
version="$(dist/rsquish --version)"
version="${version#rsquish }"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
[[ -z "${RELEASE_TAG:-}" || "$RELEASE_TAG" == "v$version" ]] || { echo 'Tag does not match the built version.' >&2; exit 1; }
release_dir="$project_dir/dist/releases/v$version"
mkdir -p "$release_dir"
app_name="rsquish-v$version-macos-arm64-app.zip"
cli_name="rsquish-v$version-macos-arm64-cli.tar.gz"
ditto -c -k --sequesterRsrc --keepParent dist/rsquish.app "$release_dir/$app_name"
COPYFILE_DISABLE=1 tar -czf "$release_dir/$cli_name" -C dist rsquish LICENSE THIRD_PARTY_NOTICES.md LICENSES SSIMULACRA2-LICENSE.txt IMAGE-LICENSE-MIT.txt
(cd "$release_dir" && shasum -a 256 "$app_name" "$cli_name" > SHA256SUMS)
cat > "$release_dir/RELEASE_NOTES.md" <<'NOTES'
Local image optimization with a Rust core, a Tauri desktop app, and a CLI.

### Downloads

- **App ZIP**: extract `rsquish.app` and move it to Applications.
- **CLI tar.gz**: extract and run `./rsquish --help`.
- **SHA256SUMS**: download alongside the archives and verify with `shasum -a 256 -c SHA256SUMS`.

**Requires Apple Silicon and macOS 26.5 or newer.** Intel Macs, Windows and Linux binaries are not included.

The app is ad-hoc signed, **not Developer ID signed or notarized**. macOS may block the first launch. If you trust this release, follow [Apple’s instructions for opening an app from an unidentified developer](https://support.apple.com/en-us/102445). You can also build from source.

### Features

- Korean and English UI, with a saved language preference and instant switching.
- Quality or file-size targets across JPEG, WebP and AVIF.
- Memory-aware automatic concurrency, or 1–4 images at a time.
- Thumbnails and an interactive before/after comparison.
- All processing stays local; original files are preserved.

This is an early 0.x release. Transparent, animated and HDR images are not supported yet. See the README for format limits and measured performance.
NOTES
printf 'Release assets: %s\n' "$release_dir"
