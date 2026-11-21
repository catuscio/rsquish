//! Shared, bounded image optimizer. No subprocesses, uploads or source writes.
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage};
use raster_codecs::Codec;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssimulacra2::{ColorPrimaries, LinearRgb, Rgb, TransferCharacteristic};
mod batch;
pub use batch::{BatchPlan, BatchResult, optimize_batch, plan_batch};
mod preview_cache;
mod row_resize;
pub use preview_cache::{PreviewCache, PreviewImage};
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub const CODECS: &[&str] = &["mozjpeg", "webp", "avif-iq", "avif-ssim"];
pub const METRIC: &str = "ssimulacra2-rust-0.5.1";
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PIXELS: u64 = 32_000_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Target,
    Budget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub input_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: Mode,
    pub target_score: f64,
    pub max_bytes: u64,
    pub max_width: Option<u32>,
    pub codecs: Vec<String>,
    #[serde(default = "default_probes")]
    pub probes: u8,
    #[serde(default = "default_seconds")]
    pub max_seconds: u64,
}
fn default_probes() -> u8 {
    7
}
fn default_seconds() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub stage: String,
    pub codec: Option<String>,
    pub completed: usize,
    pub total: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub codec: String,
    pub quality: u8,
    pub bytes: u64,
    /// None when the byte budget rules this candidate out before evaluation.
    pub score: Option<f64>,
    pub evaluation_skipped: bool,
    pub sha256: String,
    pub encode_ms: u64,
    pub evaluation_ms: u64,
    pub metric_cache_hit: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub input_path: String,
    pub output_path: Option<String>,
    pub status: String,
    pub input_bytes: u64,
    pub output_bytes: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub score: Option<f64>,
    pub codec: Option<String>,
    pub quality: Option<u8>,
    pub elapsed_ms: u64,
    pub encode_count: usize,
    pub metric_cache_hits: usize,
    pub warnings: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub metric: String,
    pub input_sha256: String,
    pub request: Request,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn millis(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}
fn codec(id: &str) -> Result<Codec, String> {
    match id {
        "mozjpeg" => Ok(Codec::Mozjpeg),
        "webp" => Ok(Codec::Webp),
        "avif-iq" => Ok(Codec::AvifIq),
        "avif-ssim" => Ok(Codec::AvifSsim),
        _ => Err(format!("Unknown codec: {id}")),
    }
}
fn validate(r: &Request) -> Result<(), String> {
    if !(3..=32).contains(&r.probes) {
        return Err("probes must be 3..32".into());
    }
    if !(1..=3600).contains(&r.max_seconds) {
        return Err("maxSeconds must be 1..3600".into());
    }
    if !r.target_score.is_finite() || !(0.0..=100.0).contains(&r.target_score) {
        return Err("targetScore must be finite and 0..100".into());
    }
    if r.mode == Mode::Budget && r.max_bytes == 0 {
        return Err("maxBytes must be positive".into());
    }
    if r.max_width.is_some_and(|w| w < 8) {
        return Err("maxWidth must be at least 8".into());
    }
    if r.codecs.is_empty() || r.codecs.len() > CODECS.len() {
        return Err("Select 1..4 codecs".into());
    }
    for (i, id) in r.codecs.iter().enumerate() {
        codec(id)?;
        if r.codecs[..i].contains(id) {
            return Err(format!("Duplicate codec: {id}"));
        }
    }
    Ok(())
}

/// Parse container chunk boundaries, never substring-match compressed payloads.
fn has_chunk(bytes: &[u8], png: bool, wanted: &[u8; 4]) -> bool {
    let mut offset: usize = if png { 8 } else { 12 };
    while let Some(header) = bytes.get(offset..offset.saturating_add(8)) {
        let (tag, length) = if png {
            (
                &header[4..8],
                u32::from_be_bytes(header[..4].try_into().unwrap()) as usize,
            )
        } else {
            (
                &header[..4],
                u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize,
            )
        };
        if tag == wanted {
            return true;
        }
        let trailer = if png { 4 } else { length % 2 };
        let Some(next) = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .and_then(|n| n.checked_add(trailer))
        else {
            break;
        };
        offset = next;
    }
    false
}

struct Reference {
    rgb: RgbImage,
    input_bytes: u64,
    hash: String,
    warnings: Vec<String>,
}

fn jpeg_components(bytes: &[u8]) -> Option<u8> {
    let mut offset = 2usize;
    while offset < bytes.len() {
        if *bytes.get(offset)? != 0xff {
            return None;
        }
        while *bytes.get(offset)? == 0xff {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if matches!(marker, 0xda | 0xd9) {
            return None;
        }
        if matches!(marker, 0xd0..=0xd8 | 0x01) {
            continue;
        }
        let length = u16::from_be_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as usize;
        if length < 2 {
            return None;
        }
        if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            return bytes.get(offset + 7).copied();
        }
        offset = offset.checked_add(length)?;
    }
    None
}

fn load_reference(path: &Path, max_width: Option<u32>) -> Result<Reference, String> {
    let file = fs::File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > MAX_INPUT_BYTES {
        return Err("Input exceeds 256 MiB limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("Input exceeds 256 MiB limit".into());
    }
    let format = image::guess_format(&bytes).map_err(err)?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) {
        return Err("This version accepts JPEG, PNG and WebP input".into());
    }
    if format == ImageFormat::Jpeg && jpeg_components(&bytes) == Some(4) {
        return Err("CMYK/YCCK JPEG input requires a separate color-managed path".into());
    }
    if format == ImageFormat::Png && has_chunk(&bytes, true, b"cICP") {
        return Err("PNG cICP/HDR color signaling is not supported in this version".into());
    }
    if (format == ImageFormat::Png && has_chunk(&bytes, true, b"acTL"))
        || (format == ImageFormat::WebP && has_chunk(&bytes, false, b"ANIM"))
    {
        return Err("Animated input is not supported; no frames were discarded".into());
    }
    let unsupported_png_color = format == ImageFormat::Png
        && !has_chunk(&bytes, true, b"sRGB")
        && [*b"gAMA", *b"cHRM", *b"cICP"]
            .iter()
            .any(|tag| has_chunk(&bytes, true, tag));
    let hash = digest(&bytes);
    let input_bytes = bytes.len() as u64;
    let reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut decoder = reader.into_decoder().map_err(err)?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("Input exceeds 32 megapixel limit".into());
    }
    if decoder.color_type().bits_per_pixel() / u16::from(decoder.color_type().channel_count()) > 8 {
        return Err("16-bit/HDR input is not supported; no precision was discarded".into());
    }
    let profile = decoder.icc_profile().map_err(err)?;
    if profile.is_none() && unsupported_png_color {
        return Err(
            "PNG color metadata needs explicit conversion; use an RGB ICC or sRGB-tagged input"
                .into(),
        );
    }
    let orientation = decoder.orientation().map_err(err)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(err)?;
    image.apply_orientation(orientation);
    if has_transparency(&image) {
        return Err("Transparent input requires the future alpha-preserving path".into());
    }
    let mut rgb = image.into_rgb8();
    let mut warnings = vec![
        "SSIMULACRA2 is an operational score, not a visually lossless guarantee.".into(),
        "Output is normalized to sRGB; EXIF/GPS and original metadata are removed.".into(),
    ];
    if let Some(icc) = profile {
        let source = lcms2::Profile::new_icc(&icc).map_err(err)?;
        if source.color_space() != lcms2::ColorSpaceSignature::RgbData {
            return Err("Only RGB ICC profiles are supported in this version".into());
        }
        let target = lcms2::Profile::new_srgb();
        let transform: lcms2::Transform<u8, u8> = lcms2::Transform::new(
            &source,
            lcms2::PixelFormat::RGB_8,
            &target,
            lcms2::PixelFormat::RGB_8,
            lcms2::Intent::RelativeColorimetric,
        )
        .map_err(err)?;
        let (width, height) = rgb.dimensions();
        let mut pixels = rgb.into_raw();
        transform.transform_in_place(&mut pixels);
        rgb = RgbImage::from_raw(width, height, pixels).ok_or("Invalid ICC result")?;
    } else {
        warnings.push("Untagged input is interpreted as sRGB.".into());
    }
    if let Some(max) = max_width.filter(|m| *m < rgb.width()) {
        let height = ((u64::from(rgb.height()) * u64::from(max) + u64::from(rgb.width()) / 2)
            / u64::from(rgb.width()))
        .max(1) as u32;
        rgb = row_resize::resize(&rgb, max, height);
        warnings.push(
            "Quality is measured against the resized reference, not the full-resolution input."
                .into(),
        );
    }
    if rgb.width() < 8 || rgb.height() < 8 {
        return Err("Metric requires dimensions of at least 8 × 8".into());
    }
    Ok(Reference {
        rgb,
        input_bytes,
        hash,
        warnings,
    })
}

fn has_transparency(image: &DynamicImage) -> bool {
    match image {
        DynamicImage::ImageRgba8(pixels) => pixels.pixels().any(|p| p[3] != 255),
        DynamicImage::ImageLumaA8(pixels) => pixels.pixels().any(|p| p[1] != 255),
        // Non-8-bit inputs are rejected before this helper; future alpha formats
        // must opt into an explicit borrowed scan rather than silent flattening.
        _ => image.has_alpha(),
    }
}

fn linear(rgb: &RgbImage) -> Result<LinearRgb, String> {
    let pixels = rgb
        .pixels()
        .map(|p| {
            [
                f32::from(p[0]) / 255.0,
                f32::from(p[1]) / 255.0,
                f32::from(p[2]) / 255.0,
            ]
        })
        .collect();
    let rgb = Rgb::new(
        pixels,
        rgb.width() as usize,
        rgb.height() as usize,
        TransferCharacteristic::SRGB,
        ColorPrimaries::BT709,
    )
    .map_err(err)?;
    LinearRgb::try_from(rgb).map_err(err)
}

/// Measure two normalized images with the same metric used by the optimizer.
pub fn score_rgb(reference: &RgbImage, distorted: &RgbImage) -> Result<f64, String> {
    ssimulacra2::compute_frame_ssimulacra2(linear(reference)?, linear(distorted)?).map_err(err)
}

/// Display-only PNG preview. Never creates files or recompresses the source.
/// The dimensions describe the full oriented image, not the reduced preview.
pub fn preview_png(path: &Path, max_edge: u32) -> Result<(Vec<u8>, u32, u32), String> {
    if !(64..=1600).contains(&max_edge) {
        return Err("Preview size must be 64..1600 pixels".into());
    }
    let rgb = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("avif"))
    {
        let file = fs::File::open(path).map_err(err)?;
        if file.metadata().map_err(err)?.len() > MAX_INPUT_BYTES {
            return Err("Input exceeds 256 MiB limit".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        let (pixels, width, height) = raster_codecs::decode_avif(&bytes)?;
        RgbImage::from_raw(width, height, pixels).ok_or("Invalid AVIF preview")?
    } else {
        load_reference(path, None)?.rgb
    };
    let (width, height) = rgb.dimensions();
    let preview = DynamicImage::ImageRgb8(rgb).thumbnail(max_edge, max_edge);
    let mut bytes = Cursor::new(Vec::new());
    preview
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(err)?;
    Ok((bytes.into_inner(), width, height))
}

fn feasible(c: &Candidate, r: &Request) -> bool {
    match r.mode {
        Mode::Target => c.score.is_some_and(|score| score >= r.target_score),
        Mode::Budget => c.bytes <= r.max_bytes,
    }
}
fn better(a: &Candidate, b: &Candidate, mode: Mode) -> bool {
    match mode {
        Mode::Target => a.bytes < b.bytes || (a.bytes == b.bytes && a.score > b.score),
        Mode::Budget => a.score > b.score || (a.score == b.score && a.bytes < b.bytes),
    }
}

fn propose(seen: &[Candidate], r: &Request) -> Option<u8> {
    let measured = |q| seen.iter().any(|c| c.quality == q);
    let mut ordered: Vec<_> = seen.iter().collect();
    ordered.sort_by_key(|c| c.quality);
    let winner = seen
        .iter()
        .filter(|c| feasible(c, r))
        .reduce(|a, b| if better(b, a, r.mode) { b } else { a });
    let boundary = if r.mode == Mode::Target { 100 } else { 1 };
    if winner.is_none() && !measured(boundary) {
        return Some(boundary);
    }
    if seen.iter().all(|c| feasible(c, r)) {
        let boundary = if r.mode == Mode::Target { 1 } else { 100 };
        if !measured(boundary) {
            return Some(boundary);
        }
    }
    // The bracket is a proposal heuristic, not a monotonicity assumption in selection.
    let brackets: Vec<_> = ordered
        .windows(2)
        .filter(|p| {
            let crossing = match r.mode {
                Mode::Target => !feasible(p[0], r) && feasible(p[1], r),
                Mode::Budget => feasible(p[0], r) && !feasible(p[1], r),
            };
            crossing && p[1].quality - p[0].quality > 1
        })
        .collect();
    if let Some(p) = brackets.first() {
        return Some(p[0].quality + (p[1].quality - p[0].quality) / 2);
    }
    if let Some(winner) = winner {
        for d in 1..100i16 {
            for sign in if r.mode == Mode::Target {
                [-1, 1]
            } else {
                [1, -1]
            } {
                let q = i16::from(winner.quality) + sign * d;
                if (1..=100).contains(&q) && !measured(q as u8) {
                    return Some(q as u8);
                }
            }
        }
    }
    None
}

fn save_unique(dir: &Path, stem: &str, extension: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(err)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(err)?;
    temp.write_all(bytes).map_err(err)?;
    temp.as_file().sync_all().map_err(err)?;
    for i in 0..1000 {
        let suffix = if i == 0 {
            String::new()
        } else {
            format!("-{i}")
        };
        let path = dir.join(format!("{stem}-optimized{suffix}.{extension}"));
        match temp.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temp = e.file,
            Err(e) => return Err(err(e.error)),
        }
    }
    Err("Too many existing output names".into())
}

pub fn optimize(
    r: &Request,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<Report, String> {
    validate(r)?;
    let started = Instant::now();
    let total = r.codecs.len() * usize::from(r.probes);
    progress(Progress {
        stage: "prepare".into(),
        codec: None,
        completed: 0,
        total,
        message: "Preparing reference pixels".into(),
    });
    let reference = load_reference(&r.input_path, r.max_width)?;
    let (width, height) = reference.rgb.dimensions();
    let mut report = Report {
        input_path: r.input_path.to_string_lossy().into(),
        output_path: None,
        status: "constraint_unmet".into(),
        input_bytes: reference.input_bytes,
        output_bytes: None,
        width,
        height,
        score: None,
        codec: None,
        quality: None,
        elapsed_ms: 0,
        encode_count: 0,
        metric_cache_hits: 0,
        warnings: reference.warnings,
        candidates: vec![],
        metric: METRIC.into(),
        input_sha256: reference.hash,
        request: r.clone(),
    };
    // Prepare lazily, cap cached references at 4 MP, and avoid work for oversized candidates.
    let mut prepared_reference: Option<ssimulacra2::PreparedReference> = None;
    let mut source_linear: Option<LinearRgb> = None;
    let mut metric_cache: HashMap<String, f64> = HashMap::new();
    let mut best: Option<(Candidate, Vec<u8>)> = None;
    let mut stopped = false;
    // One-slot queues bound prefetch; references above 4 MP stay serial to limit peak memory.
    let overlap = u64::from(width) * u64::from(height) <= 4_000_000;
    let actual_encodes = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| -> Result<(), String> {
        let (jobs, pending) = std::sync::mpsc::sync_channel::<(Codec, u8)>(1);
        let (results, encoded) = std::sync::mpsc::sync_channel(1);
        let pixels = reference.rgb.as_raw();
        let counter = &actual_encodes;
        scope.spawn(move || {
            let mut avif_source: Option<raster_codecs::AvifSource> = None;
            while let Ok((codec, q)) = pending.recv() {
                let tick = Instant::now();
                let bytes = (|| {
                    if matches!(codec, Codec::AvifIq | Codec::AvifSsim) {
                        if avif_source.is_none() {
                            avif_source =
                                Some(raster_codecs::AvifSource::new(pixels, width, height)?);
                        }
                        avif_source.as_mut().unwrap().encode(codec, q)
                    } else {
                        raster_codecs::encode(codec, pixels, width, height, q)
                    }
                })();
                counter.fetch_add(1, Ordering::Relaxed);
                if results.send((bytes, millis(tick))).is_err() {
                    break;
                }
            }
        });
        // Round-robin proposals depend only on the codec's previous rounds. Preparing
        // this round's jobs before evaluation therefore preserves the search sequence.
        for step in 0..r.probes {
            let plan: Vec<_> = r
                .codecs
                .iter()
                .filter_map(|id| {
                    let own: Vec<_> = report
                        .candidates
                        .iter()
                        .filter(|c| &c.codec == id)
                        .cloned()
                        .collect();
                    let q = if step < 3 {
                        [50, 75, 95][usize::from(step)]
                    } else {
                        propose(&own, r)?
                    };
                    Some((id, q))
                })
                .collect();
            for (index, &(id, q)) in plan.iter().enumerate() {
                if cancelled.load(Ordering::Relaxed) {
                    report.status = "cancelled".into();
                    stopped = true;
                    break;
                }
                if started.elapsed().as_secs() >= r.max_seconds {
                    report.status = "time_limit".into();
                    stopped = true;
                    break;
                }
                progress(Progress {
                    stage: "encode".into(),
                    codec: Some(id.clone()),
                    completed: report.encode_count,
                    total,
                    message: format!("{id} · q{q}"),
                });
                if index == 0 || !overlap {
                    jobs.send((codec(id)?, q))
                        .map_err(|_| "Encoding worker stopped")?;
                }
                let (bytes, encode_ms) = encoded.recv().map_err(|_| "Encoding worker stopped")?;
                let bytes = bytes?;
                report.encode_count += 1;
                if cancelled.load(Ordering::Relaxed) {
                    report.status = "cancelled".into();
                    stopped = true;
                    break;
                }
                if overlap
                    && started.elapsed().as_secs() < r.max_seconds
                    && let Some(&(next_id, next_q)) = plan.get(index + 1)
                {
                    jobs.send((codec(next_id)?, next_q))
                        .map_err(|_| "Encoding worker stopped")?;
                }
                let hash = digest(&bytes);
                let tick = Instant::now();
                let cached = metric_cache.get(&hash).copied();
                // Byte length is an exact feasibility test. An oversized budget candidate
                // is still retained for bracketing, but cannot win regardless of its score.
                let evaluation_skipped = r.mode == Mode::Budget && bytes.len() as u64 > r.max_bytes;
                let score = if evaluation_skipped {
                    None
                } else if let Some(score) = cached {
                    report.metric_cache_hits += 1;
                    Some(score)
                } else {
                    progress(Progress {
                        stage: "evaluate".into(),
                        codec: Some(id.clone()),
                        completed: report.encode_count,
                        total,
                        message: format!("Evaluating {id} · q{q}"),
                    });
                    let rgb = if id.starts_with("avif") {
                        let (pixels, w, h) = raster_codecs::decode_avif(&bytes)?;
                        RgbImage::from_raw(w, h, pixels).ok_or("Invalid decoded AVIF buffer")?
                    } else {
                        image::load_from_memory(&bytes).map_err(err)?.into_rgb8()
                    };
                    if rgb.dimensions() != (width, height) {
                        return Err("Encoder changed image dimensions".into());
                    }
                    let score = if u64::from(width) * u64::from(height) <= 4_000_000 {
                        if prepared_reference.is_none() {
                            prepared_reference = Some(
                                ssimulacra2::PreparedReference::new(linear(&reference.rgb)?)
                                    .map_err(err)?,
                            );
                        }
                        prepared_reference
                            .as_mut()
                            .unwrap()
                            .score(linear(&rgb)?)
                            .map_err(err)?
                    } else {
                        if source_linear.is_none() {
                            source_linear = Some(linear(&reference.rgb)?);
                        }
                        ssimulacra2::compute_frame_ssimulacra2(
                            source_linear.as_ref().unwrap().clone(),
                            linear(&rgb)?,
                        )
                        .map_err(err)?
                    };
                    if !score.is_finite() {
                        return Err("Metric returned non-finite score".into());
                    }
                    metric_cache.insert(hash.clone(), score);
                    Some(score)
                };
                let candidate = Candidate {
                    codec: id.clone(),
                    quality: q,
                    bytes: bytes.len() as u64,
                    score,
                    evaluation_skipped,
                    sha256: hash,
                    encode_ms,
                    evaluation_ms: millis(tick),
                    metric_cache_hit: !evaluation_skipped && cached.is_some(),
                };
                if feasible(&candidate, r)
                    && best
                        .as_ref()
                        .is_none_or(|(b, _)| better(&candidate, b, r.mode))
                {
                    best = Some((candidate.clone(), bytes));
                }
                report.candidates.push(candidate);
            }
            if stopped {
                break;
            }
        }
        Ok(())
    })?;
    // Includes any prefetched encode discarded on cancellation/deadline.
    report.encode_count = actual_encodes.load(Ordering::Relaxed);
    // Cancellation never publishes an output; deadline may publish only a measured feasible candidate.
    if started.elapsed().as_secs() >= r.max_seconds {
        report.status = "time_limit".into();
    }
    if cancelled.load(Ordering::Relaxed) {
        report.status = "cancelled".into();
    }
    if report.status != "cancelled"
        && let Some((candidate, bytes)) = best
    {
        if report.status == "time_limit" {
            report.warnings.push("Time budget reached; saved the best measured feasible candidate. A running codec/metric finishes before checking the deadline.".into());
        }
        let extension = match candidate.codec.as_str() {
            "mozjpeg" => "jpg",
            "webp" => "webp",
            _ => "avif",
        };
        let stem = r
            .input_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let output = save_unique(&r.output_dir, &stem, extension, &bytes)?;
        report.output_path = Some(output.to_string_lossy().into());
        report.output_bytes = Some(candidate.bytes);
        report.score = candidate.score;
        report.codec = Some(candidate.codec);
        report.quality = Some(candidate.quality);
        report.status = "saved".into();
        if candidate.bytes > report.input_bytes {
            report.warnings.push("Selected output is larger than the input; the chosen quality constraint was prioritized.".into());
        }
    }
    report.elapsed_ms = millis(started);
    if let Some(path) = &report.output_path {
        let sidecar = PathBuf::from(format!("{path}.json"));
        let persist = (|| -> Result<(), String> {
            let mut temporary = tempfile::NamedTempFile::new_in(&r.output_dir).map_err(err)?;
            serde_json::to_writer_pretty(&mut temporary, &report).map_err(err)?;
            temporary.as_file().sync_all().map_err(err)?;
            temporary.persist_noclobber(sidecar).map_err(err)?;
            Ok(())
        })();
        if let Err(e) = persist {
            report.warnings.push(format!(
                "Image saved, but report sidecar could not be saved: {e}"
            ));
        }
    }
    progress(Progress {
        stage: "done".into(),
        codec: report.codec.clone(),
        completed: report.encode_count,
        total,
        message: report.status.clone(),
    });
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_alpha_scan_matches_old_rgba_conversion() {
        for alpha in 0..=255 {
            for image in [
                DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                    19,
                    17,
                    image::Rgba([4, 120, 255, alpha]),
                )),
                DynamicImage::ImageLumaA8(image::GrayAlphaImage::from_pixel(
                    17,
                    19,
                    image::LumaA([120, alpha]),
                )),
                DynamicImage::ImageRgb8(RgbImage::from_pixel(17, 19, image::Rgb([4, 120, 255]))),
            ] {
                assert_eq!(
                    has_transparency(&image),
                    image.has_alpha() && image.to_rgba8().pixels().any(|p| p[3] != 255)
                );
            }
        }
    }
    #[test]
    fn in_place_icc_matches_separate_output() {
        let source = lcms2::Profile::new_srgb();
        let target = lcms2::Profile::new_srgb();
        let transform: lcms2::Transform<u8, u8> = lcms2::Transform::new(
            &source,
            lcms2::PixelFormat::RGB_8,
            &target,
            lcms2::PixelFormat::RGB_8,
            lcms2::Intent::RelativeColorimetric,
        )
        .unwrap();
        let mut pixels: Vec<u8> = (0..300_000)
            .map(|i| ((i * 37 + i / 7) % 256) as u8)
            .collect();
        let mut expected = vec![0; pixels.len()];
        transform.transform_pixels(&pixels, &mut expected);
        let address = pixels.as_ptr();
        transform.transform_in_place(&mut pixels);
        assert_eq!(pixels.as_ptr(), address);
        assert_eq!(pixels, expected);
    }
    #[test]
    fn pruning_preserves_budget_proposals_even_with_nonmonotonic_sizes_and_scores() {
        let mut r = request("unused".into(), "unused".into());
        r.max_bytes = 100;
        // Cover all feasible/infeasible patterns, including multiple crossings.
        for mask in 0..32 {
            let measured: Vec<_> = [1, 25, 50, 75, 100]
                .into_iter()
                .enumerate()
                .map(|(i, quality)| Candidate {
                    codec: "webp".into(),
                    quality,
                    bytes: if mask & (1 << i) == 0 { 90 } else { 110 },
                    score: Some([10., 80., -5., 70., 50.][i]),
                    evaluation_skipped: false,
                    sha256: String::new(),
                    encode_ms: 0,
                    evaluation_ms: 0,
                    metric_cache_hit: false,
                })
                .collect();
            let mut pruned = measured.clone();
            for c in &mut pruned {
                if c.bytes > r.max_bytes {
                    c.score = None;
                    c.evaluation_skipped = true;
                }
            }
            assert_eq!(propose(&measured, &r), propose(&pruned, &r));
        }
    }
    #[test]
    fn previews_read_original_and_avif_without_writing_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let rgb = RgbImage::from_pixel(128, 96, image::Rgb([120, 80, 30]));
        let source = dir.path().join("source.png");
        rgb.save(&source).unwrap();
        let before = fs::read(&source).unwrap();
        let (png, w, h) = preview_png(&source, 64).unwrap();
        assert_eq!((w, h), (128, 96));
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 64);
        let avif = dir.path().join("result.avif");
        fs::write(
            &avif,
            raster_codecs::encode(Codec::AvifIq, rgb.as_raw(), 128, 96, 80).unwrap(),
        )
        .unwrap();
        let (png, w, h) = preview_png(&avif, 64).unwrap();
        assert_eq!((w, h), (128, 96));
        assert_eq!(image::load_from_memory(&png).unwrap().height(), 48);
        assert_eq!(before, fs::read(source).unwrap());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    fn request(input: PathBuf, output: PathBuf) -> Request {
        Request {
            input_path: input,
            output_dir: output,
            mode: Mode::Budget,
            target_score: 85.,
            max_bytes: 1,
            max_width: None,
            codecs: vec!["webp".into()],
            probes: 3,
            max_seconds: 60,
        }
    }
    #[test]
    fn impossible_budget_does_not_write_or_change_source() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.png");
        RgbImage::from_pixel(32, 32, image::Rgb([100, 120, 140]))
            .save(&input)
            .unwrap();
        let before = fs::read(&input).unwrap();
        let output = dir.path().join("out");
        let report = optimize(
            &request(input.clone(), output.clone()),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(report.status, "constraint_unmet");
        assert!(
            report
                .candidates
                .iter()
                .all(|c| c.evaluation_skipped && c.score.is_none())
        );
        assert!(report.output_path.is_none());
        assert!(!output.exists());
        assert_eq!(before, fs::read(input).unwrap());
    }
    #[test]
    fn atomic_save_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let a = save_unique(dir.path(), "photo", "webp", b"a").unwrap();
        let b = save_unique(dir.path(), "photo", "webp", b"b").unwrap();
        assert_ne!(a, b);
        assert_eq!(fs::read(a).unwrap(), b"a");
        assert_eq!(fs::read(b).unwrap(), b"b");
    }
    #[test]
    fn invalid_request_is_rejected_before_io() {
        let r = request("missing".into(), "unused".into());
        let mut r = r;
        r.target_score = f64::NAN;
        assert!(validate(&r).is_err());
    }
    #[test]
    fn identical_pixels_score_100() {
        let rgb = RgbImage::from_pixel(32, 32, image::Rgb([10, 50, 90]));
        assert!((score_rgb(&rgb, &rgb).unwrap() - 100.).abs() < 0.0001);
    }
    #[test]
    fn payload_is_not_animation_header() {
        let mut data = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(b"ANIM");
        assert!(!has_chunk(&data, false, b"ANIM"));
    }
    #[test]
    fn cancellation_does_not_publish() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.png");
        RgbImage::from_pixel(32, 32, image::Rgb([100, 120, 140]))
            .save(&input)
            .unwrap();
        let mut r = request(input, dir.path().join("out"));
        r.max_bytes = 10000;
        let token = AtomicBool::new(false);
        let report = optimize(&r, &token, |p| {
            if p.stage == "encode" {
                token.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert_eq!(report.status, "cancelled");
        assert!(report.output_path.is_none());
        assert!(!r.output_dir.exists());
    }
    #[test]
    fn cancellation_during_evaluation_drains_prefetch_without_publishing() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.png");
        RgbImage::from_pixel(32, 32, image::Rgb([100, 120, 140]))
            .save(&input)
            .unwrap();
        let mut r = request(input, dir.path().join("out"));
        r.max_bytes = 10000;
        r.codecs = vec!["webp".into(), "mozjpeg".into()];
        let token = AtomicBool::new(false);
        let report = optimize(&r, &token, |p| {
            if p.stage == "evaluate" {
                token.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert_eq!(report.status, "cancelled");
        assert_eq!(report.encode_count, 2);
        assert!(report.output_path.is_none());
        assert!(!r.output_dir.exists());
    }
    #[test]
    fn transparent_and_high_depth_inputs_are_not_flattened() {
        let dir = tempfile::tempdir().unwrap();
        let transparent = dir.path().join("alpha.png");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 128]))
            .save(&transparent)
            .unwrap();
        assert!(
            load_reference(&transparent, None)
                .err()
                .unwrap()
                .contains("Transparent")
        );
        let deep = dir.path().join("16.png");
        image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_pixel(16, 16, image::Luma([1000]))
            .save(&deep)
            .unwrap();
        assert!(
            load_reference(&deep, None)
                .err()
                .unwrap()
                .contains("16-bit")
        );
    }
}
