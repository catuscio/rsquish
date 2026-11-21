<p align="center">
  <img src="assets/brand/rsquish.png" width="160" height="160" alt="rsquish — a folded orange image tile">
</p>
<h1 align="center">rsquish</h1>
<p align="center"><strong>Smaller images. Measured quality. All on your machine.</strong></p>
<p align="center">A local image optimizer with a Rust core, a native desktop UI, and a standalone CLI.</p>
<p align="center">
  <a href="https://github.com/catuscio/rsquish/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/catuscio/rsquish/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-347047?style=flat-square"></a>
  <img alt="Rust core" src="https://img.shields.io/badge/core-Rust-e8592c?style=flat-square">
  <img alt="Tauri desktop" src="https://img.shields.io/badge/desktop-Tauri-242424?style=flat-square">
  <img alt="JPEG, WebP and AVIF output" src="https://img.shields.io/badge/output-JPEG%20%C2%B7%20WebP%20%C2%B7%20AVIF-242424?style=flat-square">
  <img alt="Local processing" src="https://img.shields.io/badge/processing-local-347047?style=flat-square">
</p>
<p align="center"><a href="#quick-start">Quick start</a> · <a href="#measured-not-guessed">Benchmarks</a> · <a href="#where-the-speed-comes-from">Under the hood</a> · <a href="#desktop">Desktop</a> · <a href="#build-from-source">Build</a></p>

---

**Pick the quality you want, or the file size you need. rsquish tests real encodes and finds a result that fits.**

Instead of treating an encoder's quality slider as a universal measure, rsquish compresses candidates, decodes them, and measures their visual quality with **SSIMULACRA2**. JPEG, WebP, and two AVIF tuning modes compete on the same normalized reference image.

| Measured batch speed | Local by design | One engine, two interfaces |
|:--:|:--:|:--:|
| **3.04× throughput** with four concurrent images in our 2400px test | No uploads, account, or server | Tauri desktop + Rust CLI |

> Quality is measured, not magically guaranteed. A perceptual score is not a promise of visually lossless output, and a bounded search is not proof of the global optimum.

## Quick start

After building, use the standalone CLI or open the desktop app:

```sh
# Let rsquish choose concurrency from image size, CPU capacity and free memory.
./dist/rsquish optimize photos/*.jpg --output-dir ./optimized \
  --max-width 2400 --jobs auto

# Fit each image within 200 kB; choose the highest-scoring feasible candidate.
./dist/rsquish optimize photo.png --output-dir ./optimized \
  --mode budget --max-bytes 200000

# Prefer a measured quality threshold; compare all four encoder modes.
./dist/rsquish optimize photo.jpg --output-dir ./optimized \
  --mode target --target-score 85 \
  --codecs mozjpeg,webp,avif-iq,avif-ssim --jobs 2

open dist/rsquish.app
```

Existing inputs and output files are never overwritten. Results receive unique names and a JSON sidecar containing candidates, scores, hashes, and timings. CLI stdout is a JSON result array in input order; progress goes to stderr. Add `--quiet` for scripting.

## Measured, not guessed

**Four photos. Same encoder settings. Identical selected output bytes.**

Apple M4 Pro · 14 logical CPUs · 48 GiB RAM · macOS 26.5.1. Four photos resized to a maximum width of 2400px, target score 85, four encoder modes, seven probes per mode. Two runs per setting, with the second run reversing the order. Values below are the mean batch wall time and mean process peak RSS.

| Concurrent images | Batch time ↓ | Throughput vs. 1 worker | Peak memory |
|:--|--:|--:|--:|
| 1 | 38.93 s | 1.00× | 594 MiB |
| 2 | 22.65 s | 1.72× | 1,103 MiB |
| 3 | 15.18 s | 2.57× | 1,485 MiB |
| **4** | **12.81 s** | **3.04×** | **1,765 MiB** |

The full 900px + 2400px experiment checked **64 output reports and 1,792 candidate records**. Candidate hashes, measured scores, and selected output hashes matched each resolution's serial reference. Original files stayed unchanged.

This is a comparison of rsquish's own worker counts, **not a claim against another optimizer**. More concurrency uses more memory. Results vary with input, hardware, and system load. The 300-second per-image benchmark budget was not reached; tighter deadlines can change which candidates finish under contention.

[Machine-readable measurements](assets/benchmark.json) · [Reproduce the benchmark](#reproduce-the-benchmark)

### Reproduce the benchmark

The four-photo dataset is private and is not included. Run the same checks with four local images:

```sh
cargo build --locked --release -p rsquish
python3 scripts/benchmark-batch.py a.jpg b.jpg c.jpg d.jpg
```

The runner performs 16 trials across 900px and 2400px widths, checks candidate/output parity, and deletes temporary outputs. It writes detailed results to the git-ignored `artifacts/batch-benchmark.json`. That local report contains input paths; review it before sharing. Different inputs will produce different timings.

## Where the speed comes from

**Less repeated work, bounded parallelism, and memory that gets reused.** The codec quality settings stay intact.

### 1. Encode the next candidate while measuring this one

For reference images up to 4 MP, one encoder worker overlaps the next candidate's encoding with the current candidate's decode and quality measurement. Single-slot queues keep the pipeline bounded; candidate consumption remains ordered. Above that threshold, the optimizer uses the serial path to avoid overlapping large working buffers.

### 2. Prepare once, compare many times

SSIMULACRA2 repeatedly compares candidates against the same source. rsquish lazily prepares and reuses the source's multi-scale features up to 4 MP. AVIF probes similarly reuse the source RGB→YUV conversion while creating a fresh encoder for each quality probe. Identical encoded candidates can reuse their measured score.

### 3. Skip work that cannot change the answer

In file-size mode, a candidate already over the byte ceiling cannot win. Its size stays in the search history, but decoding and quality evaluation are skipped. Eligible candidates still receive the full metric. Coarse probes, bracket refinement, and nearby quality checks keep search bounded; final selection uses actual measured results rather than assuming size or quality is monotonic.

### 4. Move buffers instead of copying them

Rust ownership transfers large pixel buffers between stages. ICC conversion runs in the owned RGB allocation, and alpha checks borrow existing pixels. A row-wise Lanczos3 resize avoids a full floating-point intermediate. Pooled scratch buffers and a channel-at-a-time metric path for larger cached references reduce temporary storage while preserving tested pixel/score parity.

### 5. Keep parallelism useful

A shared Rust worker pool processes **1–4 images**. `--jobs auto` checks dimensions, CPU count, and free memory at batch start; if memory information is unavailable, it falls back to one worker. Individual encoders retain their validated settings—AVIF remains single-threaded. More threads are not automatically better.

### 6. Make the interface cheap to use

Visible and nearby thumbnails load first, with at most two thumbnail requests in flight. A bounded **16 MiB native PNG cache** reuses exact previews and validates file metadata before reuse. The desktop uses the system WebView through Tauri. The CLI links its codecs statically, so it does not spawn external encoders or require an encoder executable at runtime.

<details>
<summary><strong>What we deliberately did not trade away</strong></summary>

- No lower quality threshold or cheaper approximate metric to manufacture a speedup.
- No dropped codec candidates merely because another codec is usually faster.
- No approximate image decode for cached previews.
- An AVIF two-thread experiment was rejected when it changed output bytes.
- A memory-budget experiment that serialized encoding/evaluation was rejected after it slowed the tested workload by roughly 31%.

Byte/score parity is verified on tests and measured workloads; it is not a universal guarantee for every image or a run stopped early by a deadline. Cache limits and automatic worker estimates are not hard limits on total application memory.

</details>

## Desktop

A focused interface for everyday batches:

- **용량 줄이기 / 균형 있게 / 선명하게** — small, balanced, and sharp starting presets.
- Choose a quality target or a file-size ceiling, then select formats and concurrency.
- See thumbnails, open a larger preview, and drag a **before/after comparison slider**.
- Follow each image's progress while the batch shows completed and running counts.
- Cancel running work together; undispatched images remain queued for the next start.

The current interface is in Korean. Presets are convenient starting values, not visual-quality guarantees. A changed option does not silently deselect the chosen preset.

## Formats & boundaries

| | Current support |
|:--|:--|
| Input | Opaque 8-bit JPEG, PNG, WebP; at most 32 MP and 256 MiB per input |
| Output | JPEG via MozJPEG; WebP; AVIF via libavif/libaom with IQ and SSIM tuning |
| Color | RGB ICC → sRGB; EXIF orientation applied; untagged input interpreted as sRGB |
| Resize | Optional Lanczos3 downscale; score measured against the resized reference |
| Metadata | Original EXIF/GPS and other source metadata are removed |
| Not yet supported | Transparent images, animation, 16-bit/HDR, CMYK/non-RGB ICC, AVIF optimization input |
| Verified desktop target | Apple Silicon macOS 26.5+ with the current native dependency build |

Quality 100 is not a lossless-output switch. Unsupported input types are rejected rather than silently flattened. Budget mode writes no image if no candidate fits; target mode can produce a file larger than the input when the quality requirement demands it.

Cancellation and soft deadlines are checked at processing boundaries; an in-flight native codec call is not forcibly interrupted. Already saved results remain on disk. Exit codes: `0` success, `1` processing error, `2` unmet constraint/time limit without a result, `130` cancellation. Invalid CLI arguments also use `2`.

## Build from source

**Current build path: Apple Silicon macOS.** Install Rust and Xcode Command Line Tools, then the native development dependencies:

```sh
brew install cmake mozjpeg webp aom libvmaf
bash native/build-libavif.sh

cargo build --locked --release -p rsquish -p rsquish-desktop
cargo test --locked --release --workspace
bash scripts/package-macos.sh
```

Outputs: `dist/rsquish` and `dist/rsquish.app`. The packaging script verifies ad-hoc signing and checks that no Homebrew runtime dylib dependency leaked into either executable. This is a local build, not a notarized public release or a published Homebrew formula.

Cargo dependencies and the libavif source commit are pinned; Homebrew-provided static archives are **not** pinned by Cargo. Their versions and deployment targets matter. See [native codec build notes](crates/raster-codecs/README.md). Supporting older macOS releases requires rebuilding those dependencies for the older target; changing a version string is not enough.

## Project map

```text
crates/raster-cli      rsquish CLI
crates/raster-core     search, color handling, batch scheduling, preview cache
crates/raster-codecs   statically linked native codec wrapper
crates/raster-metric   SSIMULACRA2 derivative with reusable reference features
desktop               Tauri shell and frontend
assets/brand          rsquish logo
assets/benchmark.json publication-safe benchmark measurements
```

The product is **rsquish**; internal `raster-*` crate names and `RASTER_*` build overrides are retained for now.

## Built on good work

rsquish combines Rust orchestration with established native C/C++ encoders. It is not a from-scratch Rust rewrite of JPEG, WebP, or AV1. Thanks to Tauri, MozJPEG, libwebp, libavif, libaom, Little CMS, the `image` crate, and SSIMULACRA2.

The metric derivative retains its [upstream license](crates/raster-metric/LICENSE) and [provenance](crates/raster-metric/UPSTREAM.json); the resize implementation retains the [image license](crates/raster-core/IMAGE-LICENSE-MIT). Native dependencies retain their own licenses. [Third-party notices](THIRD_PARTY_NOTICES.md).

## Contributing

Bug reports, focused pull requests and reproducible benchmarks are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md) before opening a PR. For security issues, use
[private vulnerability reporting](SECURITY.md). Please follow our
[code of conduct](CODE_OF_CONDUCT.md).

## License

Original rsquish code is licensed under [MIT](LICENSE). Vendored code and native
libraries retain their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
This software is based in part on the work of the Independent JPEG Group.
