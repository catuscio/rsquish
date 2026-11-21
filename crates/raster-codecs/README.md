# Native RGB8 codecs

`encode(Codec, &[u8], width, height, quality)` accepts packed, normalized sRGB8
and returns an owned compressed buffer. `decode_avif(&[u8])` returns packed RGB8,
width and height. The implementation invokes no external executables.

Encoding uses MozJPEG progressive optimized 4:2:0, WebP lossy method 6, or
libavif/libaom 4:2:0 at speed 6 with explicit IQ or SSIM tuning. AVIF output
uses 8-bit full-range YUV and CICP 1/13/6. JPEG and AVIF use one encoder
thread. WebP enables its native `thread_level=1` while retaining method 6.
Quality values are encoder-specific and quality 100 is not RGB-lossless.

`AvifSource` owns converted YUV planes and reuses them across quality probes and
IQ/SSIM tuning. Each call still creates a fresh encoder. The wrapper is neither
Send nor Sync and frees the image on drop. It retains no borrowed RGB pointers.
The core creates this object within its encoding worker, never across threads.

Build libavif with `bash native/build-libavif.sh` from the workspace root.
The script pins official libavif v1.4.2 commit
`c5240fc79fe5c2407e10afd35f5505ef6333ea49`. Other development dependencies are
static archives supplied by Homebrew: MozJPEG, WebP (including SharpYUV),
libaom 3.15.3, and libvmaf (a transitive dependency of this libaom build).
These system dependencies are not fetched or version-pinned by Cargo.

Default prefixes are `/opt/homebrew/opt/{mozjpeg,webp,aom,libvmaf}`.
Override with `RASTER_MOZJPEG_PREFIX`, `RASTER_WEBP_PREFIX`, `RASTER_AOM_PREFIX`,
and `RASTER_VMAF_PREFIX`; `RASTER_AVIF_STATIC_DIR` selects libavif's build
directory. The build copies archives under unique names into Cargo's output
directory to prevent Apple ld from selecting same-named dynamic libraries.

The current development archives target macOS 26 or later; the verified local
build uses `MACOSX_DEPLOYMENT_TARGET=26.5 cargo test -p raster-codecs --lib --release`.
Older macOS targets require rebuilding dependencies for that target. Static
codec linkage still uses macOS system runtimes; it does not imply a portable
Linux executable or an executable independent of the operating system.

Limits: 64 Mi pixels and 32768 per dimension, with WebP's narrower native
dimension limit applying additionally. AVIF decode rejects alpha, animation,
container transforms, embedded ICC profiles and explicit non-sRGB transfer or
primaries. This avoids silently flattening or reinterpreting unsupported input.
Higher layers must handle color conversion and orientation before encoding.

Validation covers all four encoders, AVIF round trips on odd dimensions and
invalid inputs. The release test executable was inspected with `otool -L`:
only `/usr/lib/libc++.1.dylib` and `/usr/lib/libSystem.B.dylib` are dependencies.
Inspect the final application executable separately after linking it.
