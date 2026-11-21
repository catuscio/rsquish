# Contributing to rsquish

Small, focused improvements are welcome. Open an issue before a substantial
architecture change so we can agree on scope. English and Korean are both welcome.

## Development

Follow the native dependency setup in the README. The supported build target is
Apple Silicon macOS 26.5+. Run these checks before sending a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --locked --release --workspace --all-targets -- -D warnings
cargo test --locked --release --workspace
node --test tests/i18n.test.cjs
bash scripts/package-macos.sh
```

Keep performance changes measurable. Compare the same images, encoder settings,
probe counts and deadlines; include wall time and peak memory, and check candidate
hashes, scores and selected outputs. Use `scripts/benchmark-batch.py` with four local
images. A faster result with changed quality is a tradeoff, not a free speedup.

Keep `unsafe` safety arguments, ownership assumptions and third-party copyright
notices intact. UI changes should preserve keyboard navigation, visible focus,
Korean text wrapping and before/after comparison behavior. Include screenshots for
visible changes, using synthetic or redistributable images.

Do not upload private photos, EXIF/GPS, local filesystem paths, credentials or
benchmark outputs containing them. Prefer a small synthetic reproducer or a link
to an openly licensed image. New dependencies should have a clear purpose and
compatible licensing.

## Pull requests

Explain the problem, resulting behavior, relevant measurements and checks. Keep
unrelated refactors separate. Add regression tests for correctness changes; avoid
tests that merely duplicate implementation details. Contributions are accepted
under the repository's MIT license, with existing third-party licenses preserved.

## Translations and releases

UI strings live in `desktop/ui/translations.js`. Korean source keys map to English
translations; preserve named `{placeholders}`. Use `I18n.t` for dynamic text and
`data-i18n` bindings for static text. Check both languages, including progress,
errors and comparison dialogs. Language changes must preserve active work.

For a release, update the versions in the CLI and desktop Cargo manifests and
`desktop/tauri.conf.json`, then update `Cargo.lock`. Merge after CI passes and push
a matching `vMAJOR.MINOR.PATCH` tag. The release workflow tests and builds on macOS,
then publishes app/CLI archives and checksums. `bash scripts/package-release.sh`
exercises the same packaging locally. No signing credentials are configured;
artifacts are ad-hoc signed and not notarized.
