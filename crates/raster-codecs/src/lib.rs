//! RGB8 codecs linked from static native archives. No subprocesses are used.
//!
//! Encoding is lossy 4:2:0; `quality = 100` does not promise RGB pixel identity.
//! Inputs must already be normalized to sRGB. Metadata is not carried through.

use std::ffi::{CStr, c_char, c_int, c_void};

/// AVIF/JPEG use one encoding thread; WebP enables its bounded native threading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Codec {
    Mozjpeg = 0,
    Webp = 1,
    AvifIq = 2,
    AvifSsim = 3,
}

pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 32768;

unsafe extern "C" {
    fn rk_avif_prepare(
        pixels: *const u8,
        len: usize,
        width: u32,
        height: u32,
        error: *mut c_char,
        error_len: usize,
    ) -> *mut c_void;
    fn rk_avif_source_free(source: *mut c_void);
    fn rk_avif_encode_prepared(
        source: *const c_void,
        codec: c_int,
        quality: c_int,
        output: *mut *mut u8,
        output_len: *mut usize,
        error: *mut c_char,
        error_len: usize,
    ) -> c_int;
    fn rk_encode(
        codec: c_int,
        pixels: *const u8,
        len: usize,
        width: u32,
        height: u32,
        quality: c_int,
        output: *mut *mut u8,
        output_len: *mut usize,
        error: *mut c_char,
        error_len: usize,
    ) -> c_int;
    fn rk_decode_avif(
        data: *const u8,
        len: usize,
        output: *mut *mut u8,
        output_len: *mut usize,
        width: *mut u32,
        height: *mut u32,
        error: *mut c_char,
        error_len: usize,
    ) -> c_int;
    fn rk_free(data: *mut u8);
}

/// Owned YUV420 source reused across AVIF probes. Never borrows the caller's RGB.
/// Kept on the owning optimization thread; no Send/Sync implementation.
pub struct AvifSource(std::ptr::NonNull<c_void>);
impl AvifSource {
    pub fn new(pixels: &[u8], width: u32, height: u32) -> Result<Self, String> {
        let mut error = [0 as c_char; 512];
        // Native code validates dimensions and byte length before reading pixels,
        // and copies converted pixels into its own avifImage allocation.
        let ptr = unsafe {
            rk_avif_prepare(
                pixels.as_ptr(),
                pixels.len(),
                width,
                height,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        std::ptr::NonNull::new(ptr)
            .map(Self)
            .ok_or_else(|| error_message(&error))
    }

    pub fn encode(&mut self, codec: Codec, quality: u8) -> Result<Vec<u8>, String> {
        if !matches!(codec, Codec::AvifIq | Codec::AvifSsim) || quality > 100 {
            return Err("invalid prepared AVIF codec or quality".into());
        }
        let mut output = std::ptr::null_mut();
        let mut len = 0;
        let mut error = [0 as c_char; 512];
        // The image remains owned by self; each call creates its own encoder.
        let success = unsafe {
            rk_avif_encode_prepared(
                self.0.as_ptr(),
                codec as c_int,
                quality.into(),
                &mut output,
                &mut len,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        let owned = Buffer(output);
        if success != 1 {
            return Err(error_message(&error));
        }
        copy_result(owned, len)
    }
}
impl Drop for AvifSource {
    fn drop(&mut self) {
        unsafe { rk_avif_source_free(self.0.as_ptr()) }
    }
}

struct Buffer(*mut u8);
impl Drop for Buffer {
    fn drop(&mut self) {
        // The wrapper always returns memory allocated with its matching malloc.
        unsafe { rk_free(self.0) }
    }
}

fn error_message(error: &[c_char; 512]) -> String {
    // Buffer is initialized to zero; C writes only bounded, terminated messages.
    unsafe { CStr::from_ptr(error.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

fn copy_result(buffer: Buffer, len: usize) -> Result<Vec<u8>, String> {
    if buffer.0.is_null() || len == 0 || len > isize::MAX as usize {
        return Err("native codec returned an invalid output buffer".into());
    }
    // C allocated at least len bytes and transferred ownership to Buffer.
    Ok(unsafe { std::slice::from_raw_parts(buffer.0, len) }.to_vec())
}

/// Encode packed RGB8 data. Width/height are positive, <=32768, and <=64 MP.
/// WebP has a narrower native dimension limit and reports it as an error.
pub fn encode(
    codec: Codec,
    pixels: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let count = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || count > MAX_PIXELS
        || count * 3 != pixels.len() as u64
    {
        return Err("invalid RGB8 buffer dimensions or length (maximum 64 MP)".into());
    }
    if quality > 100 {
        return Err("quality must be in 0..=100".into());
    }
    let mut output = std::ptr::null_mut();
    let mut len = 0;
    let mut error = [0 as c_char; 512];
    // All borrowed buffers remain valid for the call; C does not retain them.
    let success = unsafe {
        rk_encode(
            codec as c_int,
            pixels.as_ptr(),
            pixels.len(),
            width,
            height,
            quality.into(),
            &mut output,
            &mut len,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    let owned = Buffer(output);
    if success != 1 {
        return Err(error_message(&error));
    }
    copy_result(owned, len)
}

/// Decode a single opaque AVIF image into packed RGB8, using one libaom thread.
/// Rejects alpha, animations and container transforms to avoid silently losing
/// semantics. ICC/color conversion beyond AVIF's YUV-to-RGB is not performed.
pub fn decode_avif(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    if bytes.is_empty() || bytes.len() > 256 * 1024 * 1024 {
        return Err("AVIF input must contain 1..=256 MiB".into());
    }
    let mut output = std::ptr::null_mut();
    let mut len = 0;
    let (mut width, mut height) = (0, 0);
    let mut error = [0 as c_char; 512];
    let success = unsafe {
        rk_decode_avif(
            bytes.as_ptr(),
            bytes.len(),
            &mut output,
            &mut len,
            &mut width,
            &mut height,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    let owned = Buffer(output);
    if success != 1 {
        return Err(error_message(&error));
    }
    if u64::from(width) * u64::from(height) * 3 != len as u64 {
        return Err("AVIF decoder returned inconsistent dimensions".into());
    }
    Ok((copy_result(owned, len)?, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels() -> Vec<u8> {
        (0..19)
            .flat_map(|y| (0..17).flat_map(move |x| [x * 12, y * 10, 80]))
            .collect()
    }

    #[test]
    fn prepared_avif_is_identical_across_tunes_and_repeated_probes() {
        for (w, h) in [(17, 19), (256, 257)] {
            let pixels: Vec<_> = (0..w * h * 3)
                .map(|n| ((n * 37 + n / 11) % 256) as u8)
                .collect();
            let mut prepared = AvifSource::new(&pixels, w, h).unwrap();
            for q in [50, 75, 95, 1, 100, 50] {
                for codec in [Codec::AvifIq, Codec::AvifSsim] {
                    let expected = encode(codec, &pixels, w, h, q);
                    let actual = prepared.encode(codec, q);
                    // libaom IQ at q100 may reject its own lossless/chroma-deltaq
                    // combination. Preserve that error as well as successful bytes.
                    if q != 100 {
                        assert!(expected.is_ok(), "{expected:?}");
                    }
                    assert!(
                        actual == expected,
                        "prepared output differs: {w}x{h} {codec:?} q{q}"
                    );
                }
            }
            assert!(prepared.encode(Codec::Webp, 50).is_err());
            assert!(prepared.encode(Codec::AvifIq, 101).is_err());
        }
        assert!(AvifSource::new(&[0; 3], 2, 1).is_err());
    }

    #[test]
    fn native_encoders_and_avif_roundtrip_odd_dimensions() {
        let rgb = pixels();
        for codec in [Codec::Mozjpeg, Codec::Webp, Codec::AvifIq, Codec::AvifSsim] {
            let bytes = encode(codec, &rgb, 17, 19, 90).expect("native encode");
            assert!(bytes.len() > 20);
            match codec {
                Codec::Mozjpeg => assert_eq!(&bytes[..2], &[0xff, 0xd8]),
                Codec::Webp => assert_eq!(&bytes[..4], b"RIFF"),
                _ => {
                    let (decoded, width, height) = decode_avif(&bytes).expect("AVIF decode");
                    assert_eq!((width, height), (17, 19));
                    let mae: f64 = rgb
                        .iter()
                        .zip(&decoded)
                        .map(|(a, b)| f64::from(a.abs_diff(*b)))
                        .sum::<f64>()
                        / rgb.len() as f64;
                    assert!(mae < 20.0, "unexpected large RGB roundtrip error: {mae}");
                }
            }
        }
    }

    #[test]
    fn invalid_input_returns_errors() {
        assert!(encode(Codec::Mozjpeg, &[0; 3], 2, 1, 80).is_err());
        assert!(encode(Codec::Webp, &[0; 3], 1, 1, 101).is_err());
        assert!(decode_avif(b"not an avif").is_err());
        assert!(decode_avif(&[]).is_err());
    }
}
