#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use raster_core::{Progress, Report, Request};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, State};

static PREVIEW_CACHE: std::sync::LazyLock<Mutex<raster_core::PreviewCache>> =
    std::sync::LazyLock::new(|| Mutex::new(raster_core::PreviewCache::new(16 * 1024 * 1024)));

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImagePreview {
    data_url: String,
    width: u32,
    height: u32,
}

#[tauri::command]
async fn preview_image(path: String, max_edge: u32) -> Result<ImagePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Bound full-resolution decodes even if several UI requests arrive together.
        let mut cache = PREVIEW_CACHE.lock().map_err(|_| "미리보기 잠금 오류")?;
        let preview = cache.get(std::path::Path::new(&path), max_edge)?;
        Ok(ImagePreview {
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&preview.png)
            ),
            width: preview.width,
            height: preview.height,
        })
    })
    .await
    .map_err(|e| format!("미리보기 실행 오류: {e}"))?
}

#[derive(Default)]
struct Jobs(Mutex<Option<Arc<AtomicBool>>>);

#[derive(Serialize)]
struct PickedImage {
    path: String,
    name: String,
    bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobProgress {
    id: String,
    #[serde(flatten)]
    progress: Progress,
}

#[tauri::command]
async fn pick_images() -> Result<Vec<PickedImage>, String> {
    let selection = rfd::AsyncFileDialog::new()
        .set_title("최적화할 사진 선택")
        .add_filter("사진", &["jpg", "jpeg", "png", "webp"])
        .pick_files()
        .await;
    selection
        .unwrap_or_default()
        .into_iter()
        .map(|file| {
            let path = file.path();
            let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
            Ok(PickedImage {
                path: path.to_string_lossy().into_owned(),
                name: file.file_name(),
                bytes: metadata.len(),
            })
        })
        .collect()
}

#[tauri::command]
async fn pick_output_dir() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("결과를 저장할 폴더 선택")
        .pick_folder()
        .await
        .map(|p| p.path().to_string_lossy().into_owned())
}

#[tauri::command]
fn cancel_current(jobs: State<'_, Jobs>) -> Result<bool, String> {
    let active = jobs.0.lock().map_err(|_| "작업 상태 잠금 오류")?;
    if let Some(cancel) = active.as_ref() {
        cancel.store(true, Ordering::SeqCst);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[derive(Deserialize)]
struct BatchInput {
    id: String,
    request: Request,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobResult {
    id: String,
    report: Option<Report>,
    error: Option<String>,
}
#[derive(Serialize)]
struct BatchResponse {
    plan: raster_core::BatchPlan,
    results: Vec<JobResult>,
}

#[tauri::command]
async fn optimize_batch(
    app: tauri::AppHandle,
    jobs: State<'_, Jobs>,
    items: Vec<BatchInput>,
    concurrency: usize,
) -> Result<BatchResponse, String> {
    if concurrency > 4 {
        return Err("동시 처리는 자동 또는 1~4개를 선택하세요.".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = jobs.0.lock().map_err(|_| "작업 상태 잠금 오류")?;
        if active.is_some() {
            return Err("이미 처리 중인 작업이 있습니다.".into());
        }
        *active = Some(cancel.clone());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let requests: Vec<_> = items.iter().map(|i| i.request.clone()).collect();
        let plan = raster_core::plan_batch(&requests, concurrency)?;
        let _ = app.emit("batch-plan", &plan);
        let make_result = |index: usize, result: &Result<Report, String>| JobResult {
            id: items[index].id.clone(),
            report: result.as_ref().ok().cloned(),
            error: result.as_ref().err().cloned(),
        };
        let results = raster_core::optimize_batch(
            &requests,
            plan.jobs,
            &cancel,
            |index, progress| {
                let _ = app.emit(
                    "optimizer-progress",
                    JobProgress {
                        id: items[index].id.clone(),
                        progress,
                    },
                );
            },
            |index, result| {
                let _ = app.emit("optimizer-result", make_result(index, result));
            },
        )?;
        Ok(BatchResponse {
            plan,
            results: results
                .iter()
                .map(|(index, r)| make_result(*index, r))
                .collect(),
        })
    })
    .await
    .map_err(|e| format!("작업 실행 오류: {e}"));
    *jobs.0.lock().map_err(|_| "작업 상태 잠금 오류")? = None;
    result?
}

fn main() {
    tauri::Builder::default()
        .manage(Jobs::default())
        .invoke_handler(tauri::generate_handler![
            pick_images,
            pick_output_dir,
            optimize_batch,
            cancel_current,
            preview_image
        ])
        .run(tauri::generate_context!())
        .expect("rsquish 실행 실패");
}
