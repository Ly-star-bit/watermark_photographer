// 画布比例扩展：给照片四周补白边，扩展到目标宽高比或精确像素尺寸
//
// 与 frame.rs 的区别：frame 是等宽边框（上/左/右同宽）+ 底部参数条；
// 这里只补齐单一方向（宽图补上下，高图补左右）到目标比例，不加文字。
//
// 两个入口：
//   expand_to_ratio    —— 保持原始分辨率，只按比例补白（供 WatermarkConfig.canvas_ratio 使用）
//   fit_to_exact_size  —— 缩放到刚好装入目标像素框，再补白到精确尺寸（供社媒导出预设使用）

use image::imageops::FilterType;
use image::{Rgb, RgbImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasRatioConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ratio_w")]
    pub ratio_w: u32,
    #[serde(default = "default_ratio_h")]
    pub ratio_h: u32,
    #[serde(default = "default_fill_color")]
    pub fill_color: [u8; 3],
}

fn default_ratio_w() -> u32 {
    1
}
fn default_ratio_h() -> u32 {
    1
}
fn default_fill_color() -> [u8; 3] {
    [255, 255, 255]
}

impl Default for CanvasRatioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ratio_w: default_ratio_w(),
            ratio_h: default_ratio_h(),
            fill_color: default_fill_color(),
        }
    }
}

/// 把图像居中补白边扩展到目标宽高比，不裁切、不缩放原内容。
/// ratio_w/ratio_h 或图像本身尺寸为 0 时原样返回。
pub fn expand_to_ratio(img: &RgbImage, ratio_w: u32, ratio_h: u32, fill: [u8; 3]) -> RgbImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || ratio_w == 0 || ratio_h == 0 {
        return img.clone();
    }

    let target_ratio = ratio_w as f32 / ratio_h as f32;
    let cur_ratio = w as f32 / h as f32;

    let (new_w, new_h) = if cur_ratio > target_ratio {
        // 原图比目标更"扁"（更宽），需要补高度
        let new_h = (w as f32 / target_ratio).round() as u32;
        (w, new_h.max(h))
    } else {
        // 原图比目标更"窄"（更高），需要补宽度
        let new_w = (h as f32 * target_ratio).round() as u32;
        (new_w.max(w), h)
    };

    if new_w == w && new_h == h {
        return img.clone();
    }

    let mut canvas = RgbImage::from_pixel(new_w, new_h, Rgb(fill));
    let off_x = (new_w - w) / 2;
    let off_y = (new_h - h) / 2;
    image::imageops::replace(&mut canvas, img, off_x as i64, off_y as i64);
    canvas
}

/// 缩放图像到刚好装入 (target_w, target_h)（等比、不放大裁切），再居中补白到精确目标尺寸。
/// target_w/target_h 或图像本身尺寸为 0 时原样返回。
pub fn fit_to_exact_size(img: &RgbImage, target_w: u32, target_h: u32, fill: [u8; 3]) -> RgbImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || target_w == 0 || target_h == 0 {
        return img.clone();
    }

    let scale = (target_w as f32 / w as f32).min(target_h as f32 / h as f32);
    let new_w = ((w as f32 * scale).round() as u32).max(1).min(target_w);
    let new_h = ((h as f32 * scale).round() as u32).max(1).min(target_h);

    let scaled = if new_w == w && new_h == h {
        img.clone()
    } else {
        image::imageops::resize(img, new_w, new_h, FilterType::Lanczos3)
    };

    if new_w == target_w && new_h == target_h {
        return scaled;
    }

    let mut canvas = RgbImage::from_pixel(target_w, target_h, Rgb(fill));
    let off_x = (target_w - new_w) / 2;
    let off_y = (target_h - new_h) / 2;
    image::imageops::replace(&mut canvas, &scaled, off_x as i64, off_y as i64);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_wide_image_adds_height() {
        // 400x200，目标 1:1 → 需要补高到 400x400
        let img = RgbImage::from_pixel(400, 200, Rgb([100, 100, 100]));
        let out = expand_to_ratio(&img, 1, 1, [255, 255, 255]);
        assert_eq!(out.dimensions(), (400, 400));
        // 顶部应为白边
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255]);
        // 中心应为原图内容
        assert_eq!(out.get_pixel(200, 200).0, [100, 100, 100]);
    }

    #[test]
    fn expand_tall_image_adds_width() {
        // 200x400，目标 1:1 → 需要补宽到 400x400
        let img = RgbImage::from_pixel(200, 400, Rgb([50, 60, 70]));
        let out = expand_to_ratio(&img, 1, 1, [255, 255, 255]);
        assert_eq!(out.dimensions(), (400, 400));
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(out.get_pixel(200, 200).0, [50, 60, 70]);
    }

    #[test]
    fn expand_noop_when_ratio_already_matches() {
        let img = RgbImage::from_pixel(400, 400, Rgb([1, 2, 3]));
        let out = expand_to_ratio(&img, 1, 1, [255, 255, 255]);
        assert_eq!(out.dimensions(), (400, 400));
    }

    #[test]
    fn fit_to_exact_size_matches_target_dimensions() {
        let img = RgbImage::from_pixel(3000, 4000, Rgb([10, 20, 30]));
        let out = fit_to_exact_size(&img, 1080, 1440, [255, 255, 255]);
        assert_eq!(out.dimensions(), (1080, 1440));
    }

    #[test]
    fn fit_to_exact_size_pads_mismatched_ratio() {
        // 原图 4000x3000（4:3），目标 1080x1440（3:4 竖版）
        // scale = min(1080/4000, 1440/3000) = 0.27 → 缩放后 1080x810，宽度刚好填满，
        // 高度不足，需要在上下补白（而非左右）。
        let img = RgbImage::from_pixel(4000, 3000, Rgb([10, 20, 30]));
        let out = fit_to_exact_size(&img, 1080, 1440, [255, 255, 255]);
        assert_eq!(out.dimensions(), (1080, 1440));
        // 顶部应为白边
        assert_eq!(out.get_pixel(540, 5).0, [255, 255, 255]);
        // 垂直居中区域应为原图内容
        assert_eq!(out.get_pixel(540, 720).0, [10, 20, 30]);
    }
}
