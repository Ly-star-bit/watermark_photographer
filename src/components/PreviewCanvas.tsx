import { useEffect, useRef, useState } from "react";
import { Aperture, Loader2 } from "lucide-react";
import { computePosition, targetWatermarkWidth } from "@/lib/preview";
import { previewExifText, previewFrame } from "@/lib/api";
import type { PhotoFile, WatermarkConfig } from "@/lib/types";

/** 相框参数条预览文本（左/右两行 + 品牌名） */
interface FrameTexts {
  left: string[];
  right: string[];
  brand: string;
}

const EMPTY_FRAME_TEXTS: FrameTexts = { left: [], right: [], brand: "" };

/** 把颜色按因子（0..1）压暗，与 Rust frame::darken 语义一致 */
function darkenColor(c: [number, number, number], factor: number): string {
  return `rgb(${Math.round(c[0] * factor)},${Math.round(c[1] * factor)},${Math.round(c[2] * factor)})`;
}

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
  const [frameTexts, setFrameTexts] = useState<FrameTexts>(EMPTY_FRAME_TEXTS);
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

  // 获取相框参数条预览文本（异步从 Rust 获取，套用模板 + 品牌归一化）
  useEffect(() => {
    if (!photo || !config.frame?.enabled) {
      setFrameTexts(EMPTY_FRAME_TEXTS);
      return;
    }
    const fc = config.frame;
    let cancelled = false;
    previewFrame(photo.path, fc)
      .then((r) => {
        if (!cancelled) setFrameTexts(r);
      })
      .catch(() => {
        if (!cancelled) setFrameTexts(EMPTY_FRAME_TEXTS);
      });
    return () => {
      cancelled = true;
    };
  }, [
    photo,
    config.frame?.enabled,
    config.frame?.left_lines?.join("|") ?? "",
    config.frame?.right_lines?.join("|") ?? "",
    config.frame?.brand_template,
    config.frame?.show_brand,
  ]);

  // 绘制
  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container || !baseImg) return;

    const cw = container.clientWidth;
    const ch = container.clientHeight;
    if (cw === 0 || ch === 0) return;

    const pw = baseImg.width;
    const ph = baseImg.height;
    const fc = config.frame?.enabled ? config.frame : null;

    // 相框几何（与 Rust frame::apply 同公式）：短边 × ratio 得到边框/参数条宽度。
    // border/bottomBar 与 photo 共享同一原图像素坐标系，photo 偏移 (border, border)。
    const short = Math.min(pw, ph);
    const border = fc ? Math.round(short * fc.border_ratio) : 0;
    const bottomBar = fc ? Math.round(short * fc.bottom_bar_ratio) : 0;
    const newW = pw + border * 2;
    const newH = ph + border + bottomBar;

    const ratio = newW / newH;
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

    // 统一缩放系数：原图像素坐标系（含边框/参数条）→ 显示像素。
    // 未启用相框时 border=0、newW=pw，退化为原有的 s = dw / baseImg.width。
    const s = dw / newW;

    if (fc) {
      ctx.fillStyle = `rgb(${fc.border_color[0]},${fc.border_color[1]},${fc.border_color[2]})`;
      ctx.fillRect(0, 0, dw, dh);
    }

    ctx.drawImage(baseImg.el, border * s, border * s, pw * s, ph * s);

    // PNG 签名水印（叠加在照片区内，坐标需加上 border 偏移）
    if (wmImg) {
      const targetWmW = targetWatermarkWidth(pw, ph, config.size_ratio);
      const wmScale = targetWmW / wmImg.width;
      const targetWmH = Math.max(1, Math.round(wmImg.height * wmScale));

      const { x, y } = computePosition(pw, ph, targetWmW, targetWmH, config);

      const src = tintWatermark(wmImg.el, wmImg.width, wmImg.height, config.tint);
      ctx.globalAlpha = Math.max(0, Math.min(1, config.opacity));
      ctx.drawImage(src, (border + x) * s, (border + y) * s, targetWmW * s, targetWmH * s);
      ctx.globalAlpha = 1;
    }

    // EXIF 文字水印（Canvas fillText 渲染，与 Rust ab_glyph 渲染对应）
    if (exifText && config.exif_text) {
      const etc = config.exif_text;
      // 与 Rust 端保持一致：字号 = 长边 × ratio，下限 8px（原图坐标系）
      const longSide = Math.max(pw, ph);
      const fontPxImg = Math.max(8, longSide * etc.font_size_ratio);
      const fontSize = fontPxImg * s;
      if (fontSize >= 2) {
        const fontFamily = "'Source Code Pro', 'Courier New', monospace";
        ctx.font = `${fontSize}px ${fontFamily}`;
        ctx.textBaseline = "top";
        ctx.textAlign = "left";

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

        // 用 Rust 位置算法计算坐标（照片区内，仍需加 border 偏移）
        const { x, y } = computePosition(
          pw,
          ph,
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
            frame: null,
          },
        );

        const tx = (border + x) * s;
        const ty = (border + y) * s;

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
    }

    // 相框：顶部分割线 + 底部参数条文本（与 Rust frame::apply 排版公式一致）
    if (fc) {
      const barTop = border + ph;
      const barH = bottomBar;
      const innerPad = Math.round(barH * 0.15);
      const mainFontPx = Math.max(barH * fc.font_size_ratio, 10);
      const brandFontPx = Math.max(barH * fc.brand_size_ratio, 12);
      const subFontPx = mainFontPx * 0.85;
      const fontFamily = "'Source Code Pro', 'Courier New', monospace";

      // 顶部分割线
      const sepThickness = Math.max(barH * 0.015, 1);
      ctx.fillStyle = darkenColor(fc.border_color, 0.85);
      ctx.fillRect(border * s, barTop * s, pw * s, sepThickness * s);

      const textBlockH = mainFontPx + subFontPx * 0.2 + subFontPx;
      const textY0 = barTop + (barH - textBlockH) / 2;
      const lineYs = [textY0, textY0 + mainFontPx * 1.15];

      ctx.textBaseline = "top";
      const drawBlock = (lines: string[], anchorX: number, align: CanvasTextAlign) => {
        ctx.textAlign = align;
        lines.forEach((line, i) => {
          const fontPx = i === 0 ? mainFontPx : subFontPx;
          const color = i === 0 ? fc.text_color : fc.subtext_color;
          ctx.font = `${fontPx * s}px ${fontFamily}`;
          ctx.fillStyle = `rgb(${color[0]},${color[1]},${color[2]})`;
          const y = lineYs[i] ?? lineYs[lineYs.length - 1];
          ctx.fillText(line, anchorX * s, y * s);
        });
      };

      drawBlock(frameTexts.left, border + innerPad, "left");
      drawBlock(frameTexts.right, newW - border - innerPad, "right");

      if (fc.show_brand && frameTexts.brand) {
        ctx.textAlign = "center";
        ctx.font = `${brandFontPx * s}px ${fontFamily}`;
        ctx.fillStyle = `rgb(${fc.text_color[0]},${fc.text_color[1]},${fc.text_color[2]})`;
        const cy = barTop + (barH - brandFontPx) / 2;
        ctx.fillText(frameTexts.brand, (newW / 2) * s, cy * s);
      }
    }
  }, [baseImg, wmImg, config, exifText, frameTexts]);

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
