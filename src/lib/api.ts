/**
 * Tauri 通道封装。
 * 所有对 Rust 侧 command / plugin 的调用都从这里出，方便统一错误处理和替换。
 */

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { SUPPORTED_INPUT_EXTS, type PhotoFile, type WatermarkConfig, type ExportOptions, type FrameConfig } from "./types";

/** 判断文件扩展名是否为支持的输入图片格式（JPEG/PNG/TIFF/WebP/BMP） */
export function isSupportedImagePath(path: string): boolean {
  const lower = path.toLowerCase();
  return SUPPORTED_INPUT_EXTS.some((ext) => lower.endsWith(`.${ext}`));
}

/** 判断是否为 PNG */
export function isPngPath(path: string): boolean {
  return path.toLowerCase().endsWith(".png");
}

/** 从绝对路径推导 basename */
export function basename(path: string): string {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx >= 0 ? path.slice(idx + 1) : path;
}

/** 把磁盘路径包装为 PhotoFile（含 asset:// URL） */
export function toPhotoFile(path: string): PhotoFile {
  return {
    path,
    name: basename(path),
    assetUrl: convertFileSrc(path),
    thumbnailUrl: null,
  };
}

/** 弹出系统文件选择框，返回选中的图片绝对路径列表（JPEG/PNG/TIFF/WebP/BMP） */
export async function pickImageFiles(): Promise<string[]> {
  const selected = await open({
    multiple: true,
    filters: [
      { name: "图片", extensions: [...SUPPORTED_INPUT_EXTS] },
    ],
  });
  if (!selected) return [];
  const arr = Array.isArray(selected) ? selected : [selected];
  return arr.filter(isSupportedImagePath);
}

/** 弹出文件选择框，选一张 PNG 签名图 */
export async function pickPngFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    filters: [{ name: "PNG 签名", extensions: ["png"] }],
  });
  if (!selected || Array.isArray(selected)) return null;
  return isPngPath(selected) ? selected : null;
}

/** 注册窗口级 OS 拖入文件事件，过滤支持的图片格式后回调 */
export async function onImageDrop(cb: (paths: string[]) => void): Promise<() => void> {
  const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      const imgs = event.payload.paths.filter(isSupportedImagePath);
      if (imgs.length > 0) cb(imgs);
    }
  });
  return unlisten;
}

// —— 缩略图 —————————————————————————————————————————————
/** 请求 Rust 生成缩略图，返回 blob URL 供 <img> 使用。
 *  内部：Rust 用 Triangle 滤波下采样到 max_size 长边，编码为质量 78 的 JPEG。
 */
export async function createThumbnail(path: string, maxSize = 240): Promise<string> {
  const bytes = await invoke<number[]>("create_thumbnail", {
    path,
    maxSize,
  });
  const blob = new Blob([new Uint8Array(bytes)], { type: "image/jpeg" });
  return URL.createObjectURL(blob);
}

/** 选择输出目录 */
export async function pickOutputDir(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  if (!selected || Array.isArray(selected)) return null;
  return selected;
}

// —— 批量导出 —————————————————————————————————————————————

/** 进度事件 payload（与 Rust BatchProgress 一致） */
export interface BatchProgress {
  done: number;
  total: number;
  filename: string;
  ok: boolean;
}

/** 单项结果（与 Rust ItemResult 一致） */
export interface ItemResult {
  input: string;
  output: string | null;
  error: string | null;
}

/** 批量结果汇总（与 Rust BatchSummary 一致） */
export interface BatchSummary {
  total: number;
  success: number;
  failed: number;
  items: ItemResult[];
}

/** 监听 batch-progress 事件 */
export async function onBatchProgress(
  cb: (p: BatchProgress) => void,
): Promise<UnlistenFn> {
  return listen<BatchProgress>("batch-progress", (e) => cb(e.payload));
}

/** 触发批量导出（Rust 侧 rayon 并行 + 事件推送进度） */
export async function exportBatch(args: {
  inputPaths: string[];
  outputDir: string;
  watermarkPath: string;
  config: WatermarkConfig;
  exportOptions: ExportOptions;
  filenameTemplate: string;
}): Promise<BatchSummary> {
  return invoke<BatchSummary>("export_batch", {
    args: {
      input_paths: args.inputPaths,
      output_dir: args.outputDir,
      watermark_path: args.watermarkPath,
      config: args.config,
      export_options: args.exportOptions,
      filename_template: args.filenameTemplate,
    },
  });
}

/** 完成后打开输出目录（复用 opener 插件）
 *  Windows 上把 openPath 用于目录 = 用 Explorer 打开
 */
export async function openInFileManager(path: string): Promise<void> {
  const { openPath } = await import("@tauri-apps/plugin-opener");
  await openPath(path);
}

// —— 预设管理 —————————————————————————————————————————————

export interface Preset {
  name: string;
  config: WatermarkConfig;
  watermark_path: string | null;
}

export async function listPresets(): Promise<Preset[]> {
  return invoke<Preset[]>("list_presets");
}

export async function savePreset(preset: Preset): Promise<Preset[]> {
  return invoke<Preset[]>("save_preset", { preset });
}

export async function deletePreset(name: string): Promise<Preset[]> {
  return invoke<Preset[]>("delete_preset", { name });
}

/** 获取照片的 EXIF 文字预览（模板渲染后的文本，或自定义文字） */
export async function previewExifText(
  path: string,
  template: string,
  customText: string | null,
): Promise<string> {
  const result = await invoke<{ text: string }>("preview_exif_text", {
    path,
    template,
    customText,
  });
  return result.text;
}

/** 获取相框参数条的预览文本（左/右两行 + 品牌名，均已套用模板+EXIF解析） */
export async function previewFrame(
  path: string,
  config: FrameConfig,
): Promise<{ left: string[]; right: string[]; brand: string }> {
  return invoke<{ left: string[]; right: string[]; brand: string }>("preview_frame", {
    path,
    config,
  });
}
