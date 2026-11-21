# Third-party notices

The root MIT license covers original rsquish code. It does not replace the
licenses of third-party source code, libraries or dependencies.

## Source included in this repository

- **SSIMULACRA2 Rust implementation 0.5.1**: BSD-2-Clause. The modified copy in
  `crates/raster-metric` retains its [license](crates/raster-metric/LICENSE) and
  [upstream provenance](crates/raster-metric/UPSTREAM.json). rsquish adds reusable
  reference features and scratch buffers, with parity tests against upstream.
- **image 0.25.10 resize implementation**: MIT. The adapted row-wise resampler in
  `crates/raster-core/src/row_resize.rs` retains the upstream
  [license](crates/raster-core/IMAGE-LICENSE-MIT).

## Native libraries linked during the build

| Library | License notices |
|---|---|
| MozJPEG / libjpeg-turbo / IJG | [License overview and BSD terms](LICENSES/mozjpeg-LICENSE.md), [IJG README](LICENSES/README.ijg) |
| libwebp / SharpYUV | [BSD-style license](LICENSES/webp-COPYING) |
| libavif | [BSD-2-Clause](LICENSES/libavif-LICENSE) |
| libaom | [BSD-style license](LICENSES/aom-LICENSE) |
| libvmaf | [BSD+Patent](LICENSES/libvmaf-LICENSE) |

This software is based in part on the work of the Independent JPEG Group.
Native source is obtained separately by the build script or development package
manager; it is not vendored into this repository. Preserve the licenses and any
patent grants supplied with the versions you build and distribute.

Other Cargo dependencies, including Tauri, Little CMS bindings and their native
source, remain subject to their individual license terms. Cargo.lock records the
selected versions; use `cargo metadata --locked` and each dependency's upstream
license when preparing a binary distribution.

## Branding

The original transparent rsquish symbol was generated for this project with an
image-generation tool. It depicts an orange photographic tile folded into a
compressed ribbon. PNG and ICNS exports use the same artwork. The wordmark is text;
no third-party logo or icon set is incorporated.
