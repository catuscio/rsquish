# raster-metric

Local derivative of `ssimulacra2` **0.5.1**, BSD-2-Clause, from
https://github.com/rust-av/ssimulacra2 (crates.io published source).
`LICENSE` preserves the upstream copyright and terms.

`src/lib.rs`, `src/blur/`, and `build.rs` originate from that release. The
upstream metric arithmetic is unchanged. The upstream tests refer to image
fixtures excluded from the published crate; those tests are replaced with
tests against the separately pinned crates.io dependency.

`src/prepared.rs` adds immutable reference features per scale (XYB planes,
blurred planes, blurred squared planes), reused for multiple candidate scores.
Reference preparation and candidate comparison use the same operations and
scale boundaries as upstream. No approximate score or reduced resolution is used.
The original `compute_frame_ssimulacra2` remains available for uncached evaluation.

Prepared evaluation also moves consumed pyramid buffers instead of cloning them,
and reuses a typed per-job workspace. The product slot becomes the mean slot once
its previous value is dead. `Blur::blur_into` writes to caller-owned storage using
the original arithmetic; its scratch buffer can resize between scales/candidates.
Reference features are immutable, while `score(&mut self, ...)` owns exclusive
access to the workspace. Above 1.5 MP, independent channel reductions reuse one
product/mean, variance and covariance plane. Smaller references retain the prior
three-channel allocation schedule. The core caps prepared metrics at 4 MP.
Per-plane helpers preserve pixel accumulation and final score order.

Default features stay empty, matching the product's previous single-threaded
metric dependency. Upstream is a dev dependency only. If upgrading upstream,
repeat parity tests and end-to-end benchmarks before changing this derivative.
