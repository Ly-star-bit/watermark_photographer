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
use crate::export::ExportOptions;
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
    #[serde(default)]
    pub export_options: ExportOptions,
    #[serde(default = "default_filename_template")]
    pub filename_template: String,
}

fn default_filename_template() -> String {
    "{stem}_wm".to_string()
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
    eprintln!("========== [export_batch] 开始 ==========");
    eprintln!(
        "[export_batch] 输入 {} 张，输出目录={}",
        args.input_paths.len(),
        args.output_dir
    );
    eprintln!("[export_batch] 水印路径={}", args.watermark_path);
    eprintln!(
        "[export_batch] config: position={:?} size_ratio={} opacity={} margin=({},{})",
        args.config.position,
        args.config.size_ratio,
        args.config.opacity,
        args.config.margin_x,
        args.config.margin_y
    );
    match &args.config.exif_text {
        Some(etc) => eprintln!(
            "[export_batch] exif_text: enabled={} template={:?} custom_text={:?} font_size_ratio={} position={:?} margin=({},{}) opacity={} color={:?} background={:?}",
            etc.enabled, etc.template, etc.custom_text,
            etc.font_size_ratio, etc.position,
            etc.margin_x, etc.margin_y, etc.opacity,
            etc.color, etc.background
        ),
        None => eprintln!("[export_batch] exif_text=None（前端未传或为 null）"),
    }
    eprintln!(
        "[export_batch] export_options: max_long_side={:?} quality={} format={:?}",
        args.export_options.max_long_side,
        args.export_options.quality,
        args.export_options.format
    );
    eprintln!("[export_batch] filename_template={:?}", args.filename_template);
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
        export_options: args.export_options,
        filename_template: args.filename_template,
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

// —— EXIF 文字预览 ——————————————————————————————————————
// 前端 Canvas 预览需要知道当前照片的 EXIF 模板渲染结果，
// 此命令从照片文件中提取 EXIF、按模板格式化后返回纯文本。

#[derive(Debug, Serialize)]
pub struct ExifTextPreview {
    pub text: String,
}

#[tauri::command]
pub fn preview_exif_text(
    path: String,
    template: String,
    custom_text: Option<String>,
) -> Result<ExifTextPreview> {
    // 自定义文字模式：直接返回
    if let Some(ref ct) = custom_text {
        return Ok(ExifTextPreview { text: ct.clone() });
    }
    // EXIF 模式：从文件解析
    let src_bytes = std::fs::read(&path)?;
    let meta = crate::metadata::extract(&src_bytes)
        .unwrap_or_else(|_| crate::metadata::Metadata::empty());
    let tags = match &meta.exif {
        Some(raw) => crate::exif_text::parse_exif(raw.as_ref()),
        None => std::collections::HashMap::new(),
    };
    let text = if tags.is_empty() {
        String::new()
    } else {
        crate::exif_text::format_template(&template, &tags)
    };
    Ok(ExifTextPreview { text })
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
