// 水印合成核心流水线
//
// 输入：源图像字节 + PNG 水印字节 + WatermarkConfig + 可选 EXIF 文字配置
// 输出：合成后的 RGB 像素图 + 原始元数据（EXIF/ICC）
//
// 流水线（compose）：
//   1. 提取源 JPEG 的 EXIF/ICC 段（metadata::extract）
//   2. image crate 解码底图为 RGBA
//   3. image crate 解码水印 PNG 为 RGBA
//   4. 按 size_ratio 缩放水印（Lanczos3 高质量重采样）
//   5a. 按 opacity 调整水印 alpha 通道
//   5b. 应用着色(tint)：把非全透明像素 RGB 替换为目标色
//   6. 平铺模式：旋转 + 网格铺满整图；单点模式：九宫格定位 + alpha 合成
//   7. 可选：叠加 EXIF 文字水印
//   8. 展平为 RgbImage
//   9. 可选：相框模式（白/黑边框 + 底部 EXIF 参数条），作为最终包装
//  10. 可选：画布比例扩展（补白边到目标宽高比）
//  11. 返回 RgbImage + Metadata
//
// 编码已从本模块移除，由 batch.rs 根据用户选择的格式/质量参数完成。

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageReader, RgbImage, RgbaImage};
use imageproc::geometric_transformations::{rotate_about_center_no_crop, Border, Interpolation};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::canvas_expand;
use crate::error::Result;
use crate::exif_text::{self, ExifTextConfig};
use crate::frame;
use crate::metadata::{self, Metadata};
use crate::position::{self, WatermarkConfig};

// —— 平铺水印配置 ——————————————————————————————————————————

/// 全图平铺水印配置：把签名图旋转后按网格铺满整张画布，用于样片防盗。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 旋转角度（度），0-90，默认 30° 斜插防裁切
    #[serde(default = "default_tile_angle")]
    pub angle_deg: f32,
    /// tile 间距，占 tile 自身宽度的比例，默认 0.6
    #[serde(default = "default_tile_gap")]
    pub gap_ratio: f32,
}

fn default_tile_angle() -> f32 {
    30.0
}
fn default_tile_gap() -> f32 {
    0.6
}

impl Default for TileConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            angle_deg: default_tile_angle(),
            gap_ratio: default_tile_gap(),
        }
    }
}

// —— 合成 ————————————————————————————————————————————————

/// 主入口：一次完整的合成流水线。
///
/// 返回 (RgbImage, Metadata)，调用方负责编码和元数据回注。
pub fn compose(
    src_bytes: &[u8],
    watermark_png: &[u8],
    config: &WatermarkConfig,
    exif_text_config: Option<&ExifTextConfig>,
    font: &ab_glyph::FontRef<'static>,
) -> Result<(RgbImage, Metadata)> {
    config.validate()?;

    // 1. 提取源元数据（EXIF/ICC）
    let meta = metadata::extract(src_bytes).unwrap_or_else(|_| Metadata::empty());

    // 2. 解码底图（保留原色彩，无 alpha）
    let base = decode_image(src_bytes)?;
    let (img_w, img_h) = base.dimensions();
    let mut canvas = base.to_rgba8();

    // 3-4. 解码 + 缩放水印
    let watermark = prepare_watermark(watermark_png, img_w, img_h, config)?;
    let (wm_w, wm_h) = watermark.dimensions();

    // 5a. 应用着色（可选）
    let watermark = match config.tint {
        Some(rgb) => apply_tint(watermark, rgb),
        None => watermark,
    };

    // 5b. 应用不透明度
    let watermark = apply_opacity(watermark, config.opacity);

    // 6. 定位/合成（平铺 与 单点九宫格 两种模式）
    match &config.tile {
        Some(tc) if tc.enabled => {
            overlay_tiled(&mut canvas, &watermark, tc);
        }
        _ => {
            let (x, y) = position::compute_position(img_w, img_h, wm_w, wm_h, config)?;
            image::imageops::overlay(&mut canvas, &watermark, x, y);
        }
    }

    // 7. 可选：叠加文字水印（EXIF 或自定义文字）
    // 注意：自定义文字模式不依赖 EXIF，所以即使 meta.exif 为 None 也要调用 render。
    if let Some(etc) = exif_text_config {
        if etc.enabled {
            let raw_exif: &[u8] = meta.exif.as_ref().map(|b| b.as_ref()).unwrap_or(&[]);
            if let Some(text_img) =
                exif_text::render(etc, raw_exif, img_w, img_h, font).unwrap_or(None)
            {
                let (tw, th) = text_img.dimensions();
                // 用 ExifTextConfig 的定位参数构造临时 WatermarkConfig 做位置计算
                let pos_cfg = WatermarkConfig {
                    position: etc.position,
                    size_ratio: 0.0, // 未使用
                    opacity: etc.opacity,
                    margin_x: etc.margin_x,
                    margin_y: etc.margin_y,
                    landscape_override: None,
                    tint: None,
                    exif_text: None,
                    frame: None,
                    tile: None,
                    canvas_ratio: None,
                };
                if let Ok((tx, ty)) =
                    position::compute_position(img_w, img_h, tw, th, &pos_cfg)
                {
                    image::imageops::overlay(&mut canvas, &text_img, tx, ty);
                }
            }
        }
    }

    // 8. 展平为 RGB
    let composed: RgbImage = DynamicImage::ImageRgba8(canvas).to_rgb8();

    // 9. 可选：相框模式（白/黑边框 + 底部 EXIF 参数条），作为最终包装
    let composed = match &config.frame {
        Some(fc) if fc.enabled => {
            let raw_exif: &[u8] = meta.exif.as_ref().map(|b| b.as_ref()).unwrap_or(&[]);
            let tags = exif_text::parse_exif(raw_exif);
            frame::apply(&composed, fc, &tags, font)?
        }
        _ => composed,
    };

    // 10. 可选：画布比例扩展（补白边到目标宽高比），在相框之后作为最终一步
    let composed = match &config.canvas_ratio {
        Some(cr) if cr.enabled => {
            canvas_expand::expand_to_ratio(&composed, cr.ratio_w, cr.ratio_h, cr.fill_color)
        }
        _ => composed,
    };

    Ok((composed, meta))
}

// —— 编码辅助（供 batch.rs 使用） ——————————————————————————

/// 将 RGB 像素图编码为 JPEG 字节流
pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;

    let mut buf = Vec::with_capacity(img.as_raw().len() / 4);
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

/// 将 RGB 像素图编码为 PNG 字节流（无损，quality 参数忽略）
pub fn encode_png(img: &RgbImage) -> Result<Vec<u8>> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;

    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

/// 将 RGB 像素图编码为 WebP 字节流。
/// image 0.25 的 WebPEncoder 仅提供 new_lossless()。
/// 如需有损 WebP，使用 DynamicImage::save 方式。
pub fn encode_webp(img: &RgbImage, _quality: f32) -> Result<Vec<u8>> {
    use image::codecs::webp::WebPEncoder;
    use image::ImageEncoder;

    let mut buf = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut buf);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

/// 缩放图像到指定长边上限（Lanczos3），保持宽高比。
/// 如果 max_long_side 为 None 或原图长边已 <= 限制，返回 None 表示无需缩放。
pub fn maybe_resize(img: &RgbImage, max_long_side: Option<u32>) -> Option<RgbImage> {
    let limit = max_long_side?;
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= limit {
        return None;
    }
    let scale = limit as f32 / long as f32;
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(img, nw, nh, FilterType::Lanczos3);
    Some(resized)
}

// —— 内部辅助 ————————————————————————————————————————————

/// 解码任意（自动嗅探）图像字节到 DynamicImage
fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(image::ImageError::IoError)?;
    Ok(reader.decode()?)
}

/// 读取 PNG 水印并按 size_ratio 缩放到目标像素宽度，保持宽高比。
/// 缩放采用 Lanczos3（高质量），适合摄影师签名图这种高保真需求。
fn prepare_watermark(
    png_bytes: &[u8],
    img_w: u32,
    img_h: u32,
    config: &WatermarkConfig,
) -> Result<RgbaImage> {
    let raw = decode_image(png_bytes)?.to_rgba8();
    let target_w = position::target_watermark_width(img_w, img_h, config.size_ratio);

    let (ow, oh) = raw.dimensions();
    if ow == 0 || oh == 0 {
        return Err(crate::error::WatermarkError::InvalidParam(
            "水印图片尺寸为 0".to_string(),
        ));
    }
    let target_h = ((oh as f32) * (target_w as f32) / (ow as f32)).round() as u32;
    let target_h = target_h.max(1);

    let scaled = image::imageops::resize(&raw, target_w, target_h, FilterType::Lanczos3);
    Ok(scaled)
}

/// 将水印所有可见像素的 RGB 替换为指定颜色（保留 alpha 通道）。
/// 对完全透明的像素（alpha=0）不做处理，避免在 PNG 空区引入色偏。
fn apply_tint(mut wm: RgbaImage, rgb: [u8; 3]) -> RgbaImage {
    for pixel in wm.pixels_mut() {
        if pixel[3] > 0 {
            pixel[0] = rgb[0];
            pixel[1] = rgb[1];
            pixel[2] = rgb[2];
        }
    }
    wm
}

/// 用 opacity 系数缩放水印的 alpha 通道，实现整体透明度调节。
/// opacity=1.0 时不变；opacity=0.5 时所有 alpha 减半。
fn apply_opacity(mut wm: RgbaImage, opacity: f32) -> RgbaImage {
    if (opacity - 1.0).abs() < f32::EPSILON {
        return wm;
    }
    let factor = opacity.clamp(0.0, 1.0);
    for pixel in wm.pixels_mut() {
        let a = pixel[3] as f32 * factor;
        pixel[3] = a.round().clamp(0.0, 255.0) as u8;
    }
    wm
}

/// 全图平铺水印：把水印按 angle_deg 旋转（透明扩边不裁切），
/// 再以 gap_ratio 间距铺满整张画布。
/// 从负一个步长开始遍历，确保旋转后凸出边界的部分也能覆盖画布四角。
fn overlay_tiled(canvas: &mut RgbaImage, watermark: &RgbaImage, tc: &TileConfig) {
    let tile: RgbaImage = if tc.angle_deg.abs() > f32::EPSILON {
        rotate_about_center_no_crop(
            watermark,
            tc.angle_deg.to_radians(),
            Interpolation::Bilinear,
            Border::Constant(image::Rgba([0, 0, 0, 0])),
        )
    } else {
        watermark.clone()
    };

    let (tw, th) = tile.dimensions();
    if tw == 0 || th == 0 {
        return;
    }
    let (cw, ch) = canvas.dimensions();

    let gap = tc.gap_ratio.max(0.0);
    let step_x = ((tw as f32) * (1.0 + gap)).max(1.0) as i64;
    let step_y = ((th as f32) * (1.0 + gap)).max(1.0) as i64;

    let mut y = -step_y;
    while y < ch as i64 {
        let mut x = -step_x;
        while x < cw as i64 {
            image::imageops::overlay(canvas, &tile, x, y);
            x += step_x;
        }
        y += step_y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::GridPosition;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageEncoder, Rgb, Rgba};
    use std::sync::OnceLock;

    /// 全局字体引用（只在测试中惰性初始化一次）
    static TEST_FONT: OnceLock<ab_glyph::FontRef<'static>> = OnceLock::new();

    fn test_font() -> &'static ab_glyph::FontRef<'static> {
        TEST_FONT.get_or_init(|| {
            let data = include_bytes!("../assets/SourceCodePro-Regular.ttf");
            ab_glyph::FontRef::try_from_slice(data).expect("测试字体解析失败")
        })
    }

    /// 生成一张纯色 JPEG 字节流
    fn make_jpeg(w: u32, h: u32, color: Rgb<u8>) -> Vec<u8> {
        let img = RgbImage::from_pixel(w, h, color);
        let mut buf = Vec::new();
        let enc = JpegEncoder::new_with_quality(&mut buf, 95);
        enc.write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        buf
    }

    /// 生成一张纯色 PNG 字节流（RGBA，alpha=255）
    fn make_png(w: u32, h: u32, color: Rgba<u8>) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, color);
        let mut buf = Vec::new();
        image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        buf
    }

    fn cfg(pos: GridPosition, size: f32, opacity: f32) -> WatermarkConfig {
        WatermarkConfig {
            position: pos,
            size_ratio: size,
            opacity,
            margin_x: 0,
            margin_y: 0,
            landscape_override: None,
            tint: None,
            exif_text: None,
            frame: None,
            tile: None,
            canvas_ratio: None,
        }
    }

    /// 辅助：compose + encode JPEG 的便捷组合
    fn compose_and_encode(
        src: &[u8],
        wm: &[u8],
        config: &WatermarkConfig,
    ) -> Result<Vec<u8>> {
        let (img, _meta) = compose(src, wm, config, None, test_font())?;
        encode_jpeg(&img, 95)
    }

    #[test]
    fn opacity_reduces_alpha() {
        let wm = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 200]));
        let out = apply_opacity(wm, 0.5);
        assert_eq!(out.get_pixel(0, 0)[3], 100);
    }

    #[test]
    fn opacity_unchanged_at_full() {
        let wm = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 200]));
        let out = apply_opacity(wm, 1.0);
        assert_eq!(out.get_pixel(0, 0)[3], 200);
    }

    #[test]
    fn opacity_zero_hides_watermark() {
        let wm = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
        let out = apply_opacity(wm, 0.0);
        assert_eq!(out.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn watermark_placed_at_top_left() {
        let base = make_jpeg(200, 200, Rgb([255, 255, 255]));
        let wm_src = make_png(40, 40, Rgba([255, 0, 0, 255]));
        let c = cfg(GridPosition::TopLeft, 0.2, 1.0);

        let out = compose_and_encode(&base, &wm_src, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();

        let p = decoded.get_pixel(5, 5);
        assert!(
            p[0] > 200 && p[1] < 60 && p[2] < 60,
            "(5,5) 期望红色 got {:?}",
            p
        );
        let p = decoded.get_pixel(150, 150);
        assert!(
            p[0] > 240 && p[1] > 240 && p[2] > 240,
            "(150,150) 期望白色 got {:?}",
            p
        );
    }

    #[test]
    fn watermark_placed_at_bottom_right() {
        let base = make_jpeg(200, 200, Rgb([255, 255, 255]));
        let wm_src = make_png(40, 40, Rgba([0, 0, 255, 255]));
        let c = cfg(GridPosition::BottomRight, 0.2, 1.0);

        let out = compose_and_encode(&base, &wm_src, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();

        let p = decoded.get_pixel(195, 195);
        assert!(
            p[2] > 200 && p[0] < 60 && p[1] < 60,
            "(195,195) 期望蓝色 got {:?}",
            p
        );
        let p = decoded.get_pixel(10, 10);
        assert!(
            p[0] > 240 && p[1] > 240 && p[2] > 240,
            "(10,10) 期望白色 got {:?}",
            p
        );
    }

    #[test]
    fn output_is_valid_jpeg() {
        let base = make_jpeg(300, 200, Rgb([100, 150, 200]));
        let wm = make_png(50, 50, Rgba([0, 0, 0, 255]));
        let c = cfg(GridPosition::Center, 0.15, 0.8);

        let out = compose_and_encode(&base, &wm, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!(decoded.dimensions(), (300, 200));
    }

    /// 端到端验证：源 JPEG 带 EXIF/ICC → compose → 输出应仍带完整 EXIF/ICC
    #[test]
    fn end_to_end_preserves_exif_and_icc() {
        use crate::metadata;
        use img_parts::jpeg::Jpeg;
        use img_parts::{Bytes, ImageEXIF, ImageICC};

        let bare = make_jpeg(300, 200, Rgb([100, 150, 200]));

        let src_exif: &[u8] = &[
            0x45, 0x78, 0x69, 0x66, 0x00, 0x00,
            0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00,
            0x01, 0x00,
            0x0E, 0x01, 0x02, 0x00, 0x05, 0x00, 0x00, 0x00,
            b'P', b'H', b'O', b'T', 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        let src_icc: &[u8] = b"FAKE_ICC_PROFILE_FOR_TEST";

        let mut jpeg = Jpeg::from_bytes(Bytes::from(bare)).unwrap();
        jpeg.set_exif(Some(Bytes::from(src_exif.to_vec())));
        jpeg.set_icc_profile(Some(Bytes::from(src_icc.to_vec())));
        let mut src_with_meta = Vec::new();
        jpeg.encoder().write_to(&mut src_with_meta).unwrap();

        let wm = make_png(30, 30, Rgba([255, 0, 0, 200]));
        let c = cfg(GridPosition::BottomRight, 0.1, 0.8);
        let (composed, meta) = compose(&src_with_meta, &wm, &c, None, test_font()).unwrap();
        let encoded = encode_jpeg(&composed, 95).unwrap();
        let output = metadata::inject(encoded, &meta).unwrap();

        let recovered = metadata::extract(&output).unwrap();
        assert!(recovered.exif.is_some(), "输出应包含 EXIF");
        assert!(recovered.icc.is_some(), "输出应包含 ICC");

        let out_exif = recovered.exif.as_ref().unwrap();
        assert!(
            out_exif.windows(4).any(|w| w == b"PHOT"),
            "EXIF 中的 PHOT 标记应保留"
        );

        let out_icc = recovered.icc.as_ref().unwrap();
        assert_eq!(&out_icc[..], src_icc, "ICC profile 应字节级一致");

        let decoded = image::load_from_memory(&output).unwrap();
        assert_eq!(decoded.dimensions(), (300, 200));
    }

    #[test]
    fn tint_replaces_rgb_preserves_alpha() {
        let wm = RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 200]));
        let out = apply_tint(wm, [200, 50, 30]);
        let p = out.get_pixel(0, 0);
        assert_eq!(p[0], 200);
        assert_eq!(p[1], 50);
        assert_eq!(p[2], 30);
        assert_eq!(p[3], 200);
    }

    #[test]
    fn tint_skips_transparent_pixels() {
        let mut wm = RgbaImage::new(2, 1);
        wm.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        wm.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
        let out = apply_tint(wm, [255, 0, 0]);
        assert_eq!(*out.get_pixel(0, 0), Rgba([255, 0, 0, 255]));
        assert_eq!(*out.get_pixel(1, 0), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn watermark_with_tint_shows_tint_color() {
        let base = make_jpeg(100, 100, Rgb([255, 255, 255]));
        let wm = make_png(30, 30, Rgba([255, 255, 255, 255]));
        let mut c = cfg(GridPosition::TopLeft, 0.3, 1.0);
        c.tint = Some([255, 0, 0]);

        let out = compose_and_encode(&base, &wm, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();
        let p = decoded.get_pixel(5, 5);
        assert!(
            p[0] > 200 && p[1] < 60 && p[2] < 60,
            "带红色 tint 的白水印应显示红色, got {:?}",
            p
        );
    }

    #[test]
    fn tiled_watermark_covers_multiple_regions() {
        let base = make_jpeg(400, 400, Rgb([255, 255, 255]));
        let wm = make_png(30, 30, Rgba([255, 0, 0, 255]));
        let mut c = cfg(GridPosition::Center, 0.05, 1.0);
        c.tile = Some(TileConfig {
            enabled: true,
            angle_deg: 20.0,
            gap_ratio: 0.4,
        });

        let out = compose_and_encode(&base, &wm, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();

        // 平铺应覆盖整张画布，四个象限都应能采样到红色像素，而非只集中在单点
        let quadrants = [(50, 50), (350, 50), (50, 350), (350, 350)];
        for (x, y) in quadrants {
            let mut found_red = false;
            for dy in 0..40u32 {
                for dx in 0..40u32 {
                    let p = decoded.get_pixel(
                        (x + dx as i32 - 20).clamp(0, 399) as u32,
                        (y + dy as i32 - 20).clamp(0, 399) as u32,
                    );
                    if p[0] > 180 && p[1] < 100 && p[2] < 100 {
                        found_red = true;
                    }
                }
            }
            assert!(found_red, "象限 ({x},{y}) 附近应有平铺水印像素");
        }
    }

    #[test]
    fn invalid_opacity_rejected() {
        let base = make_jpeg(100, 100, Rgb([255, 255, 255]));
        let wm = make_png(20, 20, Rgba([0, 0, 0, 255]));
        let mut c = cfg(GridPosition::Center, 0.2, 1.5);
        c.opacity = 1.5;
        assert!(compose_and_encode(&base, &wm, &c).is_err());
    }

    #[test]
    fn maybe_resize_noop_for_small_image() {
        let img = RgbImage::from_pixel(100, 50, Rgb([100, 100, 100]));
        let result = maybe_resize(&img, Some(200));
        assert!(result.is_none());
    }

    #[test]
    fn maybe_resize_scales_down() {
        let img = RgbImage::from_pixel(4000, 3000, Rgb([100, 100, 100]));
        let result = maybe_resize(&img, Some(2000));
        assert!(result.is_some());
        let resized = result.unwrap();
        assert_eq!(resized.width(), 2000);
        assert_eq!(resized.height(), 1500);
    }

    #[test]
    fn encode_png_roundtrip() {
        let img = RgbImage::from_pixel(50, 50, Rgb([200, 100, 50]));
        let png_bytes = encode_png(&img).unwrap();
        let decoded = image::load_from_memory(&png_bytes).unwrap();
        assert_eq!(decoded.dimensions(), (50, 50));
    }

    #[test]
    fn encode_webp_roundtrip() {
        let img = RgbImage::from_pixel(50, 50, Rgb([200, 100, 50]));
        let webp_bytes = encode_webp(&img, 95.0).unwrap();
        let decoded = image::load_from_memory(&webp_bytes).unwrap();
        assert_eq!(decoded.dimensions(), (50, 50));
    }

    /// 端到端验证：compose 接入相框模式后，画布按 border/bottom_bar 比例扩大，
    /// 边框色正确，且参数条上确实画出了文字（用字面量模板，绕开 EXIF 解析依赖）。
    #[test]
    fn compose_with_frame_expands_canvas_and_draws_bar() {
        use crate::frame::FrameConfig;

        let base = make_jpeg(400, 300, Rgb([120, 120, 120]));
        let wm = make_png(30, 30, Rgba([255, 0, 0, 255]));
        let mut c = cfg(GridPosition::BottomRight, 0.1, 0.8);
        c.frame = Some(FrameConfig {
            enabled: true,
            border_color: [255, 255, 255],
            border_ratio: 0.02,
            bottom_bar_ratio: 0.12,
            text_color: [0, 0, 0],
            subtext_color: [80, 80, 80],
            left_lines: vec!["HELLO".to_string()],
            right_lines: vec![],
            brand_template: "{brand}".to_string(),
            show_brand: false,
            font_size_ratio: 0.3,
            brand_size_ratio: 0.42,
            show_divider: false,
        });

        let (composed, _meta) = compose(&base, &wm, &c, None, test_font()).unwrap();

        // 短边=300，border=round(300*0.02)=6，bottom_bar=round(300*0.12)=36
        assert_eq!(composed.width(), 400 + 6 * 2, "宽度应含左右边框");
        assert_eq!(composed.height(), 300 + 6 + 36, "高度应含上边框+底部参数条");

        // 左上角应为白色边框
        assert_eq!(composed.get_pixel(0, 0).0, [255, 255, 255]);

        // 参数条左侧区域应存在黑色文字像素（非纯白背景）
        let bar_top = 6 + 300;
        let mut found_dark = false;
        for y in bar_top..composed.height() {
            for x in 6..(6 + 100).min(composed.width()) {
                let px = composed.get_pixel(x, y);
                if px[0] < 100 && px[1] < 100 && px[2] < 100 {
                    found_dark = true;
                }
            }
        }
        assert!(found_dark, "参数条左侧应画出文字像素");
    }

    /// 未启用相框时，compose 输出尺寸应与原图一致（不受 frame 字段存在与否影响）。
    #[test]
    fn compose_without_frame_keeps_original_size() {
        let base = make_jpeg(200, 200, Rgb([255, 255, 255]));
        let wm = make_png(20, 20, Rgba([0, 0, 0, 255]));
        let c = cfg(GridPosition::TopLeft, 0.1, 1.0);
        assert!(c.frame.is_none());

        let (composed, _meta) = compose(&base, &wm, &c, None, test_font()).unwrap();
        assert_eq!(composed.dimensions(), (200, 200));
    }

    /// compose 接入画布比例扩展：400x300 目标 1:1 应补白到 400x400。
    #[test]
    fn compose_with_canvas_ratio_expands_to_target() {
        use crate::canvas_expand::CanvasRatioConfig;

        let base = make_jpeg(400, 300, Rgb([120, 120, 120]));
        let wm = make_png(20, 20, Rgba([255, 0, 0, 255]));
        let mut c = cfg(GridPosition::TopLeft, 0.05, 1.0);
        c.canvas_ratio = Some(CanvasRatioConfig {
            enabled: true,
            ratio_w: 1,
            ratio_h: 1,
            fill_color: [255, 255, 255],
        });

        let (composed, _meta) = compose(&base, &wm, &c, None, test_font()).unwrap();
        assert_eq!(composed.dimensions(), (400, 400));
    }
}
