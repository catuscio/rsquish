//! Bounded batch execution. Results retain input order; cancellation stops dispatch.
use crate::{Progress, Report, Request};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub type BatchResult = (usize, Result<Report, String>);

pub fn optimize_batch(
    requests: &[Request],
    jobs: usize,
    stop: &AtomicBool,
    progress: impl Fn(usize, Progress) + Sync,
    completed: impl Fn(usize, &Result<Report, String>) + Sync,
) -> Result<Vec<BatchResult>, String> {
    if !(1..=4).contains(&jobs) {
        return Err("Concurrent jobs must be 1..4".into());
    }
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(requests.len()));
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(requests.len()) {
            scope.spawn(|| {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(request) = requests.get(index) else {
                        break;
                    };
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let result = crate::optimize(request, stop, |p| progress(index, p));
                    completed(index, &result);
                    results.lock().unwrap().push((index, result));
                }
            });
        }
    });
    let mut results = results.into_inner().map_err(crate::err)?;
    results.sort_by_key(|(index, _)| *index);
    Ok(results)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPlan {
    pub jobs: usize,
    pub automatic: bool,
    pub estimated_job_bytes: u64,
    pub available_memory_bytes: Option<u64>,
}

/// A conservative admission estimate, not a hard allocation limit. Zero means auto.
pub fn plan_batch(requests: &[Request], requested: usize) -> Result<BatchPlan, String> {
    if requested > 4 {
        return Err("Concurrent jobs must be auto or 1..4".into());
    }
    let estimate = requests
        .iter()
        .map(|r| {
            let (w, h) = image::image_dimensions(&r.input_path).unwrap_or((8000, 4000));
            let pixels = u64::from(w).saturating_mul(u64::from(h)).min(32_000_000);
            // Use the smaller dimension so EXIF rotation cannot underestimate resize memory.
            let side = u64::from(w.min(h)).max(1);
            let width = u64::from(r.max_width.unwrap_or(u32::MAX)).min(side);
            let output = pixels.saturating_mul(width).saturating_mul(width) / side / side;
            pixels * 12 + output * 180 + 96 * 1024 * 1024
        })
        .max()
        .unwrap_or(1);
    let available = available_memory();
    let cpu = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let jobs = if requested == 0 {
        auto_jobs(cpu, available, estimate)
    } else {
        requested
    };
    Ok(BatchPlan {
        jobs: jobs.min(requests.len().max(1)),
        automatic: requested == 0,
        estimated_job_bytes: estimate,
        available_memory_bytes: available,
    })
}

fn auto_jobs(cpu: usize, available: Option<u64>, estimate: u64) -> usize {
    let memory_jobs = available
        .map(|bytes| (bytes / 2 / estimate.max(1)).min(4) as usize)
        .unwrap_or(1);
    (cpu / 4).clamp(1, 4).min(memory_jobs.max(1))
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn available_memory() -> Option<u64> {
    // Retain one host send right for process lifetime, rather than leaking one per batch.
    static HOST: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
    let host = *HOST.get_or_init(|| unsafe { libc::mach_host_self() });
    let mut stats = std::mem::MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: correctly sized, aligned writable VM stats and count, initialized on success.
    let status = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if status != 0 {
        return None;
    }
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    // Free pages only: do not assume compressed/inactive pages can be reclaimed cheaply.
    Some(u64::from(stats.free_count) * page_size as u64)
}
#[cfg(not(target_os = "macos"))]
fn available_memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(dir: &std::path::Path) -> Request {
        let input = dir.join("source.png");
        image::RgbImage::from_pixel(64, 64, image::Rgb([80, 100, 120]))
            .save(&input)
            .unwrap();
        Request {
            input_path: input,
            output_dir: dir.join("out"),
            mode: crate::Mode::Target,
            target_score: 0.,
            max_bytes: 200_000,
            max_width: None,
            codecs: vec!["webp".into()],
            probes: 3,
            max_seconds: 60,
        }
    }
    #[test]
    fn automatic_jobs_respect_cpu_memory_and_unknown_memory() {
        assert_eq!(auto_jobs(16, Some(16_000), 1_000), 4);
        assert_eq!(auto_jobs(16, Some(4_000), 1_000), 2);
        assert_eq!(auto_jobs(16, Some(1_000), 1_000), 1);
        assert_eq!(auto_jobs(8, Some(16_000), 1_000), 2);
        assert_eq!(auto_jobs(16, None, 1_000), 1);
    }
    #[test]
    fn failed_input_does_not_abort_other_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let valid = fixture(dir.path());
        let mut invalid = valid.clone();
        invalid.input_path = dir.path().join("missing.png");
        let results = optimize_batch(
            &[valid.clone(), invalid, valid],
            2,
            &AtomicBool::new(false),
            |_, _| {},
            |_, _| {},
        )
        .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_ok());
        assert!(results[1].1.is_err());
        assert!(results[2].1.is_ok());
    }
    #[test]
    fn concurrent_same_names_preserve_bytes_order_and_unique_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let r = fixture(dir.path());
        let original = std::fs::read(&r.input_path).unwrap();
        let stop = AtomicBool::new(false);
        let reference = crate::optimize(&r, &stop, |_| {}).unwrap();
        let expected = std::fs::read(reference.output_path.unwrap()).unwrap();
        let results = optimize_batch(&vec![r.clone(); 4], 4, &stop, |_, _| {}, |_, _| {}).unwrap();
        let mut names = std::collections::HashSet::new();
        for (expected_index, (index, result)) in results.into_iter().enumerate() {
            assert_eq!(index, expected_index);
            let report = result.unwrap();
            assert_eq!(report.status, "saved");
            let path = report.output_path.unwrap();
            assert!(names.insert(path.clone()));
            assert_eq!(expected, std::fs::read(path).unwrap());
        }
        assert_eq!(original, std::fs::read(r.input_path).unwrap());
    }
    #[test]
    fn cancel_all_running_workers_stops_dispatch_and_publishing() {
        let dir = tempfile::tempdir().unwrap();
        let r = fixture(dir.path());
        let stop = AtomicBool::new(false);
        let barrier = std::sync::Barrier::new(2);
        let started = AtomicUsize::new(0);
        let results = optimize_batch(
            &vec![r.clone(); 8],
            2,
            &stop,
            |_, p| {
                if p.stage == "prepare" {
                    started.fetch_add(1, Ordering::SeqCst);
                    barrier.wait();
                    stop.store(true, Ordering::SeqCst);
                }
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(results.len(), 2);
        assert!(
            results
                .into_iter()
                .all(|(_, r)| r.unwrap().status == "cancelled")
        );
        assert!(
            !r.output_dir.exists() || std::fs::read_dir(r.output_dir).unwrap().next().is_none()
        );
    }
    #[test]
    fn batch_validation_precancel_and_errors_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = fixture(dir.path());
        let stop = AtomicBool::new(true);
        assert!(
            optimize_batch(&[r.clone()], 1, &stop, |_, _| panic!(), |_, _| panic!())
                .unwrap()
                .is_empty()
        );
        assert!(optimize_batch(&[], 5, &stop, |_, _| {}, |_, _| {}).is_err());
        assert!(plan_batch(&[], 5).is_err());
        assert_eq!(plan_batch(&[r.clone()], 4).unwrap().jobs, 1);
        r.probes = 0;
        let results = optimize_batch(
            &vec![r; 5],
            2,
            &AtomicBool::new(false),
            |_, _| {},
            |_, _| {},
        )
        .unwrap();
        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|(_, r)| r.is_err()));
    }
}
