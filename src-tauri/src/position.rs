// 九宫格 + 横竖构图适配
//
// 设计约束（来自计划文档，与前端 Canvas 预览必须数学一致）：
// 1. 水印宽度以图片"短边"作为基准 * size_ratio，保证横竖构图视觉尺寸相当
// 2. 九宫格锚点 + 边距偏移决定水印像素坐标
// 3. landscape_override 允许横构图使用与竖构图不同的锚点/边距（可选）

use serde::{Deserialize, Serialize};

use crate::canvas_expand::CanvasRatioConfig;
use crate::exif_text::ExifTextConfig;
use crate::frame::FrameConfig;
use crate::watermark::TileConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GridPosition {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    #[default]
    Center,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkConfig {
    /// 位置锚点（竖构图默认，或未指定 landscape_override 时也用于横构图）
    pub position: GridPosition,
    /// 水印宽度占"短边"的比例（0.0-1.0），推荐 0.10-0.25
    pub size_ratio: f32,
    /// 不透明度 0.0-1.0
    pub opacity: f32,
    /// 水平边距（像素），从对应锚点向内偏移
    pub margin_x: u32,
    /// 垂直边距（像素）
    pub margin_y: u32,
    /// 可选：横构图使用不同的锚点（例如竖图放右下角、横图放左下角）
    #[serde(default)]
    pub landscape_override: Option<GridPosition>,
    /// 可选着色：Some([r,g,b]) 时把水印所有不透明像素替换为该颜色（保留 alpha 边缘）。
    /// None 表示使用签名图原色。
    /// 用途：当签名 PNG 是白色而底图偏亮时切换成深色避免融合。
    #[serde(default)]
    pub tint: Option<[u8; 3]>,
    /// 可选：EXIF 文字水印配置
    #[serde(default)]
    pub exif_text: Option<ExifTextConfig>,
    /// 可选：相框模式（白/黑边框 + 底部参数条）
    #[serde(default)]
    pub frame: Option<FrameConfig>,
    /// 可选：全图平铺水印（防盗样片模式）
    #[serde(default)]
    pub tile: Option<TileConfig>,
    /// 可选：画布比例扩展（补白边到目标宽高比）
    #[serde(default)]
    pub canvas_ratio: Option<CanvasRatioConfig>,
}

impl WatermarkConfig {
    /// 参数校验
    pub fn validate(&self) -> crate::error::Result<()> {
        if !(0.01..=1.0).contains(&self.size_ratio) {
            return Err(crate::error::WatermarkError::InvalidParam(format!(
                "size_ratio 必须在 [0.01, 1.0] 之间，实际 {}",
                self.size_ratio
            )));
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(crate::error::WatermarkError::InvalidParam(format!(
                "opacity 必须在 [0.0, 1.0] 之间，实际 {}",
                self.opacity
            )));
        }
        Ok(())
    }
}

/// 判断是否为横构图（宽 > 高）。方图归入横构图。
#[inline]
pub fn is_landscape(width: u32, height: u32) -> bool {
    width >= height
}

/// 计算尺寸基准：使用短边，保证横竖构图水印视觉大小接近
#[inline]
pub fn scale_base(width: u32, height: u32) -> u32 {
    width.min(height)
}

/// 根据 size_ratio 计算目标水印宽度（像素）
pub fn target_watermark_width(img_w: u32, img_h: u32, size_ratio: f32) -> u32 {
    let base = scale_base(img_w, img_h) as f32;
    ((base * size_ratio).round() as u32).max(1)
}

/// 根据水印尺寸（缩放后）和图片尺寸、位置配置，计算水印左上角像素坐标
///
/// 若水印超出图片边界，坐标会被 clamp 到有效范围内。
/// 若水印本身大于图片，返回 WatermarkError::WatermarkTooLarge。
pub fn compute_position(
    img_w: u32,
    img_h: u32,
    wm_w: u32,
    wm_h: u32,
    config: &WatermarkConfig,
) -> crate::error::Result<(i64, i64)> {
    if wm_w > img_w || wm_h > img_h {
        return Err(crate::error::WatermarkError::WatermarkTooLarge {
            img_w,
            img_h,
            wm_w,
            wm_h,
        });
    }

    // 选择实际使用的锚点：横构图且有 override 时使用 override
    let anchor = if is_landscape(img_w, img_h) {
        config.landscape_override.unwrap_or(config.position)
    } else {
        config.position
    };

    let mx = config.margin_x as i64;
    let my = config.margin_y as i64;
    let iw = img_w as i64;
    let ih = img_h as i64;
    let ww = wm_w as i64;
    let wh = wm_h as i64;

    let (x, y) = match anchor {
        GridPosition::TopLeft => (mx, my),
        GridPosition::TopCenter => ((iw - ww) / 2, my),
        GridPosition::TopRight => (iw - ww - mx, my),
        GridPosition::MiddleLeft => (mx, (ih - wh) / 2),
        GridPosition::Center => ((iw - ww) / 2, (ih - wh) / 2),
        GridPosition::MiddleRight => (iw - ww - mx, (ih - wh) / 2),
        GridPosition::BottomLeft => (mx, ih - wh - my),
        GridPosition::BottomCenter => ((iw - ww) / 2, ih - wh - my),
        GridPosition::BottomRight => (iw - ww - mx, ih - wh - my),
    };

    // Clamp 到 [0, img - wm]，防止边距过大导致越界
    let x = x.clamp(0, iw - ww);
    let y = y.clamp(0, ih - wh);
    Ok((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pos: GridPosition, margin: u32) -> WatermarkConfig {
        WatermarkConfig {
            position: pos,
            size_ratio: 0.15,
            opacity: 0.8,
            margin_x: margin,
            margin_y: margin,
            landscape_override: None,
            tint: None,
            exif_text: None,
            frame: None,
            tile: None,
            canvas_ratio: None,
        }
    }

    #[test]
    fn landscape_detection() {
        assert!(is_landscape(3000, 2000));
        assert!(is_landscape(1000, 1000));
        assert!(!is_landscape(2000, 3000));
    }

    #[test]
    fn short_side_scale_base() {
        assert_eq!(scale_base(3000, 2000), 2000);
        assert_eq!(scale_base(2000, 3000), 2000);
        assert_eq!(
            target_watermark_width(3000, 2000, 0.15),
            target_watermark_width(2000, 3000, 0.15)
        );
    }

    #[test]
    fn target_width_by_ratio() {
        assert_eq!(target_watermark_width(2000, 3000, 0.15), 300);
        assert_eq!(target_watermark_width(4000, 6000, 0.10), 400);
        assert_eq!(target_watermark_width(1000, 1000, 0.0001), 1);
    }

    #[test]
    fn nine_grid_landscape_all_positions() {
        let (iw, ih) = (1000u32, 600u32);
        let (ww, wh) = (100u32, 50u32);
        let m = 20u32;

        let cases = [
            (GridPosition::TopLeft, (20, 20)),
            (GridPosition::TopCenter, (450, 20)),
            (GridPosition::TopRight, (880, 20)),
            (GridPosition::MiddleLeft, (20, 275)),
            (GridPosition::Center, (450, 275)),
            (GridPosition::MiddleRight, (880, 275)),
            (GridPosition::BottomLeft, (20, 530)),
            (GridPosition::BottomCenter, (450, 530)),
            (GridPosition::BottomRight, (880, 530)),
        ];

        for (pos, expected) in cases {
            let c = cfg(pos, m);
            let (x, y) = compute_position(iw, ih, ww, wh, &c).unwrap();
            assert_eq!(
                (x, y),
                (expected.0 as i64, expected.1 as i64),
                "position {:?}",
                pos
            );
        }
    }

    #[test]
    fn nine_grid_portrait_bottom_right() {
        let c = cfg(GridPosition::BottomRight, 30);
        let (x, y) = compute_position(2000, 3000, 300, 100, &c).unwrap();
        assert_eq!((x, y), (2000 - 300 - 30, 3000 - 100 - 30));
    }

    #[test]
    fn landscape_override_applied() {
        let mut c = cfg(GridPosition::BottomRight, 20);
        c.landscape_override = Some(GridPosition::BottomLeft);

        // portrait: uses position (bottom right)
        let (x, _y) = compute_position(1000, 2000, 100, 50, &c).unwrap();
        assert_eq!(x, 1000 - 100 - 20);

        // landscape: uses override (bottom left)
        let (x, _y) = compute_position(2000, 1000, 100, 50, &c).unwrap();
        assert_eq!(x, 20);
    }

    #[test]
    fn watermark_too_large_errors() {
        let c = cfg(GridPosition::Center, 0);
        let err = compute_position(100, 100, 200, 50, &c).unwrap_err();
        match err {
            crate::error::WatermarkError::WatermarkTooLarge { .. } => {}
            _ => panic!("expected WatermarkTooLarge"),
        }
    }

    #[test]
    fn config_validation() {
        let mut c = cfg(GridPosition::Center, 0);
        c.opacity = 1.5;
        assert!(c.validate().is_err());
        c.opacity = 0.5;
        c.size_ratio = 0.0;
        assert!(c.validate().is_err());
        c.size_ratio = 0.15;
        assert!(c.validate().is_ok());
    }
}
