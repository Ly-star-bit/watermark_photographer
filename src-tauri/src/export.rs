// 导出控制选项：格式、质量、长边缩放
//
// 这些属于"导出时"参数（非水印风格配置），因此不保存在预设中。

use serde::{Deserialize, Serialize};

/// 导出时输出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Jpeg,
    Png,
    Webp,
}

impl OutputFormat {
    /// 返回对应的文件扩展名（不含点）
    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }
}

/// 导出控制参数（每次导出时由前端传入，不保存在预设中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportOptions {
    /// 输出长边像素上限。None = 保持原尺寸。与 target_size 同时设置时 target_size 优先。
    #[serde(default)]
    pub max_long_side: Option<u32>,
    /// 有损编码质量（1-100）。仅 JPEG 和 WebP 有损模式下使用，PNG 忽略。
    #[serde(default = "default_quality")]
    pub quality: u8,
    /// 输出文件格式
    #[serde(default)]
    pub format: OutputFormat,
    /// 社媒导出预设：精确目标像素尺寸 (宽, 高)。
    /// 缩放到刚好装入目标框（不裁切），再居中补白到精确尺寸。
    #[serde(default)]
    pub target_size: Option<(u32, u32)>,
    /// target_size 补白部分的填充色
    #[serde(default = "default_target_fill_color")]
    pub target_fill_color: [u8; 3],
}

fn default_quality() -> u8 {
    95
}

fn default_target_fill_color() -> [u8; 3] {
    [255, 255, 255]
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            max_long_side: None,
            quality: 95,
            format: OutputFormat::Jpeg,
            target_size: None,
            target_fill_color: default_target_fill_color(),
        }
    }
}
