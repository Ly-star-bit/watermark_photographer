// EXIF (APP1) 与 ICC (APP2) 段的提取与回注
//
// image crate 重新编码 JPEG 会丢弃所有非图像段（EXIF、ICC、XMP 等）。
// 摄影师最关心的元数据是：
//   - EXIF：相机型号、镜头、光圈、快门、ISO、拍摄时间、GPS
//   - ICC：色彩空间（sRGB / AdobeRGB / DisplayP3）
//
// img-parts 提供了 ImageEXIF / ImageICC trait 直接读写这两类段，无需手动
// 处理 marker 字节和段长度。

use img_parts::jpeg::Jpeg;
use img_parts::{Bytes, ImageEXIF, ImageICC};

use crate::error::Result;

/// 从源 JPEG 字节中提取 EXIF + ICC
#[derive(Debug, Default, Clone)]
pub struct Metadata {
    pub exif: Option<Bytes>,
    pub icc: Option<Bytes>,
}

impl Metadata {
    /// 空元数据（用于源图无 EXIF/ICC 或提取失败时的降级）
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn has_any(&self) -> bool {
        self.exif.is_some() || self.icc.is_some()
    }
}

/// 从源 JPEG 字节提取 EXIF 和 ICC 段
pub fn extract(src_bytes: &[u8]) -> Result<Metadata> {
    let jpeg = Jpeg::from_bytes(Bytes::copy_from_slice(src_bytes))?;
    Ok(Metadata {
        exif: jpeg.exif(),
        icc: jpeg.icc_profile(),
    })
}

/// 将保存的元数据回注入输出 JPEG 字节流
///
/// 输入是水印合成后（未含元数据）的 JPEG 字节，
/// 输出是已注入 EXIF/ICC 段的完整 JPEG。
pub fn inject(encoded_jpeg: Vec<u8>, meta: &Metadata) -> Result<Vec<u8>> {
    if !meta.has_any() {
        // 无元数据可注入，直接返回
        return Ok(encoded_jpeg);
    }

    let mut jpeg = Jpeg::from_bytes(Bytes::from(encoded_jpeg))?;
    if let Some(ref exif) = meta.exif {
        jpeg.set_exif(Some(exif.clone()));
    }
    if let Some(ref icc) = meta.icc {
        jpeg.set_icc_profile(Some(icc.clone()));
    }
    let mut out = Vec::new();
    jpeg.encoder().write_to(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageEncoder, RgbImage};

    /// 生成一张带指定 EXIF/ICC 的测试 JPEG
    fn make_test_jpeg(exif: Option<Vec<u8>>, icc: Option<Vec<u8>>) -> Vec<u8> {
        // 1. 编码一张纯色 JPEG（无元数据）
        let img = RgbImage::from_pixel(50, 50, image::Rgb([120, 120, 120]));
        let mut base = Vec::new();
        JpegEncoder::new_with_quality(&mut base, 95)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();

        // 2. 注入 EXIF/ICC
        let mut jpeg = Jpeg::from_bytes(Bytes::from(base)).unwrap();
        if let Some(e) = exif {
            jpeg.set_exif(Some(Bytes::from(e)));
        }
        if let Some(i) = icc {
            jpeg.set_icc_profile(Some(Bytes::from(i)));
        }
        let mut out = Vec::new();
        jpeg.encoder().write_to(&mut out).unwrap();
        out
    }

    /// 构造最小合法 EXIF 段：TIFF header + 一个 IFD 项
    /// 完整 EXIF 段以 "Exif\0\0" 开头，后跟 TIFF 数据。
    fn dummy_exif() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"Exif\0\0"); // EXIF identifier
        // TIFF header: 小端 + magic 42 + offset to first IFD (8)
        buf.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
        // IFD0: 1 entry
        buf.extend_from_slice(&[0x01, 0x00]);
        // Entry: tag 0x010E (ImageDescription), type 2 (ASCII), count 5, value inline
        buf.extend_from_slice(&[
            0x0E, 0x01, // tag
            0x02, 0x00, // type
            0x05, 0x00, 0x00, 0x00, // count = 5
            b'T', b'E', b'S', b'T', 0x00, // value "TEST\0" (5 bytes inline)
        ]);
        // Next IFD offset = 0
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        buf
    }

    /// 构造一段假的 ICC profile（内容任意，只测试字节级往返）
    fn dummy_icc() -> Vec<u8> {
        b"ICC_TEST_PROFILE_BYTES_SIMULATION_1234567890".to_vec()
    }

    #[test]
    fn extract_from_bare_jpeg_returns_empty() {
        let bare = make_test_jpeg(None, None);
        let m = extract(&bare).unwrap();
        assert!(!m.has_any());
    }

    #[test]
    fn extract_exif_roundtrip() {
        let src = make_test_jpeg(Some(dummy_exif()), None);
        let m = extract(&src).unwrap();
        assert!(m.exif.is_some());
        assert!(m.icc.is_none());

        // extract 出来的 EXIF 字节应包含我们注入的标识
        let exif_bytes = m.exif.as_ref().unwrap();
        let contains_test = exif_bytes.windows(4).any(|w| w == b"TEST");
        assert!(contains_test, "提取的 EXIF 应包含 TEST 标记");
    }

    #[test]
    fn extract_icc_roundtrip() {
        let src = make_test_jpeg(None, Some(dummy_icc()));
        let m = extract(&src).unwrap();
        assert!(m.icc.is_some());
        let icc_bytes = m.icc.as_ref().unwrap();
        // 注入的 ICC 内容应可完整提取
        assert!(
            icc_bytes.windows(8).any(|w| w == b"ICC_TEST"),
            "提取的 ICC 应包含 ICC_TEST 标记"
        );
    }

    #[test]
    fn inject_preserves_exif_bytes() {
        // 完整链路：源 → extract → 生成新 JPEG（不带元数据） → inject → extract → 比对
        let src_exif = dummy_exif();
        let src = make_test_jpeg(Some(src_exif.clone()), None);
        let meta = extract(&src).unwrap();

        // 模拟"水印合成"输出：新 JPEG 无元数据
        let bare = make_test_jpeg(None, None);
        let with_meta = inject(bare, &meta).unwrap();

        let recovered = extract(&with_meta).unwrap();
        assert_eq!(
            recovered.exif.as_ref().map(|b| b.as_ref()),
            meta.exif.as_ref().map(|b| b.as_ref()),
            "回注后 EXIF 字节应完全一致"
        );
    }

    #[test]
    fn inject_preserves_icc_bytes() {
        let src = make_test_jpeg(None, Some(dummy_icc()));
        let meta = extract(&src).unwrap();

        let bare = make_test_jpeg(None, None);
        let with_meta = inject(bare, &meta).unwrap();

        let recovered = extract(&with_meta).unwrap();
        assert_eq!(
            recovered.icc.as_ref().map(|b| b.as_ref()),
            meta.icc.as_ref().map(|b| b.as_ref()),
            "回注后 ICC 字节应完全一致"
        );
    }

    #[test]
    fn inject_preserves_both_exif_and_icc() {
        let src = make_test_jpeg(Some(dummy_exif()), Some(dummy_icc()));
        let meta = extract(&src).unwrap();
        assert!(meta.exif.is_some() && meta.icc.is_some());

        let bare = make_test_jpeg(None, None);
        let with_meta = inject(bare, &meta).unwrap();

        let recovered = extract(&with_meta).unwrap();
        assert_eq!(recovered.exif, meta.exif, "EXIF 一致");
        assert_eq!(recovered.icc, meta.icc, "ICC 一致");
    }

    #[test]
    fn inject_empty_metadata_is_noop() {
        let bare = make_test_jpeg(None, None);
        let expected_len = bare.len();
        let out = inject(bare, &Metadata::empty()).unwrap();
        assert_eq!(out.len(), expected_len);
    }
}
