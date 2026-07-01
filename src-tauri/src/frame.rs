// EXIF 相框模式（小米徕卡风）
//
// 职责：把原图包在白/黑边框里，底部加一条参数条，
// 左侧展示型号镜头、右侧展示光圈快门ISO/焦距、中央放品牌名。
//
// 品牌名来自 Make 标签自动归一化（FUJIFILM/SONY/Canon/NIKON/LEICA/Panasonic/Hasselblad），
// 品牌名在中央用大号加粗字（复用 exif_text 字体渲染）。
//
// 与既有渲染管线的关系：
//   frame::apply 在 watermark::compose 的最后一步（RGB 展平之后）调用，
//   作为「最终包装」：不改变签名 PNG 和文字水印的既有坐标系。

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;

// —— 配置 ————————————————————————————————————————————————

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 边框颜色 [R, G, B]，默认白 [250, 250, 250]
    #[serde(default = "default_border_color")]
    pub border_color: [u8; 3],
    /// 边框宽度（相对图片短边比例）：上/左/右三边等宽
    #[serde(default = "default_border_ratio")]
    pub border_ratio: f32,
    /// 底部参数条高度（相对短边比例）；比上/左/右厚
    #[serde(default = "default_bottom_bar_ratio")]
    pub bottom_bar_ratio: f32,
    /// 参数条上文字颜色
    #[serde(default = "default_text_color")]
    pub text_color: [u8; 3],
    /// 副文字颜色（第二行、稍暗）
    #[serde(default = "default_subtext_color")]
    pub subtext_color: [u8; 3],
    /// 左块两行模板（默认：型号 / 镜头）
    #[serde(default = "default_left_lines")]
    pub left_lines: Vec<String>,
    /// 右块两行模板（默认：光圈快门ISO / 焦距）
    #[serde(default = "default_right_lines")]
    pub right_lines: Vec<String>,
    /// 中央品牌名模板（"{brand}" 会从 make 自动归一化）
    #[serde(default = "default_brand_template")]
    pub brand_template: String,
    /// 是否显示中央品牌名（关掉的话中间留白）
    #[serde(default = "default_show_brand")]
    pub show_brand: bool,
    /// 主文字字号（相对参数条高度）
    #[serde(default = "default_font_size_ratio")]
    pub font_size_ratio: f32,
    /// 品牌名字号（相对参数条高度）
    #[serde(default = "default_brand_size_ratio")]
    pub brand_size_ratio: f32,
}

fn default_border_color() -> [u8; 3] {
    [250, 250, 250]
}
fn default_border_ratio() -> f32 {
    0.02
}
fn default_bottom_bar_ratio() -> f32 {
    0.12
}
fn default_text_color() -> [u8; 3] {
    [30, 30, 30]
}
fn default_subtext_color() -> [u8; 3] {
    [110, 110, 110]
}
fn default_left_lines() -> Vec<String> {
    vec!["{model}".to_string(), "{lens}".to_string()]
}
fn default_right_lines() -> Vec<String> {
    vec![
        "{focal}  f/{fnumber}  {shutter}s  ISO {iso}".to_string(),
        "{date}".to_string(),
    ]
}
fn default_brand_template() -> String {
    "{brand}".to_string()
}
fn default_show_brand() -> bool {
    true
}
fn default_font_size_ratio() -> f32 {
    0.22
}
fn default_brand_size_ratio() -> f32 {
    0.42
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            border_color: default_border_color(),
            border_ratio: default_border_ratio(),
            bottom_bar_ratio: default_bottom_bar_ratio(),
            text_color: default_text_color(),
            subtext_color: default_subtext_color(),
            left_lines: default_left_lines(),
            right_lines: default_right_lines(),
            brand_template: default_brand_template(),
            show_brand: default_show_brand(),
            font_size_ratio: default_font_size_ratio(),
            brand_size_ratio: default_brand_size_ratio(),
        }
    }
}

// —— 品牌名归一化 ————————————————————————————————————————
//
// 相机 Make 标签内容不统一：富士写 "FUJIFILM"、佳能写 "Canon"、尼康写 "NIKON CORPORATION"。
// 统一按大写品牌名展示，模板变量 `{brand}` 即取此值。

pub fn normalize_brand(make: &str) -> String {
    let upper = make.to_uppercase();
    if upper.contains("FUJI") {
        "FUJIFILM".to_string()
    } else if upper.contains("SONY") {
        "SONY".to_string()
    } else if upper.contains("CANON") {
        "Canon".to_string()
    } else if upper.contains("NIKON") {
        "NIKON".to_string()
    } else if upper.contains("LEICA") {
        "LEICA".to_string()
    } else if upper.contains("PANASONIC") || upper.contains("LUMIX") {
        "LUMIX".to_string()
    } else if upper.contains("HASSELBLAD") {
        "HASSELBLAD".to_string()
    } else if upper.contains("OLYMPUS") {
        "OLYMPUS".to_string()
    } else if upper.contains("PENTAX") {
        "PENTAX".to_string()
    } else if upper.contains("RICOH") {
        "RICOH".to_string()
    } else if upper.contains("SIGMA") {
        "SIGMA".to_string()
    } else if upper.contains("APPLE") {
        "iPhone".to_string()
    } else if upper.contains("XIAOMI") {
        "Xiaomi".to_string()
    } else if upper.contains("HUAWEI") {
        "HUAWEI".to_string()
    } else {
        // 未知厂商：保留原字符串
        make.to_string()
    }
}

// —— 文本解析（供渲染与前端预览命令共用） ——————————————————————

/// 参数条三块文本的解析结果：左块两行、右块两行、中央品牌名。
/// 已应用模板替换 + 品牌归一化，空行已过滤。
#[derive(Debug, Clone, Default)]
pub struct FrameTexts {
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub brand: String,
}

/// 把 `FrameConfig` 里的模板（`left_lines`/`right_lines`/`brand_template`）
/// 结合 EXIF tags 解析成最终展示文本。
/// 品牌名先从 `tags["make"]` 归一化写入 `aug_tags["brand"]`，供模板引用 `{brand}`。
pub fn resolve_texts(config: &FrameConfig, tags: &HashMap<&'static str, String>) -> FrameTexts {
    let mut brand = String::new();
    if let Some(m) = tags.get("make") {
        brand = normalize_brand(m);
    }
    let mut aug_tags = tags.clone();
    aug_tags.insert("brand", brand.clone());

    let left: Vec<String> = config
        .left_lines
        .iter()
        .map(|t| format_template(t, &aug_tags))
        .filter(|s| !s.trim().is_empty())
        .collect();
    let right: Vec<String> = config
        .right_lines
        .iter()
        .map(|t| format_template(t, &aug_tags))
        .filter(|s| !s.trim().is_empty())
        .collect();
    let brand_text = if config.show_brand {
        format_template(&config.brand_template, &aug_tags)
    } else {
        String::new()
    };

    FrameTexts {
        left,
        right,
        brand: brand_text,
    }
}

// —— 主入口 ————————————————————————————————————————————————

/// 生成加了相框的 RGB 图。
/// 输入：原图（已合成完签名/文字水印）、EXIF 解析结果、字体。
pub fn apply(
    photo: &RgbImage,
    config: &FrameConfig,
    tags: &HashMap<&'static str, String>,
    font: &FontRef<'static>,
) -> Result<RgbImage> {
    let (pw, ph) = (photo.width(), photo.height());
    let short = pw.min(ph) as f32;
    let border = (short * config.border_ratio).round() as u32;
    let bottom_bar = (short * config.bottom_bar_ratio).round() as u32;

    // 新画布尺寸：上/左/右 border + 下方 bottom_bar（bottom_bar 已包含边距）
    let new_w = pw + border * 2;
    let new_h = ph + border + bottom_bar;

    // 生成扩展画布并填充边框色
    let bg = Rgb(config.border_color);
    let mut canvas = RgbImage::from_pixel(new_w, new_h, bg);

    // 把原图拷贝到 (border, border)
    image::imageops::replace(&mut canvas, photo, border as i64, border as i64);

    // —— 在底部参数条上渲染文字 ——————————————————————————————
    let bar_top = border + ph;
    let bar_h = bottom_bar;
    let bar_w = new_w;

    // 参数条内部左右边距
    let inner_pad = (bar_h as f32 * 0.15).round() as u32;

    let main_font_px = (bar_h as f32 * config.font_size_ratio).max(10.0);
    let brand_font_px = (bar_h as f32 * config.brand_size_ratio).max(12.0);

    // 副文字略小
    let sub_font_px = main_font_px * 0.85;

    let texts = resolve_texts(config, tags);
    let left_texts = texts.left;
    let right_texts = texts.right;
    let brand_text = texts.brand;

    // 左块：从左边距开始，竖直居中
    let left_x0 = border + inner_pad;
    let text_block_h = (main_font_px + sub_font_px * 0.2 + sub_font_px) as u32;
    let text_y0 = bar_top + (bar_h - text_block_h) / 2;

    for (i, line) in left_texts.iter().enumerate() {
        let (fs, color) = if i == 0 {
            (main_font_px, config.text_color)
        } else {
            (sub_font_px, config.subtext_color)
        };
        let y = if i == 0 {
            text_y0 as f32
        } else {
            text_y0 as f32 + main_font_px * 1.15
        };
        draw_text_on_rgb(&mut canvas, line, left_x0 as f32, y, fs, color, font, TextAnchor::Left);
    }

    // 右块：从右边距对齐，竖直居中
    let right_x1 = new_w - border - inner_pad;
    for (i, line) in right_texts.iter().enumerate() {
        let (fs, color) = if i == 0 {
            (main_font_px, config.text_color)
        } else {
            (sub_font_px, config.subtext_color)
        };
        let y = if i == 0 {
            text_y0 as f32
        } else {
            text_y0 as f32 + main_font_px * 1.15
        };
        draw_text_on_rgb(&mut canvas, line, right_x1 as f32, y, fs, color, font, TextAnchor::Right);
    }

    // 中央：品牌名，字号更大，垂直居中
    if !brand_text.trim().is_empty() {
        let cx = (bar_w / 2) as f32;
        let cy = bar_top as f32 + (bar_h as f32 - brand_font_px) / 2.0;
        draw_text_on_rgb(
            &mut canvas,
            &brand_text,
            cx,
            cy,
            brand_font_px,
            config.text_color,
            font,
            TextAnchor::Center,
        );
    }

    // 参数条上方加一条细分割线，视觉上和相片分开
    let sep_thickness = ((bar_h as f32) * 0.015).max(1.0) as u32;
    let sep_color = darken(config.border_color, 0.85);
    for dy in 0..sep_thickness {
        for x in border..(new_w - border) {
            canvas.put_pixel(x, bar_top + dy, Rgb(sep_color));
        }
    }

    Ok(canvas)
}

// —— 文字绘制辅助 ————————————————————————————————————————
//
// 在 RGB 画布上直接绘制文字（不走 RGBA 中间层），效率更高。
// alpha 混合仅在字体抗锯齿边缘发生。

enum TextAnchor {
    Left,
    Right,
    Center,
}

fn draw_text_on_rgb(
    canvas: &mut RgbImage,
    text: &str,
    x: f32,
    y: f32,
    font_px: f32,
    color: [u8; 3],
    font: &FontRef<'static>,
    anchor: TextAnchor,
) {
    let scale = PxScale::from(font_px);
    let scaled_font = font.as_scaled(scale);
    let ascent = scaled_font.ascent();

    // 先测量文字宽度以处理对齐
    let mut total_w = 0f32;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        total_w += scaled_font.h_advance(gid);
    }
    let start_x = match anchor {
        TextAnchor::Left => x,
        TextAnchor::Right => x - total_w,
        TextAnchor::Center => x - total_w / 2.0,
    };

    let baseline_y = y + ascent;

    let (cw, ch) = (canvas.width(), canvas.height());
    let mut x_cursor = start_x;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        let glyph = gid.with_scale_and_position(scale, point(x_cursor, baseline_y));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bb = outlined.px_bounds();
            let offset_x = bb.min.x as i32;
            let offset_y = bb.min.y as i32;
            outlined.draw(|px, py, coverage| {
                let ax = px as i32 + offset_x;
                let ay = py as i32 + offset_y;
                if ax < 0 || ay < 0 {
                    return;
                }
                let ix = ax as u32;
                let iy = ay as u32;
                if ix >= cw || iy >= ch {
                    return;
                }
                let cov = coverage.clamp(0.0, 1.0);
                if cov <= 0.0 {
                    return;
                }
                let p = canvas.get_pixel_mut(ix, iy);
                // 直接线性 alpha 混合到 RGB（背景不透明）
                p[0] = ((color[0] as f32) * cov + (p[0] as f32) * (1.0 - cov)) as u8;
                p[1] = ((color[1] as f32) * cov + (p[1] as f32) * (1.0 - cov)) as u8;
                p[2] = ((color[2] as f32) * cov + (p[2] as f32) * (1.0 - cov)) as u8;
            });
        }
        x_cursor += scaled_font.h_advance(gid);
    }
}

/// 把颜色按因子（0..1）压暗
fn darken(c: [u8; 3], factor: f32) -> [u8; 3] {
    [
        (c[0] as f32 * factor) as u8,
        (c[1] as f32 * factor) as u8,
        (c[2] as f32 * factor) as u8,
    ]
}

/// 简化版模板替换（与 exif_text::format_template 语义一致）
fn format_template(template: &str, tags: &HashMap<&'static str, String>) -> String {
    let mut result = String::with_capacity(template.len() + 32);
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut pos = 0;
    while pos < len {
        if chars[pos] == '{' {
            let mut end = pos + 1;
            let mut found = false;
            while end < len {
                if chars[end] == '}' {
                    found = true;
                    break;
                }
                end += 1;
            }
            if found {
                let key: String = chars[pos + 1..end].iter().collect();
                if let Some(val) = tags.get(key.as_str()) {
                    result.push_str(val);
                }
                // 未匹配的 key 直接删除（不像 exif_text 保留原样，
                // 这样空 EXIF 时参数条更干净不会出现 {fnumber}）
                pos = end + 1;
            } else {
                result.push('{');
                pos += 1;
            }
        } else {
            result.push(chars[pos]);
            pos += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exif_text::get_font;

    fn make_test_tags() -> HashMap<&'static str, String> {
        let mut t = HashMap::new();
        t.insert("make", "FUJIFILM".to_string());
        t.insert("model", "X-T50".to_string());
        t.insert("lens", "XF33mmF1.4".to_string());
        t.insert("fnumber", "1.4".to_string());
        t.insert("shutter", "1/200".to_string());
        t.insert("iso", "125".to_string());
        t.insert("focal", "33mm".to_string());
        t.insert("date", "2026-07-01".to_string());
        t
    }

    #[test]
    fn brand_normalization() {
        assert_eq!(normalize_brand("FUJIFILM"), "FUJIFILM");
        assert_eq!(normalize_brand("SONY"), "SONY");
        assert_eq!(normalize_brand("NIKON CORPORATION"), "NIKON");
        assert_eq!(normalize_brand("Canon"), "Canon");
        assert_eq!(normalize_brand("Apple"), "iPhone");
        assert_eq!(normalize_brand("Unknown Camera"), "Unknown Camera");
    }

    #[test]
    fn apply_expands_canvas_and_paints_border() {
        let photo = RgbImage::from_pixel(800, 600, Rgb([100, 100, 100]));
        let tags = make_test_tags();
        let font = get_font();
        let config = FrameConfig {
            enabled: true,
            border_color: [255, 255, 255],
            border_ratio: 0.02,
            bottom_bar_ratio: 0.12,
            ..Default::default()
        };
        let out = apply(&photo, &config, &tags, font).unwrap();
        assert!(out.width() > photo.width(), "宽度应扩大");
        assert!(out.height() > photo.height(), "高度应扩大（含底部参数条）");
        // 左上角应为纯白（边框）
        let p = out.get_pixel(0, 0);
        assert_eq!(p.0, [255, 255, 255]);
        // 参数条最下方应为白（背景）
        let p2 = out.get_pixel(out.width() - 1, out.height() - 1);
        assert_eq!(p2.0, [255, 255, 255]);
    }

    #[test]
    fn apply_renders_black_border_when_configured() {
        let photo = RgbImage::from_pixel(400, 300, Rgb([200, 200, 200]));
        let tags = make_test_tags();
        let font = get_font();
        let config = FrameConfig {
            enabled: true,
            border_color: [0, 0, 0],
            border_ratio: 0.03,
            ..Default::default()
        };
        let out = apply(&photo, &config, &tags, font).unwrap();
        let p = out.get_pixel(0, 0);
        assert_eq!(p.0, [0, 0, 0]);
    }

    #[test]
    fn format_template_deletes_unknown_keys() {
        // 与 exif_text::format_template 不同，这里未匹配的 key 直接删除
        let tags = HashMap::new();
        let out = format_template("A{unknown}B", &tags);
        assert_eq!(out, "AB");
    }
}
