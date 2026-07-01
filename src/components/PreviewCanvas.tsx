import { useEffect, useRef, useState } from "react";
import { Aperture, Loader2 } from "lucide-react";
import { computePosition, targetWatermarkWidth } from "@/lib/preview";
import { previewExifText } from "@/lib/api";
import type { PhotoFile, WatermarkConfig } from "@/lib/types";

/**
 * 生成着色后的水印离屏 canvas。
 * 原理：先原样绘制 PNG，然后 source-in 复合模式用目标色填充整个 canvas，
 * 结果是所有不透明像素的 RGB 被替换为目标色、alpha 通道保持原样（边缘不糊）。
 */
function tintWatermark(
  img: HTMLImageElement,
  w: number,
  h: number,
  tint: [number, number, number] | null,
): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const cx = c.getContext("2d")!;
  cx.drawImage(img, 0, 0, w, h);
  if (tint) {
    cx.globalCompositeOperation = "source-in";
    cx.fillStyle = `rgb(${tint[0]},${tint[1]},${tint[2]})`;
    cx.fillRect(0, 0, w, h);
    cx.globalCompositeOperation = "source-over";
  }
  return c;
}

interface Props {
  photo: PhotoFile | null;
  watermarkUrl: string | null;
  config: WatermarkConfig;
}

interface LoadedImage {
  el: HTMLImageElement;
  width: number;
  height: number;
}

/**
 * 使用 img.decode() 触发浏览器异步解码（可能走 off-main-thread），
 * 比等 onload 更可靠，且能在 await 中捕获解码错误。
 */
async function loadImg(url: string): Promise<LoadedImage> {
  const img = new Image();
  img.decoding = "async";
  img.src = url;
  await img.decode();
  return { el: img, width: img.naturalWidth, height: img.naturalHeight };
}

/** LRU 缓存上限：缓存 5 张最近解码的原图，切换回已访问过的照片瞬时 */
const IMAGE_CACHE_LIMIT = 5;

/**
 * Canvas 实时预览。
 * 与 Rust 端 watermark::compose() 输出等效，位置/缩放使用 preview.ts 中与 Rust 一致的算法。
 * 支持：PNG 签名水印 + EXIF 文字水印（异步从 Rust 获取渲染文本）。
 */
export function PreviewCanvas({ photo, watermarkUrl, config }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const cacheRef = useRef<Map<string, LoadedImage>>(new Map());
  const [baseImg, setBaseImg] = useState<LoadedImage | null>(null);
  const [wmImg, setWmImg] = useState<LoadedImage | null>(null);
  const [exifText, setExifText] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 载入底图（含 LRU 缓存）
  useEffect(() => {
    if (!photo) {
      setBaseImg(null);
      setLoading(false);
      return;
    }
    setError(null);

    const cached = cacheRef.current.get(photo.assetUrl);
    if (cached) {
      cacheRef.current.delete(photo.assetUrl);
      cacheRef.current.set(photo.assetUrl, cached);
      setBaseImg(cached);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    loadImg(photo.assetUrl)
      .then((r) => {
        if (cancelled) return;
        cacheRef.current.set(photo.assetUrl, r);
        while (cacheRef.current.size > IMAGE_CACHE_LIMIT) {
          const first = cacheRef.current.keys().next().value;
          if (first) cacheRef.current.delete(first);
        }
        setBaseImg(r);
        setLoading(false);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [photo]);

  // 载入水印
  useEffect(() => {
    if (!watermarkUrl) {
      setWmImg(null);
      return;
    }
    let cancelled = false;
    loadImg(watermarkUrl)
      .then((r) => !cancelled && setWmImg(r))
      .catch(() => !cancelled && setWmImg(null));
    return () => {
      cancelled = true;
    };
  }, [watermarkUrl]);

  // 获取文字水印文本（自定义文字优先，否则异步获取 EXIF）
  useEffect(() => {
    if (!photo || !config.exif_text?.enabled) {
      setExifText("");
      return;
    }
    const etc = config.exif_text;
    // 自定义文字模式：直接使用
    if (etc.custom_text !== null) {
      setExifText(etc.custom_text);
      return;
    }
    // EXIF 模式：异步从 Rust 获取
    let cancelled = false;
    previewExifText(photo.path, etc.template, etc.custom_text)
      .then((text) => {
        if (!cancelled) setExifText(text);
      })
      .catch(() => {
        if (!cancelled) setExifText("");
      });
    return () => {
      cancelled = true;
    };
  }, [photo, config.exif_text?.enabled, config.exif_text?.template, config.exif_text?.custom_text]);

  // 绘制
  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container || !baseImg) return;

    const cw = container.clientWidth;
    const ch = container.clientHeight;
    if (cw === 0 || ch === 0) return;

    const ratio = baseImg.width / baseImg.height;
    let dw = cw;
    let dh = cw / ratio;
    if (dh > ch) {
      dh = ch;
      dw = ch * ratio;
    }
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(dw * dpr);
    canvas.height = Math.round(dh * dpr);
    canvas.style.width = `${dw}px`;
    canvas.style.height = `${dh}px`;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, dw, dh);

    const s = dw / baseImg.width;

    ctx.drawImage(baseImg.el, 0, 0, dw, dh);

    // PNG 签名水印
    if (wmImg) {
      const targetWmW = targetWatermarkWidth(
        baseImg.width,
        baseImg.height,
        config.size_ratio,
      );
      const wmScale = targetWmW / wmImg.width;
      const targetWmH = Math.max(1, Math.round(wmImg.height * wmScale));

      const { x, y } = computePosition(
        baseImg.width,
        baseImg.height,
        targetWmW,
        targetWmH,
        config,
      );

      const src = tintWatermark(wmImg.el, wmImg.width, wmImg.height, config.tint);
      ctx.globalAlpha = Math.max(0, Math.min(1, config.opacity));
      ctx.drawImage(src, x * s, y * s, targetWmW * s, targetWmH * s);
      ctx.globalAlpha = 1;
    }

    // EXIF 文字水印（Canvas fillText 渲染，与 Rust ab_glyph 渲染对应）
    if (exifText && config.exif_text) {
      const etc = config.exif_text;
      // 与 Rust 端保持一致：字号 = 长边 × ratio，下限 8px（原图坐标系）
      const longSide = Math.max(baseImg.width, baseImg.height);
      const fontPxImg = Math.max(8, longSide * etc.font_size_ratio);
      const fontSize = fontPxImg * s;
      if (fontSize < 2) return; // 太小不可见，跳过

      const fontFamily = "'Source Code Pro', 'Courier New', monospace";
      ctx.font = `${fontSize}px ${fontFamily}`;
      ctx.textBaseline = "top";

      // 计算文字尺寸
      const lines = exifText.split("\n");
      const lineHeight = fontSize * 1.3;
      let maxW = 0;
      for (const line of lines) {
        const m = ctx.measureText(line);
        if (m.width > maxW) maxW = m.width;
      }
      const textW = Math.ceil(maxW);
      const textH = Math.ceil(lineHeight * lines.length);

      // 内边距（与 Rust padding 对应）
      const pad = etc.background ? fontSize * 0.3 : 0;
      const totalW = textW + pad * 2;
      const totalH = textH + pad * 2;

      // 用 Rust 位置算法计算坐标
      const { x, y } = computePosition(
        baseImg.width,
        baseImg.height,
        Math.ceil(totalW / s),
        Math.ceil(totalH / s),
        {
          position: etc.position,
          size_ratio: 0,
          opacity: etc.opacity,
          margin_x: etc.margin_x,
          margin_y: etc.margin_y,
          landscape_override: null,
          tint: null,
          exif_text: null,
        },
      );

      const tx = x * s;
      const ty = y * s;

      // 背景条
      if (etc.background) {
        const [br, bg, bb, ba] = etc.background;
        ctx.fillStyle = `rgba(${br},${bg},${bb},${ba / 255})`;
        ctx.fillRect(tx, ty, totalW, totalH);
      }

      // 文字
      const [cr, cg, cb] = etc.color;
      ctx.fillStyle = `rgba(${cr},${cg},${cb},${etc.opacity})`;
      for (let li = 0; li < lines.length; li++) {
        ctx.fillText(lines[li], tx + pad, ty + pad + li * lineHeight);
      }
    }
  }, [baseImg, wmImg, config, exifText]);

  // 容器 resize 时重绘
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setBaseImg((prev) => (prev ? { ...prev } : prev));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return (
    <div
      ref={containerRef}
      className="relative flex h-full w-full items-center justify-center overflow-hidden"
    >
      {baseImg ? (
        <>
          <canvas ref={canvasRef} className="rounded-md shadow-2xl shadow-black/40" />
          {loading && (
            <div className="absolute right-3 top-3 flex items-center gap-1.5 rounded-md bg-black/60 px-2 py-1 text-[11px] text-white backdrop-blur">
              <Loader2 className="h-3 w-3 animate-spin" />
              解码中
            </div>
          )}
        </>
      ) : error ? (
        <div className="text-center text-destructive text-sm">{error}</div>
      ) : loading ? (
        <div className="flex flex-col items-center gap-3 text-muted-foreground">
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
          <p className="text-sm">解码中...</p>
        </div>
      ) : (
        <div className="text-center">
          <Aperture className="mx-auto h-10 w-10 text-muted-foreground/40 mb-3" />
          <p className="text-sm text-muted-foreground">
            {photo ? "载入中..." : "选择一张照片预览水印效果"}
          </p>
        </div>
      )}
    </div>
  );
}
