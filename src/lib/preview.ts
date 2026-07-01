/**
 * 预览端位置算法。
 *
 * ⚠️ 关键约束：本文件的四个函数必须与 Rust 端 (src-tauri/src/position.rs)
 * 数学完全一致。用户在 UI 中看到的预览位置 = Rust 导出的实际位置。
 * 任一端修改，另一端必须同步。
 */

import type { GridPosition, WatermarkConfig } from "./types";

export function isLandscape(w: number, h: number): boolean {
  return w >= h;
}

export function scaleBase(w: number, h: number): number {
  return Math.min(w, h);
}

/** 目标水印宽度 = 短边 × size_ratio，至少 1 像素 */
export function targetWatermarkWidth(
  imgW: number,
  imgH: number,
  sizeRatio: number,
): number {
  return Math.max(1, Math.round(scaleBase(imgW, imgH) * sizeRatio));
}

/** 计算水印左上角坐标（相对底图像素） */
export function computePosition(
  imgW: number,
  imgH: number,
  wmW: number,
  wmH: number,
  config: WatermarkConfig,
): { x: number; y: number } {
  const anchor: GridPosition = isLandscape(imgW, imgH)
    ? (config.landscape_override ?? config.position)
    : config.position;

  const mx = config.margin_x;
  const my = config.margin_y;

  let x: number;
  let y: number;

  switch (anchor) {
    case "top_left":       x = mx;                    y = my;                    break;
    case "top_center":     x = (imgW - wmW) / 2;      y = my;                    break;
    case "top_right":      x = imgW - wmW - mx;       y = my;                    break;
    case "middle_left":    x = mx;                    y = (imgH - wmH) / 2;      break;
    case "center":         x = (imgW - wmW) / 2;      y = (imgH - wmH) / 2;      break;
    case "middle_right":   x = imgW - wmW - mx;       y = (imgH - wmH) / 2;      break;
    case "bottom_left":    x = mx;                    y = imgH - wmH - my;       break;
    case "bottom_center":  x = (imgW - wmW) / 2;      y = imgH - wmH - my;       break;
    case "bottom_right":   x = imgW - wmW - mx;       y = imgH - wmH - my;       break;
  }

  // Clamp 防越界（与 Rust 端一致）
  x = Math.min(Math.max(0, x), imgW - wmW);
  y = Math.min(Math.max(0, y), imgH - wmH);

  return { x, y };
}
