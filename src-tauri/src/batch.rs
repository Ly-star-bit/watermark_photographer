// 批量并行处理
//
// 设计：
// - 水印 PNG 预读一次，字节切片共享给所有 worker（避免重复 IO）
// - rayon par_iter 全 CPU 核并行，AtomicUsize 计数完成数
// - 每处理完一张调用 on_progress 回调，由 command 层转发为 Tauri 事件
// - 输出文件名：{原文件名 stem}_wm.{原扩展名}，落到指定输出目录
// - 单张失败不影响其他文件继续处理，错误信息聚合到结果中

use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::Result;
use crate::position::WatermarkConfig;
use crate::watermark;

pub struct BatchInput {
    pub input_paths: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub watermark_bytes: Vec<u8>,
    pub config: WatermarkConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemResult {
    pub input: String,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// 进度回调签名：(已完成数, 总数, 当前文件名, 本项是否成功)
/// 使用泛型避免 dyn 的 'static 生命周期约束，让调用方可以借用栈上数据
pub fn run<F>(task: &BatchInput, on_progress: F) -> Vec<ItemResult>
where
    F: Fn(usize, usize, &str, bool) + Sync + Send,
{
    let _ = std::fs::create_dir_all(&task.output_dir);
    let total = task.input_paths.len();
    let counter = AtomicUsize::new(0);

    task.input_paths
        .par_iter()
        .map(|src| {
            let result = process_one(src, &task.output_dir, &task.watermark_bytes, &task.config);
            let done = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("?");
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
) -> ItemResult {
    let input_str = src.display().to_string();
    match do_one(src, out_dir, wm, config) {
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
) -> Result<PathBuf> {
    let src_bytes = std::fs::read(src)?;
    let out_bytes = watermark::apply(&src_bytes, wm, config)?;

    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    // 输出统一为 JPEG（不管输入是 png/tiff/webp/bmp，都产出 .jpg）
    let out_name = format!("{stem}_wm.jpg");
    let out_path = out_dir.join(out_name);

    std::fs::write(&out_path, out_bytes)?;
    Ok(out_path)
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

    #[test]
    fn batch_produces_outputs_and_progress() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("out");

        // 三张源图
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
            config: WatermarkConfig {
                position: GridPosition::BottomRight,
                size_ratio: 0.15,
                opacity: 0.8,
                margin_x: 10,
                margin_y: 10,
                landscape_override: None,
                tint: None,
            },
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

        // 输出文件都存在，命名带 _wm 后缀
        for i in 0..3 {
            let expected = out_dir.join(format!("photo_{i}_wm.jpg"));
            assert!(expected.exists(), "missing {expected:?}");
        }
    }

    #[test]
    fn batch_isolates_failure() {
        // 一张源不存在，另两张正常。失败不应中断其他文件。
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
            },
        };

        let results = run(&task, |_, _, _, _| {});
        assert_eq!(results.len(), 3);
        let failures: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
        let successes: Vec<_> = results.iter().filter(|r| r.error.is_none()).collect();
        assert_eq!(failures.len(), 1, "应有 1 个失败");
        assert_eq!(successes.len(), 2, "应有 2 个成功");
    }
}
