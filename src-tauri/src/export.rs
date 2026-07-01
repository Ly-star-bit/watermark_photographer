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
    /// 输出长边像素上限。None = 保持原尺寸。
    #[serde(default)]
    pub max_long_side: Option<u32>,
    /// 有损编码质量（1-100）。仅 JPEG 和 WebP 有损模式下使用，PNG 忽略。
    #[serde(default = "default_quality")]
    pub quality: u8,
    /// 输出文件格式
    #[serde(default)]
    pub format: OutputFormat,
}

fn default_quality() -> u8 {
    95
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            max_long_side: None,
            quality: 95,
            format: OutputFormat::Jpeg,
        }
    }
}
