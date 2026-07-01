// 统一错误类型
// 使用 thiserror 派生 Error trait，方便向 Tauri command 返回

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WatermarkError {
    #[error("图像读写错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("图像解码/编码错误：{0}")]
    Image(#[from] image::ImageError),

    #[error("JPEG 段解析错误：{0}")]
    JpegParts(#[from] img_parts::Error),

    #[error("JSON 序列化错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("参数非法：{0}")]
    InvalidParam(String),

    #[error("水印尺寸大于底图：底图 {img_w}x{img_h}，水印 {wm_w}x{wm_h}")]
    WatermarkTooLarge {
        img_w: u32,
        img_h: u32,
        wm_w: u32,
        wm_h: u32,
    },
}

pub type Result<T> = std::result::Result<T, WatermarkError>;

// Tauri command 需要错误实现 Serialize
impl serde::Serialize for WatermarkError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
