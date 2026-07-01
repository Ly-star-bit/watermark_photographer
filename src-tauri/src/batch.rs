// 批量并行处理
//
// 设计：
// - 水印 PNG 预读一次，字节切片共享给所有 worker（避免重复 IO）
// - 字体全局惰性加载一次
// - rayon par_iter 全 CPU 核并行，AtomicUsize 计数完成数
// - 每处理完一张调用 on_progress 回调，由 command 层转发为 Tauri 事件
// - 编码管道在 compose 之后：可选长边缩放 → 按格式/质量编码 → 元数据回注 → 写文件
// - 文件名由模板生成（{stem}/{n}/{date}）
// - 单张失败不影响其他文件继续处理，错误信息聚合到结果中

use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::Result;
use crate::export::{ExportOptions, OutputFormat};
use crate::position::WatermarkConfig;
use crate::watermark;

pub struct BatchInput {
    pub input_paths: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub watermark_bytes: Vec<u8>,
    pub config: WatermarkConfig,
    pub export_options: ExportOptions,
    pub filename_template: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemResult {
    pub input: String,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// 进度回调签名：(已完成数, 总数, 当前文件名, 本项是否成功)
pub fn run<F>(task: &BatchInput, on_progress: F) -> Vec<ItemResult>
where
    F: Fn(usize, usize, &str, bool) + Sync + Send,
{
    let _ = std::fs::create_dir_all(&task.output_dir);
    let total = task.input_paths.len();
    let counter = AtomicUsize::new(0);
    let font = crate::exif_text::get_font();

    task.input_paths
        .par_iter()
        .map(|src| {
            let seq = counter.fetch_add(1, Ordering::SeqCst) + 1; // 1-based
            let result = process_one(
                src,
                &task.output_dir,
                &task.watermark_bytes,
                &task.config,
                font,
                &task.export_options,
                &task.filename_template,
                seq,
            );
            let done = seq; // seq is already = fetch_add + 1 = done count
            let name = src
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?");
            (on_progress)(done, total, name, result.error.is_none());
            result
        })
        .collect()
}

fn process_one(
    src: &Path,
    out_dir: &Path,
    wm: &[u8],
    config: &WatermarkConfig,
    font: &ab_glyph::FontRef<'static>,
    export_opts: &ExportOptions,
    filename_template: &str,
    seq: usize,
) -> ItemResult {
    let input_str = src.display().to_string();
    match do_one(src, out_dir, wm, config, font, export_opts, filename_template, seq) {
        Ok(out) => ItemResult {
            input: input_str,
            output: Some(out.display().to_string()),
            error: None,
        },
        Err(e) => ItemResult {
            input: input_str,
            output: None,
            error: Some(e.to_string()),
        },
    }
}

fn do_one(
    src: &Path,
    out_dir: &Path,
    wm: &[u8],
    config: &WatermarkConfig,
    font: &ab_glyph::FontRef<'static>,
    export_opts: &ExportOptions,
    filename_template: &str,
    seq: usize,
) -> Result<PathBuf> {
    eprintln!("---------- [do_one] 处理 {} ----------", src.display());
    let src_bytes = std::fs::read(src)?;
    eprintln!("[do_one] 源文件字节数={}", src_bytes.len());

    // 1. 合成（含签名水印 + 可选 EXIF 文字）
    let exif_text_cfg = config.exif_text.as_ref();
    eprintln!(
        "[do_one] exif_text_cfg 传入 compose: {}",
        if exif_text_cfg.is_some() { "Some(..)" } else { "None" }
    );
    let (composed, meta) = watermark::compose(&src_bytes, wm, config, exif_text_cfg, font)?;
    eprintln!(
        "[do_one] compose 完成，composed={}x{} meta.exif={} meta.icc={}",
        composed.width(),
        composed.height(),
        meta.exif.is_some(),
        meta.icc.is_some()
    );

    // 2. 可选缩图（长边限制）
    let final_img = if let Some(resized) = watermark::maybe_resize(&composed, export_opts.max_long_side) {
        eprintln!(
            "[do_one] maybe_resize 触发：{}x{} → {}x{}",
            composed.width(), composed.height(), resized.width(), resized.height()
        );
        resized
    } else {
        eprintln!("[do_one] maybe_resize 未触发（max_long_side={:?}）", export_opts.max_long_side);
        composed
    };

    // 3. 编码
    let encoded = encode_final(&final_img, export_opts)?;
    eprintln!(
        "[do_one] 编码完成 format={:?} quality={} 字节数={}",
        export_opts.format, export_opts.quality, encoded.len()
    );

    // 4. 回注 EXIF/ICC（仅 JPEG 输出；PNG/WebP 的元数据处理由 image crate 内部完成）
    let final_bytes = if export_opts.format == OutputFormat::Jpeg {
        let injected = crate::metadata::inject(encoded, &meta)?;
        eprintln!("[do_one] EXIF/ICC 回注后字节数={}", injected.len());
        injected
    } else {
        encoded
    };

    // 5. 文件名模板
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = export_opts.format.extension();
    let fname = apply_template(filename_template, stem, seq);
    let out_path = out_dir.join(format!("{fname}.{ext}"));

    // 6. 写入
    std::fs::write(&out_path, final_bytes)?;
    eprintln!("[do_one] ✅ 写入 {}", out_path.display());
    Ok(out_path)
}

// —— 编码 ————————————————————————————————————————————————

fn encode_final(img: &image::RgbImage, opts: &ExportOptions) -> Result<Vec<u8>> {
    match opts.format {
        OutputFormat::Jpeg => watermark::encode_jpeg(img, opts.quality),
        OutputFormat::Png => watermark::encode_png(img),
        OutputFormat::Webp => watermark::encode_webp(img, opts.quality as f32),
    }
}

// —— 文件名模板 ————————————————————————————————————————————

/// 替换模板变量：
/// - {stem} → 原始文件名（不含扩展名）
/// - {n}    → 序号（1-based，3 位零填充）
/// - {date} → 当天日期（YYYYMMDD）
/// 其余字符原样保留。
fn apply_template(template: &str, stem: &str, n: usize) -> String {
    let date = chrono_date();
    let mut result = String::with_capacity(template.len() + 32);
    let bytes = template.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        if bytes[pos] == b'{' {
            if let Some(end) = template[pos..].find('}') {
                let key = &template[pos + 1..pos + end];
                match key {
                    "stem" => result.push_str(stem),
                    "n" => result.push_str(&format!("{n:03}")),
                    "date" => result.push_str(&date),
                    _ => {
                        result.push('{');
                        result.push_str(key);
                        result.push('}');
                    }
                }
                pos += end + 1;
            } else {
                result.push('{');
                pos += 1;
            }
        } else {
            result.push(bytes[pos] as char);
            pos += 1;
        }
    }

    result
}

/// 获取当天日期字符串（YYYYMMDD），使用标准库避免额外依赖
fn chrono_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 从 UNIX 时间戳计算日期（不考虑闰秒）
    let days = secs / 86400;
    // 从 1970-01-01 开始推算
    let (y, m, d) = civil_from_days(days as i64 + 719468); // 719468 = days from 0000-01-01 to 1970-01-01
    format!("{y:04}{m:02}{d:02}")
}

/// 从 Rata Die 天数反推公历日期（简化版，1900-2100 年准确）
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // https://howardhinnant.github.io/date_algorithms.html#civil_from_days
    let z = days;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::GridPosition;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
    use std::sync::atomic::AtomicUsize;
    use tempfile::tempdir;

    fn make_jpeg_at(path: &Path, w: u32, h: u32, color: Rgb<u8>) {
        let img = RgbImage::from_pixel(w, h, color);
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 95)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        std::fs::write(path, buf).unwrap();
    }

    fn make_png_bytes(w: u32, h: u32, color: Rgba<u8>) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, color);
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    fn default_export_opts() -> ExportOptions {
        ExportOptions::default()
    }

    fn default_config() -> WatermarkConfig {
        WatermarkConfig {
            position: GridPosition::BottomRight,
            size_ratio: 0.15,
            opacity: 0.8,
            margin_x: 10,
            margin_y: 10,
            landscape_override: None,
            tint: None,
            exif_text: None,
        }
    }

    #[test]
    fn batch_produces_outputs_and_progress() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("out");

        let mut inputs = Vec::new();
        for i in 0..3 {
            let p = dir.path().join(format!("photo_{i}.jpg"));
            make_jpeg_at(&p, 200, 150, Rgb([200, 200, 200]));
            inputs.push(p);
        }

        let task = BatchInput {
            input_paths: inputs.clone(),
            output_dir: out_dir.clone(),
            watermark_bytes: make_png_bytes(30, 30, Rgba([255, 0, 0, 255])),
            config: default_config(),
            export_options: default_export_opts(),
            filename_template: "{stem}_wm".to_string(),
        };

        let progress_calls = AtomicUsize::new(0);
        let last_done = std::sync::Mutex::new(0usize);
        let results = run(&task, |done, total, _name, ok| {
            assert!(ok);
            assert_eq!(total, 3);
            progress_calls.fetch_add(1, Ordering::SeqCst);
            let mut ld = last_done.lock().unwrap();
            if done > *ld {
                *ld = done;
            }
        });
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.error.is_none()));
        assert_eq!(progress_calls.load(Ordering::SeqCst), 3);
        assert_eq!(*last_done.lock().unwrap(), 3);

        for i in 0..3 {
            let expected = out_dir.join(format!("photo_{i}_wm.jpg"));
            assert!(expected.exists(), "missing {expected:?}");
        }
    }

    #[test]
    fn batch_isolates_failure() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("out");
        let good = dir.path().join("good.jpg");
        make_jpeg_at(&good, 100, 100, Rgb([100, 100, 100]));
        let bad = dir.path().join("does_not_exist.jpg");

        let task = BatchInput {
            input_paths: vec![good.clone(), bad.clone(), good.clone()],
            output_dir: out_dir.clone(),
            watermark_bytes: make_png_bytes(20, 20, Rgba([0, 255, 0, 255])),
            config: WatermarkConfig {
                position: GridPosition::TopLeft,
                size_ratio: 0.15,
                opacity: 1.0,
                margin_x: 5,
                margin_y: 5,
                landscape_override: None,
                tint: None,
                exif_text: None,
            },
            export_options: default_export_opts(),
            filename_template: "{stem}_wm".to_string(),
        };

        let results = run(&task, |_, _, _, _| {});
        assert_eq!(results.len(), 3);
        let failures: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
        let successes: Vec<_> = results.iter().filter(|r| r.error.is_none()).collect();
        assert_eq!(failures.len(), 1, "应有 1 个失败");
        assert_eq!(successes.len(), 2, "应有 2 个成功");
    }

    #[test]
    fn filename_template_stem() {
        let result = apply_template("{stem}_wm", "DSCF0001", 1);
        assert_eq!(result, "DSCF0001_wm");
    }

    #[test]
    fn filename_template_with_seq() {
        let result = apply_template("{stem}_{n}", "photo", 5);
        assert_eq!(result, "photo_005");
    }

    #[test]
    fn filename_template_with_date() {
        let result = apply_template("{date}_{stem}", "img", 1);
        // 日期部分应为 8 位数字
        let parts: Vec<&str> = result.splitn(2, '_').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 8, "日期应为 8 位数字");
        assert!(parts[0].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(parts[1], "img");
    }

    #[test]
    fn filename_template_complex() {
        let result = apply_template("wedding/{date}_{stem}_{n}", "DSCF0001", 42);
        assert!(result.starts_with("wedding/"));
        assert!(result.contains("_DSCF0001_042"));
    }

    #[test]
    fn png_output_format() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("out");
        let src = dir.path().join("test.jpg");
        make_jpeg_at(&src, 200, 150, Rgb([100, 100, 100]));

        let task = BatchInput {
            input_paths: vec![src],
            output_dir: out_dir.clone(),
            watermark_bytes: make_png_bytes(20, 20, Rgba([255, 0, 0, 255])),
            config: default_config(),
            export_options: ExportOptions {
                format: OutputFormat::Png,
                ..Default::default()
            },
            filename_template: "{stem}".to_string(),
        };

        let results = run(&task, |_, _, _, _| {});
        assert_eq!(results.len(), 1);
        assert!(results[0].error.is_none());
        let out_path = out_dir.join("test.png");
        assert!(out_path.exists(), "PNG 输出应存在: {out_path:?}");
    }

    #[test]
    fn webp_output_format() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("out");
        let src = dir.path().join("test.jpg");
        make_jpeg_at(&src, 200, 150, Rgb([100, 100, 100]));

        let task = BatchInput {
            input_paths: vec![src],
            output_dir: out_dir.clone(),
            watermark_bytes: make_png_bytes(20, 20, Rgba([255, 0, 0, 255])),
            config: default_config(),
            export_options: ExportOptions {
                format: OutputFormat::Webp,
                quality: 80,
                ..Default::default()
            },
            filename_template: "{stem}".to_string(),
        };

        let results = run(&task, |_, _, _, _| {});
        assert_eq!(results.len(), 1);
        assert!(results[0].error.is_none());
        let out_path = out_dir.join("test.webp");
        assert!(out_path.exists(), "WebP 输出应存在: {out_path:?}");
    }

    #[test]
    fn resize_with_long_side_limit() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("out");
        let src = dir.path().join("test.jpg");
        make_jpeg_at(&src, 6000, 4000, Rgb([100, 100, 100]));

        let task = BatchInput {
            input_paths: vec![src],
            output_dir: out_dir.clone(),
            watermark_bytes: make_png_bytes(20, 20, Rgba([255, 0, 0, 255])),
            config: default_config(),
            export_options: ExportOptions {
                max_long_side: Some(2048),
                ..Default::default()
            },
            filename_template: "{stem}_{n}".to_string(),
        };

        let results = run(&task, |_, _, _, _| {});
        assert!(results[0].error.is_none());

        // 检查输出尺寸
        let out_path = out_dir.join("test_001.jpg");
        let decoded = image::open(&out_path).unwrap();
        assert_eq!(decoded.width(), 2048); // 长边限制到 2048
        assert_eq!(decoded.height(), 1365); // 4000 * 2048/6000 = 1365
    }
}
