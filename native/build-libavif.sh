#!/usr/bin/env bash
set -euo pipefail

# Runtime uses static archives only. Development prerequisites: git, cmake,
# a C compiler and a static libaom 3.15.3 installation.
native_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
avif_source="$native_dir/libavif"
avif_build="${RASTER_AVIF_STATIC_DIR:-$native_dir/libavif-build}"
aom_prefix="${RASTER_AOM_PREFIX:-/opt/homebrew/opt/aom}"
avif_commit="c5240fc79fe5c2407e10afd35f5505ef6333ea49"

if [[ ! -f "$avif_source/CMakeLists.txt" ]]; then
  git clone --depth 1 --branch v1.4.2 https://github.com/AOMediaCodec/libavif.git "$avif_source"
fi
actual_commit="$(git -C "$avif_source" rev-parse HEAD)"
if [[ "$actual_commit" != "$avif_commit" ]]; then
  echo "Expected libavif v1.4.2 commit $avif_commit; found $actual_commit" >&2
  exit 1
fi
if [[ ! -f "$aom_prefix/lib/libaom.a" ]]; then
  echo "Missing static libaom archive: $aom_prefix/lib/libaom.a" >&2
  exit 1
fi

cmake -S "$avif_source" -B "$avif_build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
  -DBUILD_SHARED_LIBS=OFF \
  -DAVIF_CODEC_AOM=SYSTEM \
  -DAVIF_CODEC_AOM_ENCODE=ON -DAVIF_CODEC_AOM_DECODE=ON \
  -DAOM_LIBRARY="$aom_prefix/lib/libaom.a" \
  -DAOM_INCLUDE_DIR="$aom_prefix/include" \
  -DAVIF_CODEC_DAV1D=OFF -DAVIF_CODEC_LIBGAV1=OFF \
  -DAVIF_CODEC_RAV1E=OFF -DAVIF_CODEC_SVT=OFF -DAVIF_CODEC_AVM=OFF \
  -DAVIF_LIBYUV=OFF -DAVIF_LIBSHARPYUV=OFF -DAVIF_LIBXML2=OFF \
  -DAVIF_ZLIBPNG=OFF -DAVIF_JPEG=OFF \
  -DAVIF_BUILD_APPS=OFF -DAVIF_BUILD_TESTS=OFF \
  -DAVIF_BUILD_EXAMPLES=OFF -DAVIF_BUILD_MAN_PAGES=OFF
cmake --build "$avif_build" --config Release --parallel "${RASTER_BUILD_JOBS:-4}"
printf 'libavif v1.4.2 commit %s\n' "$avif_commit" > "$avif_build/RASTERKIT-SOURCE.txt"
shasum -a 256 "$aom_prefix/lib/libaom.a" "$avif_build/libavif.a" >> "$avif_build/RASTERKIT-SOURCE.txt"
