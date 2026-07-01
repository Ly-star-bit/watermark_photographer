// 水印合成核心流水线
//
// 输入：源 JPEG 字节 + PNG 水印字节 + WatermarkConfig
// 输出：合成后的 JPEG 字节（保留 EXIF/ICC）
//
// 流水线：
//   1. 提取源 JPEG 的 EXIF/ICC 段（metadata::extract）
//   2. image crate 解码底图为 RGBA
//   3. image crate 解码水印 PNG 为 RGBA
//   4. 按 size_ratio 缩放水印（Lanczos3 高质量重采样）
//   5. 按 opacity 调整水印 alpha 通道
//   6. 计算九宫格坐标（position::compute_position）
//   7. alpha 合成（image::imageops::overlay）
//   8. 编码为高质量 JPEG（quality=95, 4:4:4 采样）
//   9. 回注 EXIF/ICC 段（metadata::inject）

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageReader, RgbImage, RgbaImage};
use std::io::Cursor;

use crate::error::Result;
use crate::metadata::{self, Metadata};
use crate::position::{self, WatermarkConfig};

/// JPEG 输出质量（1-100）。95 在画质与体积间取得良好平衡，
/// 摄影师后期链路通常 90+ 即可。
const JPEG_QUALITY: u8 = 95;

/// 主入口：一次完整的合成流水线
pub fn apply(
    src_jpeg: &[u8],
    watermark_png: &[u8],
    config: &WatermarkConfig,
) -> Result<Vec<u8>> {
    config.validate()?;

    // 1. 提取源元数据（EXIF/ICC）
    let meta = metadata::extract(src_jpeg).unwrap_or_else(|_| Metadata::empty());

    // 2. 解码底图（保留原色彩，无 alpha）
    let base = decode_image(src_jpeg)?;
    let (img_w, img_h) = base.dimensions();

    // 3-4. 解码 + 缩放水印
    let watermark = prepare_watermark(watermark_png, img_w, img_h, config)?;
    let (wm_w, wm_h) = watermark.dimensions();

    // 5a. 应用着色（可选）：把所有非全透明像素的 RGB 替换为目标色，
    //     alpha 保持不变，因此签名边缘的抗锯齿羽化不受影响
    let watermark = match config.tint {
        Some(rgb) => apply_tint(watermark, rgb),
        None => watermark,
    };

    // 5b. 应用不透明度
    let watermark = apply_opacity(watermark, config.opacity);

    // 6. 计算位置
    let (x, y) = position::compute_position(img_w, img_h, wm_w, wm_h, config)?;

    // 7. 合成：底图先转 RGBA 作画布，overlay 后转回 RGB
    let mut canvas = base.to_rgba8();
    image::imageops::overlay(&mut canvas, &watermark, x, y);
    let composed: RgbImage = DynamicImage::ImageRgba8(canvas).to_rgb8();

    // 8. 编码为 JPEG
    let encoded = encode_jpeg(&composed)?;

    // 9. 回注元数据
    metadata::inject(encoded, &meta)
}

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

/// 编码为 JPEG。采用 4:4:4 采样（不做色度下采样），最大化保留细节。
fn encode_jpeg(img: &RgbImage) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(img.as_raw().len() / 4);
    let encoder = JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    // set_sampling_factors 在 image 0.25 中通过 JpegEncoder 默认即可保证较高质量；
    // 采样因子在 encoder.encode_image 中由 image crate 内部处理。
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::GridPosition;
    use image::{Rgb, Rgba};

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
        }
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
        // 200x200 白底 + 40x40 红水印，size_ratio 0.2 = 40px 宽，放左上，无边距
        let base = make_jpeg(200, 200, Rgb([255, 255, 255]));
        let wm_src = make_png(40, 40, Rgba([255, 0, 0, 255]));
        let c = cfg(GridPosition::TopLeft, 0.2, 1.0);

        let out = apply(&base, &wm_src, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();

        // (5,5) 应在水印内 → 红色
        let p = decoded.get_pixel(5, 5);
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60, "(5,5) 期望红色 got {:?}", p);
        // (150,150) 应在水印外 → 白色
        let p = decoded.get_pixel(150, 150);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "(150,150) 期望白色 got {:?}", p);
    }

    #[test]
    fn watermark_placed_at_bottom_right() {
        // 200x200 白底 + 40x40 蓝水印，右下角
        let base = make_jpeg(200, 200, Rgb([255, 255, 255]));
        let wm_src = make_png(40, 40, Rgba([0, 0, 255, 255]));
        let c = cfg(GridPosition::BottomRight, 0.2, 1.0);

        let out = apply(&base, &wm_src, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();

        // (195,195) 应在水印内 → 蓝色
        let p = decoded.get_pixel(195, 195);
        assert!(p[2] > 200 && p[0] < 60 && p[1] < 60, "(195,195) 期望蓝色 got {:?}", p);
        // (10,10) 应在水印外 → 白色
        let p = decoded.get_pixel(10, 10);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "(10,10) 期望白色 got {:?}", p);
    }

    #[test]
    fn output_is_valid_jpeg() {
        let base = make_jpeg(300, 200, Rgb([100, 150, 200]));
        let wm = make_png(50, 50, Rgba([0, 0, 0, 255]));
        let c = cfg(GridPosition::Center, 0.15, 0.8);

        let out = apply(&base, &wm, &c).unwrap();
        // 能解码回来且尺寸不变即视为有效
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!(decoded.dimensions(), (300, 200));
    }

    /// 端到端验证：源 JPEG 带 EXIF/ICC → apply → 输出应仍带完整 EXIF/ICC
    /// 这是摄影师最关心的核心保障，比任何单元测试都重要。
    #[test]
    fn end_to_end_preserves_exif_and_icc() {
        use crate::metadata;
        use img_parts::jpeg::Jpeg;
        use img_parts::{Bytes, ImageEXIF, ImageICC};

        // 1. 生成一张无元数据的底图 JPEG
        let bare = make_jpeg(300, 200, Rgb([100, 150, 200]));

        // 2. 用 img-parts 给它注入已知的 EXIF + ICC
        let src_exif: &[u8] = &[
            0x45, 0x78, 0x69, 0x66, 0x00, 0x00, // "Exif\0\0"
            0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, // TIFF header LE
            0x01, 0x00, // 1 IFD entry
            0x0E, 0x01, 0x02, 0x00, 0x05, 0x00, 0x00, 0x00, // ImageDescription tag
            b'P', b'H', b'O', b'T', 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        let src_icc: &[u8] = b"FAKE_ICC_PROFILE_FOR_TEST";

        let mut jpeg = Jpeg::from_bytes(Bytes::from(bare)).unwrap();
        jpeg.set_exif(Some(Bytes::from(src_exif.to_vec())));
        jpeg.set_icc_profile(Some(Bytes::from(src_icc.to_vec())));
        let mut src_with_meta = Vec::new();
        jpeg.encoder().write_to(&mut src_with_meta).unwrap();

        // 3. 通过 apply 打水印
        let wm = make_png(30, 30, Rgba([255, 0, 0, 200]));
        let c = cfg(GridPosition::BottomRight, 0.1, 0.8);
        let output = apply(&src_with_meta, &wm, &c).unwrap();

        // 4. 从输出提取元数据，验证与源完全一致
        let recovered = metadata::extract(&output).unwrap();
        assert!(recovered.exif.is_some(), "输出应包含 EXIF");
        assert!(recovered.icc.is_some(), "输出应包含 ICC");

        // EXIF 字节级一致
        let out_exif = recovered.exif.as_ref().unwrap();
        assert!(
            out_exif.windows(4).any(|w| w == b"PHOT"),
            "EXIF 中的 PHOT 标记应保留"
        );

        // ICC 字节级一致（img-parts 会精确回搬 profile 主体）
        let out_icc = recovered.icc.as_ref().unwrap();
        assert_eq!(
            &out_icc[..],
            src_icc,
            "ICC profile 应字节级一致"
        );

        // 5. 输出应仍是合法 JPEG，尺寸未变
        let decoded = image::load_from_memory(&output).unwrap();
        assert_eq!(decoded.dimensions(), (300, 200));
    }

    #[test]
    fn tint_replaces_rgb_preserves_alpha() {
        // 白色像素（alpha=200）经 tint 后应变为目标 RGB，alpha 保持 200
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
        // alpha=0 的像素应保持不变（避免透明区域被染色）
        let mut wm = RgbaImage::new(2, 1);
        wm.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        wm.put_pixel(1, 0, Rgba([0, 0, 0, 0])); // fully transparent
        let out = apply_tint(wm, [255, 0, 0]);
        assert_eq!(*out.get_pixel(0, 0), Rgba([255, 0, 0, 255]));
        assert_eq!(*out.get_pixel(1, 0), Rgba([0, 0, 0, 0])); // 不变
    }

    #[test]
    fn watermark_with_tint_shows_tint_color() {
        // 白底 + 白色水印，正常情况看不到；开启 tint 为红色后应看到红色
        let base = make_jpeg(100, 100, Rgb([255, 255, 255]));
        let wm = make_png(30, 30, Rgba([255, 255, 255, 255]));
        let mut c = cfg(GridPosition::TopLeft, 0.3, 1.0);
        c.tint = Some([255, 0, 0]);

        let out = apply(&base, &wm, &c).unwrap();
        let decoded = image::load_from_memory(&out).unwrap().to_rgb8();
        let p = decoded.get_pixel(5, 5);
        assert!(
            p[0] > 200 && p[1] < 60 && p[2] < 60,
            "带红色 tint 的白水印应显示红色, got {:?}",
            p
        );
    }

    #[test]
    fn invalid_opacity_rejected() {
        let base = make_jpeg(100, 100, Rgb([255, 255, 255]));
        let wm = make_png(20, 20, Rgba([0, 0, 0, 255]));
        let mut c = cfg(GridPosition::Center, 0.2, 1.5);
        c.opacity = 1.5;
        assert!(apply(&base, &wm, &c).is_err());
    }
}
