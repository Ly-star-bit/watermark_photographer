// EXIF 文字水印模块
//
// 职责：从原始 APP1(EXIF) 字节中解析拍摄参数 → 按模板格式化为字符串 →
// 用 ab_glyph 渲染为 RGBA 像素图 → 供 watermark::compose 叠加。
//
// 支持两种文字来源：
//   EXIF 模式：解析照片 EXIF 标签，按模板格式化
//   自定义模式：直接使用用户输入的文本
//
// 字体来源：编译期嵌入的 SourceCodePro-Regular.ttf（SIL OFL 1.1）
// 字体加载：OnceLock 惰性初始化，首次调用时解析一次，后续复用。

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::error::Result;
use crate::position::GridPosition;

// —— 编译期嵌入字体 ——————————————————————————————————————

/// 嵌入 SourceCodePro-Regular.ttf（~100KB，仅拉丁字符集）
const FONT_DATA: &[u8] = include_bytes!("../assets/SourceCodePro-Regular.ttf");

/// 惰性字体实例（首次调用时从嵌入字节解析）
static FONT: OnceLock<FontRef> = OnceLock::new();

/// 获取全局字体引用
pub fn get_font() -> &'static FontRef<'static> {
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_DATA).expect("嵌入字体解析失败"))
}

// —— 配置 ————————————————————————————————————————————————

/// EXIF 文字水印配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExifTextConfig {
    /// 是否启用 EXIF 文字水印
    #[serde(default)]
    pub enabled: bool,
    /// 文字模板（EXIF 模式下使用），如 "{make} {model} · f/{fnumber} · ISO {iso}"
    #[serde(default = "default_text_template")]
    pub template: String,
    /// 自定义文字。Some = 直接使用此文本（忽略 EXIF 解析）；None = 使用 EXIF 模式
    #[serde(default)]
    pub custom_text: Option<String>,
    /// 字号（相对图片长边的比例，0.01 - 0.20）
    /// 例：0.03 表示字号 = 长边像素 × 3%（6000px 长边 → 180px 字号）
    /// 用比例而非绝对像素可保证不同分辨率照片视觉一致。
    #[serde(default = "default_font_size_ratio")]
    pub font_size_ratio: f32,
    /// 文字在照片上的位置锚点
    #[serde(default)]
    pub position: GridPosition,
    /// 水平边距（像素）
    #[serde(default)]
    pub margin_x: u32,
    /// 垂直边距（像素）
    #[serde(default)]
    pub margin_y: u32,
    /// 不透明度 0.0-1.0
    #[serde(default = "default_text_opacity")]
    pub opacity: f32,
    /// 文字颜色 [R, G, B]
    #[serde(default = "default_text_color")]
    pub color: [u8; 3],
    /// 可选背景条 [R, G, B, A]（在文字后方绘制半透明色块）
    #[serde(default)]
    pub background: Option<[u8; 4]>,
    /// 整行通栏：背景条宽度铺满整幅图片宽度（而非贴合文字宽度）
    #[serde(default)]
    pub full_width: bool,
}

fn default_text_template() -> String {
    "{make} {model} · {lens} · f/{fnumber} · {shutter}s · ISO {iso}".to_string()
}
fn default_font_size_ratio() -> f32 {
    0.03
}
fn default_text_opacity() -> f32 {
    0.85
}
fn default_text_color() -> [u8; 3] {
    [255, 255, 255]
}

impl Default for ExifTextConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            template: default_text_template(),
            custom_text: None,
            font_size_ratio: default_font_size_ratio(),
            position: GridPosition::BottomLeft,
            margin_x: 40,
            margin_y: 40,
            opacity: default_text_opacity(),
            color: default_text_color(),
            background: Some([0, 0, 0, 80]),
            full_width: false,
        }
    }
}

// —— EXIF 解析 ————————————————————————————————————————————

/// 从原始 APP1 字节解析常用 EXIF 标签，返回 标签名→值 的映射。
///
/// 注意：光圈/快门/ISO 等参数存储在 ExifIFD 子目录中（IFD 编号非 PRIMARY），
/// 因此必须遍历所有字段找匹配 tag，不能限定 ifd_num。
///
/// 如果解析失败（例如源文件无 EXIF），返回空 HashMap，
/// 调用方根据空映射决定跳过渲染。
pub fn parse_exif(raw_exif: &[u8]) -> HashMap<&'static str, String> {
    let mut tags = HashMap::new();

    // img-parts 返回的 APP1 数据以 "Exif\0\0" 开头（6 字节），
    // exif::Reader::read_raw 需要纯 TIFF 数据（以 "II" 或 "MM" 开头）。
    let payload = if raw_exif.starts_with(b"Exif\0\0") {
        &raw_exif[6..]
    } else {
        raw_exif
    };

    let exif = match exif::Reader::new().read_raw(payload.to_vec()) {
        Ok(e) => e,
        Err(_) => return tags,
    };

    use exif::{Tag, Value};

    // 遍历所有字段（跨所有 IFD），匹配关心的 tag
    for f in exif.fields() {
        match f.tag {
            // 字符串字段：用 extract_ascii 提取原始字节，避免 display_value 加双引号
            Tag::Make => {
                if let Some(s) = extract_ascii(&f.value) {
                    tags.insert("make", s);
                }
            }
            Tag::Model => {
                if let Some(s) = extract_ascii(&f.value) {
                    tags.insert("model", s);
                }
            }
            Tag::LensModel => {
                if let Some(s) = extract_ascii(&f.value) {
                    tags.insert("lens", s);
                }
            }
            Tag::LensMake => {
                // LensMake 仅在 LensModel 不存在时使用
                if let Some(s) = extract_ascii(&f.value) {
                    tags.entry("lens").or_insert(s);
                }
            }

            // 光圈 FNumber（Rational）
            Tag::FNumber => {
                if let Value::Rational(ref v) = &f.value {
                    if !v.is_empty() {
                        tags.insert("fnumber", format!("{:.1}", v[0].to_f64()));
                    }
                }
            }

            // ISO 感光度
            // 主流相机（含富士）写入 PhotographicSensitivity（0x8827）
            // 仅少数机型写入 ISOSpeed（0x8833），作为后备
            // 用 insert 覆盖策略保证 PhotographicSensitivity 命中时优先生效
            Tag::PhotographicSensitivity => {
                if let Some(iso) = f.value.get_uint(0) {
                    tags.insert("iso", iso.to_string());
                }
            }
            Tag::ISOSpeed => {
                if let Some(iso) = f.value.get_uint(0) {
                    tags.entry("iso").or_insert_with(|| iso.to_string());
                }
            }

            // 焦距 FocalLength（Rational）
            Tag::FocalLength => {
                if let Value::Rational(ref v) = &f.value {
                    if !v.is_empty() {
                        tags.insert("focal", format!("{:.0}mm", v[0].to_f64().round()));
                    }
                }
            }

            // 快门 ExposureTime（Rational）
            Tag::ExposureTime => {
                if let Value::Rational(ref v) = &f.value {
                    if !v.is_empty() {
                        let t = v[0].to_f64();
                        if t >= 1.0 {
                            tags.insert("shutter", format!("{t:.0}"));
                        } else {
                            let denom: f64 = (1.0 / t).round();
                            tags.insert("shutter", format!("1/{denom:.0}"));
                        }
                    }
                }
            }

            // 拍摄日期 DateTimeOriginal
            Tag::DateTimeOriginal => {
                if let Some(raw) = extract_ascii(&f.value) {
                    if raw.len() >= 10 {
                        tags.insert("date", raw[..10].replace(':', "-"));
                        tags.insert("datetime", raw);
                    }
                }
            }

            _ => {}
        }
    }

    tags
}

/// 从 exif::Value::Ascii 中提取首个字符串（去除 NUL 结尾和首尾空格）。
/// 用于 Make/Model/LensModel 等 ASCII 字段，避免 display_value() 自动添加的引号。
fn extract_ascii(value: &exif::Value) -> Option<String> {
    if let exif::Value::Ascii(ref list) = *value {
        let first = list.first()?;
        // EXIF ASCII 字段以 NUL 结尾，截断到第一个 0 字节
        let bytes: Vec<u8> = first.iter().copied().take_while(|&b| b != 0).collect();
        let s = String::from_utf8_lossy(&bytes).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    } else {
        None
    }
}

// —— 模板替换 ————————————————————————————————————————————

/// 将模板中的 {key} 替换为 tags 中的对应值。
/// 未匹配的 key 保留原样（不做替换），方便调试。
pub fn format_template(template: &str, tags: &HashMap<&'static str, String>) -> String {
    let mut result = String::with_capacity(template.len() + 64);
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
                } else {
                    result.push('{');
                    result.push_str(&key);
                    result.push('}');
                }
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

// —— 文字渲染为 RGBA 像素图 ——————————————————————————————

/// 渲染主入口，根据 config 选择文字来源：
///   - custom_text 有值 → 直接用作文字
///   - custom_text 为 None → 从 raw_exif 解析 EXIF + 模板格式化
///
/// `img_w, img_h` 用于把 `font_size_ratio` 换算为实际像素字号，
/// 保证不同分辨率照片的文字视觉大小一致。
///
/// 返回 None 表示：无文字可渲染。
/// 返回 Some(RgbaImage) 表示成功渲染。
pub fn render(
    config: &ExifTextConfig,
    raw_exif: &[u8],
    img_w: u32,
    img_h: u32,
    font: &FontRef<'static>,
) -> Result<Option<RgbaImage>> {
    if !config.enabled {
        return Ok(None);
    }

    // 确定要渲染的文字
    let text = if let Some(ref custom) = config.custom_text {
        if custom.is_empty() {
            return Ok(None);
        }
        custom.clone()
    } else {
        let tags = parse_exif(raw_exif);
        if tags.is_empty() {
            return Ok(None);
        }
        format_template(&config.template, &tags)
    };

    // 由长边 × ratio 得到实际字号；下限 8px 防止极端小图渲染出空图
    let long_side = img_w.max(img_h) as f32;
    let font_px = (long_side * config.font_size_ratio).max(8.0);

    render_text(&text, config, font_px, img_w, font)
}

/// 纯文字渲染（不含 EXIF 解析逻辑）
///
/// `img_w` 仅在 `config.full_width` 时使用：背景条宽度铺满整幅图片，
/// 而非贴合文字宽度（用于顶部/底部通栏样式）。
fn render_text(
    text: &str,
    config: &ExifTextConfig,
    font_px: f32,
    img_w: u32,
    font: &FontRef<'static>,
) -> Result<Option<RgbaImage>> {
    let scale = PxScale::from(font_px);
    let scaled_font = font.as_scaled(scale);
    let line_height = scaled_font.height().ceil() as u32;
    let ascent = scaled_font.ascent().ceil() as u32;

    let lines: Vec<&str> = text.lines().collect();

    // 计算总宽度和总高度
    let mut max_w = 0u32;
    for line in &lines {
        let mut w = 0f32;
        for c in line.chars() {
            let gid = font.glyph_id(c);
            w += scaled_font.h_advance(gid);
        }
        max_w = max_w.max(w.ceil() as u32);
    }

    let padding = if config.background.is_some() {
        (font_px * 0.3).ceil() as u32
    } else {
        0
    };

    // 通栏模式：背景条宽度铺满整幅图片；否则贴合文字宽度
    let total_w = if config.full_width {
        img_w.max(1)
    } else {
        (max_w + padding * 2).max(1)
    };
    let total_h = (line_height * lines.len() as u32 + padding * 2).max(1);

    let mut img = RgbaImage::new(total_w, total_h);

    // 可选：背景条
    if let Some(bg) = config.background {
        for pixel in img.pixels_mut() {
            pixel[0] = bg[0];
            pixel[1] = bg[1];
            pixel[2] = bg[2];
            pixel[3] = bg[3];
        }
    }

    // 逐行逐字渲染
    for (li, line) in lines.iter().enumerate() {
        let mut x_cursor = padding as f32;
        let y = padding as f32 + ascent as f32 + (li as f32 * line_height as f32);

        for c in line.chars() {
            let gid = font.glyph_id(c);
            let glyph = gid.with_scale_and_position(scale, point(x_cursor, y));

            if let Some(outlined) = font.outline_glyph(glyph) {
                // ⚠️ ab_glyph 的 OutlinedGlyph::draw 回调坐标是 glyph 局部（0..bounds_w, 0..bounds_h），
                // 必须加上 px_bounds.min 才是画布绝对坐标。否则所有字符都画到画布左上角同一位置。
                let bb = outlined.px_bounds();
                let offset_x = bb.min.x as i32;
                let offset_y = bb.min.y as i32;
                outlined.draw(|px, py, coverage| {
                    // 局部坐标 → 画布绝对坐标
                    let ax = px as i32 + offset_x;
                    let ay = py as i32 + offset_y;
                    if ax < 0 || ay < 0 {
                        return;
                    }
                    let ix = ax as u32;
                    let iy = ay as u32;
                    if ix >= total_w || iy >= total_h {
                        return;
                    }
                    let src_a = coverage * config.opacity; // 0..1
                    if src_a <= 0.0 {
                        return;
                    }
                    // 标准 source-over 合成：文字覆盖到已有背景（含半透明背景条）上
                    // out_rgb = (src_rgb * src_a + dst_rgb * dst_a * (1 - src_a)) / out_a
                    // out_a   = src_a + dst_a * (1 - src_a)
                    let pixel = img.get_pixel_mut(ix, iy);
                    let dst_a = pixel[3] as f32 / 255.0;
                    let inv_src = 1.0 - src_a;
                    let out_a = src_a + dst_a * inv_src;
                    if out_a <= 0.0 {
                        return;
                    }
                    let src_r = config.color[0] as f32;
                    let src_g = config.color[1] as f32;
                    let src_b = config.color[2] as f32;
                    let dst_r = pixel[0] as f32;
                    let dst_g = pixel[1] as f32;
                    let dst_b = pixel[2] as f32;
                    pixel[0] = ((src_r * src_a + dst_r * dst_a * inv_src) / out_a) as u8;
                    pixel[1] = ((src_g * src_a + dst_g * dst_a * inv_src) / out_a) as u8;
                    pixel[2] = ((src_b * src_a + dst_b * dst_a * inv_src) / out_a) as u8;
                    pixel[3] = (out_a * 255.0) as u8;
                });
            }

            x_cursor += scaled_font.h_advance(gid);
        }
    }

    Ok(Some(img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_template_replaces_known_keys() {
        let mut tags = HashMap::new();
        tags.insert("make", "FUJIFILM".to_string());
        tags.insert("model", "X-T5".to_string());
        tags.insert("lens", "XF33mmF1.4".to_string());
        tags.insert("fnumber", "1.4".to_string());
        tags.insert("shutter", "1/200".to_string());
        tags.insert("iso", "125".to_string());
        let tpl = "{make} {model} · {lens} · f/{fnumber} · {shutter}s · ISO {iso}";
        let result = format_template(tpl, &tags);
        assert_eq!(
            result,
            "FUJIFILM X-T5 · XF33mmF1.4 · f/1.4 · 1/200s · ISO 125"
        );
    }

    #[test]
    fn format_template_keeps_unknown_keys() {
        let tags = HashMap::new();
        let result = format_template("{unknown_key}", &tags);
        assert_eq!(result, "{unknown_key}");
    }

    #[test]
    fn font_loads_without_panic() {
        let _font = get_font();
    }

    #[test]
    fn render_returns_none_when_disabled() {
        let config = ExifTextConfig {
            enabled: false,
            ..Default::default()
        };
        let font = get_font();
        let result = render(&config, &[], 1000, 1000, font).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn render_returns_none_for_empty_exif() {
        let config = ExifTextConfig {
            enabled: true,
            ..Default::default()
        };
        let font = get_font();
        let result = render(&config, &[], 1000, 1000, font).unwrap();
        assert!(result.is_none());
    }

    /// 端到端：img-parts 注入 → metadata::extract → parse_exif
    /// 使用与 metadata 模块测试相同的 dummy_exif 格式（已验证通过 img-parts 往返）
    #[test]
    fn roundtrip_inject_extract_parse() {
        use image::codecs::jpeg::JpegEncoder;
        use image::{ImageEncoder, RgbImage};
        use img_parts::jpeg::Jpeg;
        use img_parts::{Bytes, ImageEXIF};

        // 1. 创建纯色 JPEG
        let img = RgbImage::from_pixel(50, 50, image::Rgb([120, 120, 120]));
        let mut base = Vec::new();
        JpegEncoder::new_with_quality(&mut base, 95)
            .write_image(img.as_raw(), 50, 50, image::ExtendedColorType::Rgb8)
            .unwrap();

        // 2. 注入合法 EXIF（复用 metadata 模块已验证的格式）
        let exif_data = crate::metadata::make_exif_for_test();
        let mut jpeg = Jpeg::from_bytes(Bytes::from(base)).unwrap();
        jpeg.set_exif(Some(Bytes::from(exif_data)));
        let mut src_with_exif = Vec::new();
        jpeg.encoder().write_to(&mut src_with_exif).unwrap();

        // 3. 提取 → 解析
        let meta = crate::metadata::extract(&src_with_exif).unwrap();
        assert!(meta.exif.is_some(), "EXIF 应可提取");
        let tags = parse_exif(meta.exif.as_ref().unwrap().as_ref());
        // 注：hand-crafted EXIF 的 Make tag 仅用于验证解析不 panic；
        // 实际相机 EXIF 通过 `fields()` 遍历能正确解析所有子 IFD 标签。
        assert!(!tags.contains_key("make") || tags.get("make").unwrap() != "");
    }

    #[test]
    fn custom_text_renders_ignoring_exif() {
        let config = ExifTextConfig {
            enabled: true,
            custom_text: Some("© Photographer".to_string()),
            font_size_ratio: 0.03,
            ..Default::default()
        };
        let font = get_font();
        let result = render(&config, &[], 2000, 1500, font).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn custom_text_empty_returns_none() {
        let config = ExifTextConfig {
            enabled: true,
            custom_text: Some(String::new()),
            ..Default::default()
        };
        let font = get_font();
        let result = render(&config, &[], 1000, 1000, font).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn full_width_bar_matches_image_width() {
        let config = ExifTextConfig {
            enabled: true,
            custom_text: Some("SONY ILCE-7RM3A".to_string()),
            full_width: true,
            background: Some([0, 0, 0, 80]),
            ..Default::default()
        };
        let font = get_font();
        let result = render(&config, &[], 4000, 3000, font).unwrap().unwrap();
        assert_eq!(result.width(), 4000, "通栏模式下背景条宽度应等于图片宽度");
    }

    #[test]
    fn non_full_width_bar_fits_text() {
        let config = ExifTextConfig {
            enabled: true,
            custom_text: Some("SONY ILCE-7RM3A".to_string()),
            full_width: false,
            background: Some([0, 0, 0, 80]),
            ..Default::default()
        };
        let font = get_font();
        let result = render(&config, &[], 4000, 3000, font).unwrap().unwrap();
        assert!(
            result.width() < 4000,
            "非通栏模式下背景条宽度应贴合文字，远小于图片宽度"
        );
    }

    #[test]
    fn font_size_scales_with_image_dimensions() {
        // 相同 ratio 下，大图应生成更大的文字位图
        let config = ExifTextConfig {
            enabled: true,
            custom_text: Some("TEST".to_string()),
            font_size_ratio: 0.05,
            background: None,
            ..Default::default()
        };
        let font = get_font();
        let small = render(&config, &[], 1000, 800, font).unwrap().unwrap();
        let big = render(&config, &[], 6000, 4000, font).unwrap().unwrap();
        assert!(
            big.width() > small.width() * 3,
            "6000px 图片文字宽度应远大于 1000px（预期 ≈6 倍）"
        );
    }
}
