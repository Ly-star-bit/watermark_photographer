/**
 * 前后端共享类型。
 * GridPosition / WatermarkConfig 与 Rust 端 (src-tauri/src/position.rs) 结构一一对应，
 * 字段命名保持 snake_case，让 serde 反序列化不需自定义映射。
 */

export type GridPosition =
  | "top_left"
  | "top_center"
  | "top_right"
  | "middle_left"
  | "center"
  | "middle_right"
  | "bottom_left"
  | "bottom_center"
  | "bottom_right";

export const GRID_POSITIONS: GridPosition[] = [
  "top_left", "top_center", "top_right",
  "middle_left", "center", "middle_right",
  "bottom_left", "bottom_center", "bottom_right",
];

export interface WatermarkConfig {
  position: GridPosition;
  size_ratio: number;         // 0.01 - 1.0
  opacity: number;            // 0.0 - 1.0
  margin_x: number;           // px
  margin_y: number;           // px
  landscape_override: GridPosition | null;
  /** 可选着色：[r,g,b]（0-255）。null=用签名原色。 */
  tint: [number, number, number] | null;
}

/** 支持的图片输入格式扩展名（小写） */
export const SUPPORTED_INPUT_EXTS = ["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp"] as const;

export interface PhotoFile {
  /** 磁盘绝对路径 */
  path: string;
  /** 文件名（含扩展名） */
  name: string;
  /** Tauri asset:// 协议 URL，可直接给 <img> 使用（原图，用于预览） */
  assetUrl: string;
  /** 异步生成的缩略图 blob URL（用于列表；null=还没生成好） */
  thumbnailUrl: string | null;
}

export const DEFAULT_CONFIG: WatermarkConfig = {
  position: "bottom_right",
  size_ratio: 0.15,
  opacity: 0.8,
  margin_x: 30,
  margin_y: 30,
  landscape_override: null,
  tint: null,
};

/** RGB 数组 → CSS hex 字符串 */
export function rgbToHex(rgb: [number, number, number]): string {
  const hx = (n: number) => n.toString(16).padStart(2, "0");
  return `#${hx(rgb[0])}${hx(rgb[1])}${hx(rgb[2])}`;
}

/** CSS hex → RGB 数组，非法输入返回 null */
export function hexToRgb(hex: string): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}
