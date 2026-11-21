//! Reference-only work extracted from upstream 0.5.1 without changing arithmetic.
use super::*;

fn multiply(a: &[f32], b: &[f32], out: &mut [f32]) {
    for ((a, b), out) in a.iter().zip(b).zip(out) {
        *out = a * b;
    }
}

struct Scale {
    width: usize,
    height: usize,
    planes: [Vec<f32>; 3],
    mu: [Vec<f32>; 3],
    sigma_sq: [Vec<f32>; 3],
}

/// Immutable reference features plus a mutable, per-job evaluation workspace.
/// No cache survives an optimization job.
pub struct PreparedReference {
    scales: Vec<Scale>,
    workspace: Workspace,
    compact: bool,
}

/// Typed slots rather than a general arena: ownership prevents accidental aliasing.
/// Products are dead after covariance is blurred, so that slot then stores means.
struct Workspace {
    planes: [Vec<f32>; 3],
    products_or_means: [Vec<f32>; 3],
    sigma: [Vec<f32>; 3],
    cross: [Vec<f32>; 3],
    blur: Blur,
}
impl Workspace {
    fn new() -> Self {
        Self {
            planes: std::array::from_fn(|_| Vec::new()),
            products_or_means: std::array::from_fn(|_| Vec::new()),
            sigma: std::array::from_fn(|_| Vec::new()),
            cross: std::array::from_fn(|_| Vec::new()),
            blur: Blur::new(0, 0),
        }
    }
    fn resize(&mut self, width: usize, height: usize, compact: bool) {
        for plane in &mut self.planes {
            plane.resize(width * height, 0.0);
        }
        for plane in self
            .products_or_means
            .iter_mut()
            .take(if compact { 1 } else { 3 })
        {
            plane.resize(width * height, 0.0);
        }
        self.blur.resize(width, height);
    }
}

impl PreparedReference {
    /// Prepare all reference scales using the same operations as upstream 0.5.1.
    pub fn new(source: LinearRgb) -> Result<Self, Ssimulacra2Error> {
        if source.width() < 8 || source.height() < 8 {
            return Err(Ssimulacra2Error::InvalidImageSize);
        }
        let width = source.width();
        let height = source.height();
        // Small-image batch RSS did not reliably improve in the compact experiment.
        // Retain its prior allocation pattern; use compact scratch above 1.5 MP.
        let compact = width * height > 1_500_000;
        let mut mul: [Vec<f32>; 3] = std::array::from_fn(|c| {
            if c == 0 || !compact {
                vec![0.0; width * height]
            } else {
                Vec::new()
            }
        });
        let mut blur = Blur::new(width, height);
        let mut scales = Vec::with_capacity(NUM_SCALES);
        let mut current = Some(source);
        for scale in 0..NUM_SCALES {
            let Some(source) = current.take() else {
                break;
            };
            let (width, height) = (source.width(), source.height());
            // Build the smaller successor first, then consume the large buffer.
            // Upstream processes one sub-8 scale after downscaling; preserve it.
            current = if scale + 1 < NUM_SCALES && width >= 8 && height >= 8 {
                Some(downscale_by_2(&source))
            } else {
                None
            };
            for plane in &mut mul {
                plane.truncate(width * height);
            }
            blur.shrink_to(width, height);
            let mut xyb = Xyb::from(source);
            make_positive_xyb(&mut xyb);
            let planes = xyb_to_planar(&xyb);
            drop(xyb);
            let (sigma_sq, mu) = if compact {
                let mut sigma_sq = std::array::from_fn(|_| Vec::new());
                let mut mu = std::array::from_fn(|_| Vec::new());
                for c in 0..3 {
                    multiply(&planes[c], &planes[c], &mut mul[0]);
                    blur.blur_plane_into(&mul[0], &mut sigma_sq[c]);
                    blur.blur_plane_into(&planes[c], &mut mu[c]);
                }
                (sigma_sq, mu)
            } else {
                image_multiply(&planes, &planes, &mut mul);
                (blur.blur(&mul), blur.blur(&planes))
            };
            scales.push(Scale {
                width,
                height,
                planes,
                mu,
                sigma_sq,
            });
        }
        Ok(Self {
            scales,
            workspace: Workspace::new(),
            compact,
        })
    }

    /// Evaluate a candidate against these reference features at every original scale.
    pub fn score(&mut self, distorted: LinearRgb) -> Result<f64, Ssimulacra2Error> {
        let first = &self.scales[0];
        if distorted.width() != first.width || distorted.height() != first.height {
            return Err(Ssimulacra2Error::NonMatchingImageDimensions);
        }
        let scratch = &mut self.workspace;
        let mut msssim = Msssim::default();
        let mut current = Some(distorted);
        for (i, s) in self.scales.iter().enumerate() {
            let distorted = current.take().unwrap();
            current = if i + 1 < self.scales.len() {
                Some(downscale_by_2(&distorted))
            } else {
                None
            };
            scratch.resize(s.width, s.height, self.compact);
            let mut xyb = Xyb::from(distorted);
            make_positive_xyb(&mut xyb);
            for (i, pixel) in xyb.data().iter().enumerate() {
                for (c, plane) in scratch.planes.iter_mut().enumerate() {
                    plane[i] = pixel[c];
                }
            }
            drop(xyb);
            let mut avg_ssim = [0.0; 6];
            let mut avg_edgediff = [0.0; 12];
            if self.compact {
                // Channels have independent reductions. Finish one before reusing its
                // scratch buffers; the final weighted-score order remains unchanged.
                for c in 0..3 {
                    multiply(
                        &scratch.planes[c],
                        &scratch.planes[c],
                        &mut scratch.products_or_means[0],
                    );
                    scratch
                        .blur
                        .blur_plane_into(&scratch.products_or_means[0], &mut scratch.sigma[0]);
                    multiply(
                        &s.planes[c],
                        &scratch.planes[c],
                        &mut scratch.products_or_means[0],
                    );
                    scratch
                        .blur
                        .blur_plane_into(&scratch.products_or_means[0], &mut scratch.cross[0]);
                    scratch
                        .blur
                        .blur_plane_into(&scratch.planes[c], &mut scratch.products_or_means[0]);
                    avg_ssim[c * 2..(c + 1) * 2].copy_from_slice(&ssim_map_plane(
                        s.width,
                        s.height,
                        &s.mu[c],
                        &scratch.products_or_means[0],
                        &s.sigma_sq[c],
                        &scratch.sigma[0],
                        &scratch.cross[0],
                    ));
                    avg_edgediff[c * 4..(c + 1) * 4].copy_from_slice(&edge_diff_map_plane(
                        s.width,
                        s.height,
                        &s.planes[c],
                        &s.mu[c],
                        &scratch.planes[c],
                        &scratch.products_or_means[0],
                    ));
                }
            } else {
                image_multiply(
                    &scratch.planes,
                    &scratch.planes,
                    &mut scratch.products_or_means,
                );
                scratch
                    .blur
                    .blur_into(&scratch.products_or_means, &mut scratch.sigma);
                image_multiply(&s.planes, &scratch.planes, &mut scratch.products_or_means);
                scratch
                    .blur
                    .blur_into(&scratch.products_or_means, &mut scratch.cross);
                scratch
                    .blur
                    .blur_into(&scratch.planes, &mut scratch.products_or_means);
                avg_ssim = ssim_map(
                    s.width,
                    s.height,
                    &s.mu,
                    &scratch.products_or_means,
                    &s.sigma_sq,
                    &scratch.sigma,
                    &scratch.cross,
                );
                avg_edgediff = edge_diff_map(
                    s.width,
                    s.height,
                    &s.planes,
                    &s.mu,
                    &scratch.planes,
                    &scratch.products_or_means,
                );
            }
            msssim.scales.push(MsssimScale {
                avg_ssim,
                avg_edgediff,
            });
        }
        Ok(msssim.score())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: usize, h: usize, seed: u32) -> LinearRgb {
        let mut state = seed;
        let data = (0..w * h)
            .map(|_| {
                std::array::from_fn(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    (state >> 8) as f32 / 16777215.0
                })
            })
            .collect();
        LinearRgb::new(data, w, h).unwrap()
    }

    #[test]
    fn prepared_matches_pinned_upstream_bit_for_bit() {
        for (w, h) in [
            (8, 8),
            (9, 17),
            (31, 33),
            (64, 64),
            (257, 129),
            (900, 507),
            (1501, 1000),
        ] {
            let source = frame(w, h, 7);
            let mut prepared = PreparedReference::new(source.clone()).unwrap();
            for candidate in [source.clone(), frame(w, h, 12), frame(w, h, 888)] {
                let expected =
                    upstream::compute_frame_ssimulacra2(source.clone(), candidate.clone()).unwrap();
                let actual = prepared.score(candidate).unwrap();
                assert_eq!(actual.to_bits(), expected.to_bits(), "{w}x{h}");
            }
            // Reusing after unrelated comparisons must not mutate reference features.
            assert_eq!(prepared.score(source).unwrap(), 100.0);
        }
    }

    #[test]
    fn invalid_dimensions_are_rejected() {
        assert!(matches!(
            PreparedReference::new(frame(7, 8, 1)),
            Err(Ssimulacra2Error::InvalidImageSize)
        ));
        let mut prepared = PreparedReference::new(frame(8, 8, 1)).unwrap();
        assert_eq!(
            prepared.score(frame(9, 8, 1)),
            Err(Ssimulacra2Error::NonMatchingImageDimensions)
        );
    }
}
