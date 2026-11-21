//! RGB8 Lanczos3 resampling with one intermediate row.
//! Adapted from image 0.25.10 imageops/sample.rs; see ../IMAGE-LICENSE-MIT.
//! Preserve vertical-then-horizontal arithmetic and per-pixel summation order.
use image::{Rgb, RgbImage};

fn weights(size: u32, target: u32, out: u32, ws: &mut Vec<f32>) -> u32 {
    let ratio = size as f32 / target as f32;
    let sratio = ratio.max(1.0);
    let support = 3.0 * sratio;
    let center = (out as f32 + 0.5) * ratio;
    let left = ((center - support).floor() as i64).clamp(0, i64::from(size) - 1) as u32;
    let right =
        ((center + support).ceil() as i64).clamp(i64::from(left) + 1, i64::from(size)) as u32;
    let center = center - 0.5;
    ws.clear();
    let mut sum = 0.0;
    for i in left..right {
        let x = (i as f32 - center) / sratio;
        let sinc = |t: f32| {
            let a = t * std::f32::consts::PI;
            if t == 0.0 { 1.0 } else { a.sin() / a }
        };
        let w = if x.abs() < 3.0 {
            sinc(x) * sinc(x / 3.0)
        } else {
            0.0
        };
        ws.push(w);
        sum += w;
    }
    for w in ws {
        *w /= sum;
    }
    left
}

pub fn resize(image: &RgbImage, width: u32, height: u32) -> RgbImage {
    assert!(image.width() > 0 && image.height() > 0 && width > 0 && height > 0);
    if image.dimensions() == (width, height) {
        return image.clone();
    }
    let horizontal: Vec<_> = (0..width)
        .map(|x| {
            let mut ws = Vec::new();
            let first = weights(image.width(), width, x, &mut ws);
            (first, ws)
        })
        .collect();
    let mut output = RgbImage::new(width, height);
    output
        .set_color_space(image.color_space())
        .expect("RGB source color space is valid for RGB output");
    let mut row = vec![[0.0f32; 3]; image.width() as usize];
    let mut ws = Vec::new();
    for y in 0..height {
        let first = weights(image.height(), height, y, &mut ws);
        for (x, pixel) in row.iter_mut().enumerate() {
            let mut t = [0.0; 3];
            for (i, w) in ws.iter().enumerate() {
                let p = image.get_pixel(x as u32, first + i as u32);
                for c in 0..3 {
                    t[c] += f32::from(p[c]) * w;
                }
            }
            *pixel = t;
        }
        for (x, (first, ws)) in horizontal.iter().enumerate() {
            let mut t = [0.0f32; 3];
            for (i, w) in ws.iter().enumerate() {
                let p = row[*first as usize + i];
                for c in 0..3 {
                    t[c] += p[c] * w;
                }
            }
            output.put_pixel(
                x as u32,
                y,
                Rgb(t.map(|v| v.clamp(0.0, 255.0).round() as u8)),
            );
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixels_match_image_crate_for_odd_edges_downscale_upscale_and_identity() {
        for (w, h) in [(1, 1), (1, 19), (17, 1), (17, 19), (127, 65), (256, 257)] {
            let source = RgbImage::from_fn(w, h, |x, y| {
                Rgb([
                    ((x * 37 + y * 19) % 256) as u8,
                    ((x * 91 + y * 7) % 256) as u8,
                    ((x + y) % 2 * 255) as u8,
                ])
            });
            for (dw, dh) in [(1, 1), (8, 9), (31, 33), (w, h), (w + 3, h + 7)] {
                let expected =
                    image::imageops::resize(&source, dw, dh, image::imageops::FilterType::Lanczos3);
                assert!(resize(&source, dw, dh) == expected, "{w}x{h} -> {dw}x{dh}");
            }
        }
    }
}
