// Tauri command 处理器
//
// export_batch: 前端"批量导出"入口。
//   - 参数：输入文件列表、输出目录、水印 PNG 路径、水印配置
//   - 内部：rayon 并行处理（spawn_blocking 避免阻塞 async runtime）
//   - 进度：通过 Tauri 事件 "batch-progress" 实时推送
//   - 返回：BatchSummary 汇总（成功/失败数、失败明细）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

use crate::batch::{self, BatchInput, ItemResult};
use crate::error::{Result, WatermarkError};
use crate::position::WatermarkConfig;
use crate::preset::{self, Preset};

#[tauri::command]
pub fn ping() -> String {
    "pong from Rust".to_string()
}

#[derive(Debug, Serialize, Clone)]
pub struct BatchProgress {
    pub done: usize,
    pub total: usize,
    pub filename: String,
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
pub struct ExportBatchArgs {
    pub input_paths: Vec<String>,
    pub output_dir: String,
    pub watermark_path: String,
    pub config: WatermarkConfig,
}

#[derive(Debug, Serialize)]
pub struct BatchSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub items: Vec<ItemResult>,
}

#[tauri::command]
pub async fn export_batch(app: AppHandle, args: ExportBatchArgs) -> Result<BatchSummary> {
    // 参数基本校验
    if args.input_paths.is_empty() {
        return Err(WatermarkError::InvalidParam(
            "输入照片列表为空".to_string(),
        ));
    }
    args.config.validate()?;

    // 预读水印 PNG（一次 IO，共享给所有 worker）
    let wm_bytes = std::fs::read(&args.watermark_path)?;

    let input_paths: Vec<PathBuf> = args.input_paths.into_iter().map(PathBuf::from).collect();
    let output_dir = PathBuf::from(&args.output_dir);

    let task = BatchInput {
        input_paths,
        output_dir,
        watermark_bytes: wm_bytes,
        config: args.config,
    };

    // 在阻塞线程池上跑（rayon 会 saturate CPU，避免占用 tauri async runtime 线程）
    let app_handle = app.clone();
    let results = tauri::async_runtime::spawn_blocking(move || {
        batch::run(&task, move |done, total, name, ok| {
            let _ = app_handle.emit(
                "batch-progress",
                BatchProgress {
                    done,
                    total,
                    filename: name.to_string(),
                    ok,
                },
            );
        })
    })
    .await
    .map_err(|e| WatermarkError::InvalidParam(format!("批量任务执行失败: {e}")))?;

    let success = results.iter().filter(|r| r.error.is_none()).count();
    let failed = results.len() - success;
    Ok(BatchSummary {
        total: results.len(),
        success,
        failed,
        items: results,
    })
}

// —— 预设管理 ————————————————————————————————————————————

fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_config_dir()
        .map_err(|e| WatermarkError::InvalidParam(format!("无法定位配置目录: {e}")))
}

#[tauri::command]
pub fn list_presets(app: AppHandle) -> Result<Vec<Preset>> {
    preset::load_all(&config_dir(&app)?)
}

#[tauri::command]
pub fn save_preset(app: AppHandle, preset: Preset) -> Result<Vec<Preset>> {
    if preset.name.trim().is_empty() {
        return Err(WatermarkError::InvalidParam("预设名称不能为空".to_string()));
    }
    preset.config.validate()?;
    preset::upsert(&config_dir(&app)?, preset)
}

#[tauri::command]
pub fn delete_preset(app: AppHandle, name: String) -> Result<Vec<Preset>> {
    preset::delete(&config_dir(&app)?, &name)
}

// —— 缩略图 ————————————————————————————————————————————
// 用于左栏文件列表：避免用原图渲染 40x40 缩略图导致 24MP 富士 JPEG 全部载入内存

#[tauri::command]
pub async fn create_thumbnail(path: String, max_size: u32) -> Result<Vec<u8>> {
    tauri::async_runtime::spawn_blocking(move || create_thumbnail_impl(&path, max_size))
        .await
        .map_err(|e| WatermarkError::InvalidParam(format!("缩略图任务失败: {e}")))?
}

fn create_thumbnail_impl(path: &str, max_size: u32) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;

    let img = image::open(path)?;
    let (w, h) = (img.width(), img.height());
    let long_side = w.max(h) as f32;
    let scale = (max_size as f32 / long_side).min(1.0);
    let tw = ((w as f32 * scale).round() as u32).max(1);
    let th = ((h as f32 * scale).round() as u32).max(1);

    // Triangle 滤波：足够快、质量对缩略图够用（Lanczos3 生成 200px 缩略图属于过度）
    let small = img
        .resize(tw, th, image::imageops::FilterType::Triangle)
        .to_rgb8();

    let mut buf = Vec::with_capacity(4096);
    JpegEncoder::new_with_quality(&mut buf, 78)
        .write_image(small.as_raw(), tw, th, image::ExtendedColorType::Rgb8)?;
    Ok(buf)
}
