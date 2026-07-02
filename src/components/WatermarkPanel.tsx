import { useRef, useState } from "react";
import { FileImage, Frame, Image, Ratio, Type, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { GridPosition, WatermarkConfig } from "@/lib/types";
import {
  CANVAS_RATIO_PRESETS,
  DEFAULT_CANVAS_RATIO,
  DEFAULT_EXIF_TEXT,
  DEFAULT_FRAME,
  DEFAULT_TILE,
  GRID_POSITIONS,
  hexToRgb,
  rgbToHex,
} from "@/lib/types";
import { pickPngFile, basename } from "@/lib/api";
import { convertFileSrc } from "@tauri-apps/api/core";

interface Props {
  watermarkPath: string | null;
  onWatermarkChange: (path: string | null) => void;
  config: WatermarkConfig;
  onConfigChange: (patch: Partial<WatermarkConfig>) => void;
}

type TabId = "image" | "text" | "frame" | "canvas";

const TABS: { id: TabId; label: string; icon: typeof Image }[] = [
  { id: "image", label: "图片水印", icon: Image },
  { id: "text", label: "文字水印", icon: Type },
  { id: "frame", label: "相框", icon: Frame },
  { id: "canvas", label: "画布比例", icon: Ratio },
];

export function WatermarkPanel({
  watermarkPath,
  onWatermarkChange,
  config,
  onConfigChange,
}: Props) {
  const [tab, setTab] = useState<TabId>("image");

  return (
    <div className="space-y-6">
      {/* Tab 切换 */}
      <div className="flex rounded-md bg-muted/40 p-0.5">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-sm px-3 py-1.5 text-xs font-medium transition",
              tab === t.id
                ? "bg-card text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            <t.icon className="h-3.5 w-3.5" />
            {t.label}
          </button>
        ))}
      </div>

      {tab === "image" ? (
        <ImageTab
          watermarkPath={watermarkPath}
          onWatermarkChange={onWatermarkChange}
          config={config}
          onConfigChange={onConfigChange}
        />
      ) : tab === "text" ? (
        <TextTab config={config} onConfigChange={onConfigChange} />
      ) : tab === "frame" ? (
        <FrameTab config={config} onConfigChange={onConfigChange} />
      ) : (
        <CanvasRatioTab config={config} onConfigChange={onConfigChange} />
      )}
    </div>
  );
}

// —— 图片水印 Tab ——————————————————————————————————————

function ImageTab({
  watermarkPath,
  onWatermarkChange,
  config,
  onConfigChange,
}: Omit<Props, "">) {
  const tile = config.tile ?? DEFAULT_TILE;

  return (
    <div className="space-y-6">
      <Section title="签名图">
        <WatermarkPicker
          path={watermarkPath}
          onPick={onWatermarkChange}
        />
      </Section>

      {/* 全图平铺（防盗样片模式） */}
      <div className="flex items-center justify-between rounded-md border border-border/60 bg-card/40 p-3">
        <div>
          <div className="text-xs font-medium">全图平铺</div>
          <div className="text-[10px] text-muted-foreground mt-0.5">
            水印旋转后铺满整张照片，用于样片防盗
          </div>
        </div>
        <Toggle
          checked={tile.enabled}
          onChange={(v) => onConfigChange({ tile: { ...tile, enabled: v } })}
        />
      </div>

      {tile.enabled ? (
        <>
          <Section title="旋转角度" hint={`${tile.angle_deg.toFixed(0)}°`}>
            <Slider
              min={0}
              max={90}
              step={1}
              value={tile.angle_deg}
              onChange={(v) => onConfigChange({ tile: { ...tile, angle_deg: v } })}
            />
          </Section>

          <Section title="平铺间距" hint={`${(tile.gap_ratio * 100).toFixed(0)}%`}>
            <Slider
              min={0}
              max={2}
              step={0.05}
              value={tile.gap_ratio}
              onChange={(v) => onConfigChange({ tile: { ...tile, gap_ratio: v } })}
            />
          </Section>
        </>
      ) : (
        <Section title="位置">
          <NineGrid
            selected={config.position}
            onSelect={(pos) => onConfigChange({ position: pos })}
          />
        </Section>
      )}

      <Section title="大小" hint={`${(config.size_ratio * 100).toFixed(0)}%`}>
        <Slider
          min={0.03}
          max={0.5}
          step={0.005}
          value={config.size_ratio}
          onChange={(v) => onConfigChange({ size_ratio: v })}
        />
      </Section>

      <Section title="不透明度" hint={`${Math.round(config.opacity * 100)}%`}>
        <Slider
          min={0}
          max={1}
          step={0.01}
          value={config.opacity}
          onChange={(v) => onConfigChange({ opacity: v })}
        />
      </Section>

      <Section title="边距" hint={`${config.margin_x} px`}>
        <Slider
          min={0}
          max={200}
          step={1}
          value={config.margin_x}
          onChange={(v) => onConfigChange({ margin_x: v, margin_y: v })}
        />
      </Section>

      <Section
        title="颜色"
        hint={config.tint ? rgbToHex(config.tint).toUpperCase() : "原色"}
      >
        <ColorPicker
          tint={config.tint}
          onChange={(t) => onConfigChange({ tint: t })}
        />
      </Section>
    </div>
  );
}

// —— 文字水印 Tab ——————————————————————————————————————

function TextTab({
  config,
  onConfigChange,
}: {
  config: WatermarkConfig;
  onConfigChange: (patch: Partial<WatermarkConfig>) => void;
}) {
  const exifEnabled = config.exif_text?.enabled ?? false;
  const cfg = config.exif_text ?? DEFAULT_EXIF_TEXT;
  const isCustom = cfg.custom_text !== null;

  return (
    <div className="space-y-6">
      {/* 启用开关 */}
      <div className="flex items-center justify-between rounded-md border border-border/60 bg-card/40 p-3">
        <div>
          <div className="text-xs font-medium">文字水印</div>
          <div className="text-[10px] text-muted-foreground mt-0.5">
            {isCustom ? "自定义文字" : "将相机型号、光圈、快门等参数以文字叠加到照片"}
          </div>
        </div>
        <button
          type="button"
          onClick={() => {
            const next = !exifEnabled;
            onConfigChange({
              exif_text: next
                ? { ...cfg, enabled: true }
                : { ...cfg, enabled: false },
            });
          }}
          className={cn(
            "inline-flex h-6 w-10 shrink-0 items-center rounded-full transition",
            exifEnabled ? "bg-primary" : "bg-muted-foreground/30",
          )}
        >
          <span
            className={cn(
              "inline-block h-4 w-4 rounded-full bg-white shadow transition",
              exifEnabled ? "translate-x-5" : "translate-x-1",
            )}
          />
        </button>
      </div>

      {exifEnabled && (
        <>
          {/* 文字来源切换 */}
          <div className="flex rounded-md bg-muted/40 p-0.5">
            <button
              type="button"
              onClick={() =>
                onConfigChange({
                  exif_text: { ...cfg, custom_text: null },
                })
              }
              className={cn(
                "flex-1 rounded-sm px-3 py-1.5 text-xs transition",
                !isCustom
                  ? "bg-card text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              EXIF 参数
            </button>
            <button
              type="button"
              onClick={() =>
                onConfigChange({
                  exif_text: { ...cfg, custom_text: "" },
                })
              }
              className={cn(
                "flex-1 rounded-sm px-3 py-1.5 text-xs transition",
                isCustom
                  ? "bg-card text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              自定义
            </button>
          </div>

          {isCustom ? (
            /* 自定义文字输入 */
            <div className="space-y-1.5">
              <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                文字内容
              </label>
              <textarea
                value={cfg.custom_text ?? ""}
                onChange={(e) =>
                  onConfigChange({
                    exif_text: { ...cfg, custom_text: e.target.value },
                  })
                }
                rows={3}
                placeholder="例如：© Photographer Name"
                className="w-full resize-none rounded-md border border-border/60 bg-card/40 px-2.5 py-1.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:border-primary/60 focus:outline-none"
              />
              <p className="text-[10px] text-muted-foreground/60">
                支持多行，使用 Enter 换行
              </p>
            </div>
          ) : (
            /* EXIF 模板输入 */
            <div className="space-y-1.5">
              <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                模板
              </label>
              <input
                type="text"
                value={cfg.template}
                onChange={(e) =>
                  onConfigChange({
                    exif_text: { ...cfg, template: e.target.value },
                  })
                }
                className="h-8 w-full rounded-md border border-border/60 bg-card/40 px-2.5 text-[11px] text-foreground focus:border-primary/60 focus:outline-none font-mono"
              />
              <p className="text-[10px] text-muted-foreground/60">
                {"{make} {model} · {lens} · f/{fnumber} · {shutter}s · ISO {iso} · {date}"}
              </p>
            </div>
          )}

          {/* 字号（相对图片长边的比例，例：6000px 长边 × 3% = 180px 字号） */}
          <Section
            title="字号"
            hint={`${(cfg.font_size_ratio * 100).toFixed(1)}%`}
          >
            <Slider
              min={0.01}
              max={0.1}
              step={0.001}
              value={cfg.font_size_ratio}
              onChange={(v) =>
                onConfigChange({ exif_text: { ...cfg, font_size_ratio: v } })
              }
            />
          </Section>

          {/* 位置 */}
          <div className="space-y-1.5">
            <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              位置
            </label>
            <NineGrid
              selected={cfg.position}
              onSelect={(pos) =>
                onConfigChange({ exif_text: { ...cfg, position: pos } })
              }
            />
          </div>

          {/* 边距 */}
          <Section title="边距" hint={`${cfg.margin_x}px`}>
            <Slider
              min={0}
              max={200}
              step={1}
              value={cfg.margin_x}
              onChange={(v) =>
                onConfigChange({
                  exif_text: { ...cfg, margin_x: v, margin_y: v },
                })
              }
            />
          </Section>

          {/* 不透明度 */}
          <Section title="不透明度" hint={`${Math.round(cfg.opacity * 100)}%`}>
            <Slider
              min={0}
              max={1}
              step={0.01}
              value={cfg.opacity}
              onChange={(v) =>
                onConfigChange({ exif_text: { ...cfg, opacity: v } })
              }
            />
          </Section>

          {/* 文字颜色 */}
          <Section title="文字颜色" hint={rgbToHex(cfg.color).toUpperCase()}>
            <ColorPicker
              tint={cfg.color}
              onChange={(t) =>
                onConfigChange({
                  exif_text: {
                    ...cfg,
                    color: t ?? [255, 255, 255],
                  },
                })
              }
            />
          </Section>

          {/* 背景条 */}
          <div className="flex items-center justify-between rounded-md border border-border/60 bg-card/40 p-2.5">
            <span className="text-[11px] text-muted-foreground">背景条</span>
            <button
              type="button"
              onClick={() =>
                onConfigChange({
                  exif_text: {
                    ...cfg,
                    background: cfg.background ? null : [0, 0, 0, 80],
                  },
                })
              }
              className={cn(
                "inline-flex h-6 w-10 shrink-0 items-center rounded-full transition",
                cfg.background
                  ? "bg-primary"
                  : "bg-muted-foreground/30",
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white shadow transition",
                  cfg.background ? "translate-x-5" : "translate-x-1",
                )}
              />
            </button>
          </div>
        </>
      )}
    </div>
  );
}

// —— 相框 Tab ————————————————————————————————————————————

function FrameTab({
  config,
  onConfigChange,
}: {
  config: WatermarkConfig;
  onConfigChange: (patch: Partial<WatermarkConfig>) => void;
}) {
  const enabled = config.frame?.enabled ?? false;
  const cfg = config.frame ?? DEFAULT_FRAME;

  return (
    <div className="space-y-6">
      {/* 启用开关 */}
      <div className="flex items-center justify-between rounded-md border border-border/60 bg-card/40 p-3">
        <div>
          <div className="text-xs font-medium">相框模式</div>
          <div className="text-[10px] text-muted-foreground mt-0.5">
            白/黑边框 + 底部参数条（型号、镜头、光圈快门ISO、焦距）
          </div>
        </div>
        <button
          type="button"
          onClick={() => onConfigChange({ frame: { ...cfg, enabled: !enabled } })}
          className={cn(
            "inline-flex h-6 w-10 shrink-0 items-center rounded-full transition",
            enabled ? "bg-primary" : "bg-muted-foreground/30",
          )}
        >
          <span
            className={cn(
              "inline-block h-4 w-4 rounded-full bg-white shadow transition",
              enabled ? "translate-x-5" : "translate-x-1",
            )}
          />
        </button>
      </div>

      {enabled && (
        <>
          <Section title="边框颜色" hint={rgbToHex(cfg.border_color).toUpperCase()}>
            <ColorPicker
              tint={cfg.border_color}
              onChange={(t) =>
                onConfigChange({ frame: { ...cfg, border_color: t ?? cfg.border_color } })
              }
              allowOriginal={false}
            />
          </Section>

          <Section title="边框宽度" hint={`${(cfg.border_ratio * 100).toFixed(1)}%`}>
            <Slider
              min={0.005}
              max={0.06}
              step={0.001}
              value={cfg.border_ratio}
              onChange={(v) => onConfigChange({ frame: { ...cfg, border_ratio: v } })}
            />
          </Section>

          <Section title="参数条高度" hint={`${(cfg.bottom_bar_ratio * 100).toFixed(0)}%`}>
            <Slider
              min={0.06}
              max={0.25}
              step={0.005}
              value={cfg.bottom_bar_ratio}
              onChange={(v) => onConfigChange({ frame: { ...cfg, bottom_bar_ratio: v } })}
            />
          </Section>

          <Section title="文字字号" hint={`${(cfg.font_size_ratio * 100).toFixed(0)}%`}>
            <Slider
              min={0.1}
              max={0.4}
              step={0.01}
              value={cfg.font_size_ratio}
              onChange={(v) => onConfigChange({ frame: { ...cfg, font_size_ratio: v } })}
            />
          </Section>

          <div className="space-y-1.5">
            <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              左侧两行模板
            </label>
            {[0, 1].map((i) => (
              <input
                key={i}
                type="text"
                value={cfg.left_lines[i] ?? ""}
                onChange={(e) => {
                  const next = [...cfg.left_lines];
                  next[i] = e.target.value;
                  onConfigChange({ frame: { ...cfg, left_lines: next } });
                }}
                className="h-8 w-full rounded-md border border-border/60 bg-card/40 px-2.5 text-[11px] text-foreground focus:border-primary/60 focus:outline-none font-mono"
              />
            ))}
          </div>

          <div className="space-y-1.5">
            <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              右侧两行模板
            </label>
            {[0, 1].map((i) => (
              <input
                key={i}
                type="text"
                value={cfg.right_lines[i] ?? ""}
                onChange={(e) => {
                  const next = [...cfg.right_lines];
                  next[i] = e.target.value;
                  onConfigChange({ frame: { ...cfg, right_lines: next } });
                }}
                className="h-8 w-full rounded-md border border-border/60 bg-card/40 px-2.5 text-[11px] text-foreground focus:border-primary/60 focus:outline-none font-mono"
              />
            ))}
            <p className="text-[10px] text-muted-foreground/60">
              {"{model} {lens} {focal} {fnumber} {shutter} {iso} {date}"}
            </p>
          </div>

          {/* 品牌名 */}
          <div className="flex items-center justify-between rounded-md border border-border/60 bg-card/40 p-2.5">
            <span className="text-[11px] text-muted-foreground">中央品牌名</span>
            <button
              type="button"
              onClick={() => onConfigChange({ frame: { ...cfg, show_brand: !cfg.show_brand } })}
              className={cn(
                "inline-flex h-6 w-10 shrink-0 items-center rounded-full transition",
                cfg.show_brand ? "bg-primary" : "bg-muted-foreground/30",
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white shadow transition",
                  cfg.show_brand ? "translate-x-5" : "translate-x-1",
                )}
              />
            </button>
          </div>

          {cfg.show_brand && (
            <Section title="品牌字号" hint={`${(cfg.brand_size_ratio * 100).toFixed(0)}%`}>
              <Slider
                min={0.15}
                max={0.6}
                step={0.01}
                value={cfg.brand_size_ratio}
                onChange={(v) => onConfigChange({ frame: { ...cfg, brand_size_ratio: v } })}
              />
            </Section>
          )}

          <Section title="主文字颜色" hint={rgbToHex(cfg.text_color).toUpperCase()}>
            <ColorPicker
              tint={cfg.text_color}
              onChange={(t) =>
                onConfigChange({ frame: { ...cfg, text_color: t ?? cfg.text_color } })
              }
              allowOriginal={false}
            />
          </Section>

          <Section title="副文字颜色" hint={rgbToHex(cfg.subtext_color).toUpperCase()}>
            <ColorPicker
              tint={cfg.subtext_color}
              onChange={(t) =>
                onConfigChange({ frame: { ...cfg, subtext_color: t ?? cfg.subtext_color } })
              }
              allowOriginal={false}
            />
          </Section>
        </>
      )}
    </div>
  );
}

// —— 画布比例 Tab ————————————————————————————————————————

function CanvasRatioTab({
  config,
  onConfigChange,
}: {
  config: WatermarkConfig;
  onConfigChange: (patch: Partial<WatermarkConfig>) => void;
}) {
  const enabled = config.canvas_ratio?.enabled ?? false;
  const cfg = config.canvas_ratio ?? DEFAULT_CANVAS_RATIO;

  const isPreset = (w: number, h: number) => cfg.ratio_w === w && cfg.ratio_h === h;

  return (
    <div className="space-y-6">
      {/* 启用开关 */}
      <div className="flex items-center justify-between rounded-md border border-border/60 bg-card/40 p-3">
        <div>
          <div className="text-xs font-medium">画布比例扩展</div>
          <div className="text-[10px] text-muted-foreground mt-0.5">
            照片四周补白边，扩展到目标宽高比（社媒常用竖版/方图风格）
          </div>
        </div>
        <Toggle
          checked={enabled}
          onChange={(v) => onConfigChange({ canvas_ratio: { ...cfg, enabled: v } })}
        />
      </div>

      {enabled && (
        <>
          <div className="space-y-1.5">
            <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              比例预设
            </label>
            <div className="flex flex-wrap gap-1.5">
              {CANVAS_RATIO_PRESETS.map((p) => (
                <button
                  key={p.label}
                  type="button"
                  onClick={() =>
                    onConfigChange({ canvas_ratio: { ...cfg, ratio_w: p.w, ratio_h: p.h } })
                  }
                  className={cn(
                    "rounded-md border px-3 py-1.5 text-xs transition",
                    isPreset(p.w, p.h)
                      ? "border-primary/70 bg-primary/15 text-foreground"
                      : "border-border/50 bg-card/40 text-muted-foreground hover:border-primary/40 hover:text-foreground",
                  )}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>

          <Section title="自定义比例">
            <div className="flex items-center gap-2">
              <input
                type="number"
                min={1}
                value={cfg.ratio_w}
                onChange={(e) =>
                  onConfigChange({
                    canvas_ratio: { ...cfg, ratio_w: Math.max(1, parseInt(e.target.value, 10) || 1) },
                  })
                }
                className="h-8 w-16 rounded-md border border-border/60 bg-card/40 px-2 text-xs text-foreground focus:border-primary/60 focus:outline-none"
              />
              <span className="text-xs text-muted-foreground">:</span>
              <input
                type="number"
                min={1}
                value={cfg.ratio_h}
                onChange={(e) =>
                  onConfigChange({
                    canvas_ratio: { ...cfg, ratio_h: Math.max(1, parseInt(e.target.value, 10) || 1) },
                  })
                }
                className="h-8 w-16 rounded-md border border-border/60 bg-card/40 px-2 text-xs text-foreground focus:border-primary/60 focus:outline-none"
              />
            </div>
          </Section>

          <Section title="填充色" hint={rgbToHex(cfg.fill_color).toUpperCase()}>
            <ColorPicker
              tint={cfg.fill_color}
              onChange={(t) =>
                onConfigChange({ canvas_ratio: { ...cfg, fill_color: t ?? cfg.fill_color } })
              }
              allowOriginal={false}
            />
          </Section>
        </>
      )}
    </div>
  );
}

// —— 公共组件 ——————————————————————————————————————————

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      className={cn(
        "inline-flex h-6 w-10 shrink-0 items-center rounded-full transition",
        checked ? "bg-primary" : "bg-muted-foreground/30",
      )}
    >
      <span
        className={cn(
          "inline-block h-4 w-4 rounded-full bg-white shadow transition",
          checked ? "translate-x-5" : "translate-x-1",
        )}
      />
    </button>
  );
}

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between">
        <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          {title}
        </label>
        {hint && (
          <span className="text-[11px] tabular-nums text-muted-foreground/80">
            {hint}
          </span>
        )}
      </div>
      {children}
    </div>
  );
}

function WatermarkPicker({
  path,
  onPick,
}: {
  path: string | null;
  onPick: (path: string | null) => void;
}) {
  const handleClick = async () => {
    const p = await pickPngFile();
    if (p) onPick(p);
  };

  if (path) {
    return (
      <div className="group flex items-center gap-3 rounded-md border border-border/60 bg-card/40 p-2 pr-3">
        <div className="h-14 w-14 shrink-0 overflow-hidden rounded bg-[repeating-conic-gradient(#333_0%_25%,#222_0%_50%)] bg-[length:8px_8px]">
          <img
            src={convertFileSrc(path)}
            alt="watermark"
            className="h-full w-full object-contain"
          />
        </div>
        <span className="flex-1 truncate text-xs text-muted-foreground" title={path}>
          {basename(path)}
        </span>
        <button
          type="button"
          onClick={() => onPick(null)}
          className="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive transition"
          aria-label="移除水印"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
    );
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      className="flex h-24 w-full flex-col items-center justify-center rounded-md border border-dashed border-border/60 bg-card/30 text-xs text-muted-foreground transition hover:border-primary/60 hover:bg-card/50"
    >
      <FileImage className="h-5 w-5 mb-1 opacity-70" />
      选择 PNG 签名图
    </button>
  );
}

function NineGrid({
  selected,
  onSelect,
}: {
  selected: GridPosition;
  onSelect: (pos: GridPosition) => void;
}) {
  return (
    <div className="grid grid-cols-3 gap-1.5">
      {GRID_POSITIONS.map((pos) => (
        <button
          key={pos}
          type="button"
          onClick={() => onSelect(pos)}
          aria-label={pos}
          className={cn(
            "relative aspect-square rounded border transition",
            selected === pos
              ? "border-primary/70 bg-primary/15 ring-1 ring-primary/50"
              : "border-border/40 bg-card/40 hover:border-primary/40 hover:bg-card/60",
          )}
        >
          <span
            className={cn(
              "absolute h-1.5 w-1.5 rounded-full transition",
              selected === pos ? "bg-primary" : "bg-muted-foreground/40",
              positionDotClass(pos),
            )}
          />
        </button>
      ))}
    </div>
  );
}

function positionDotClass(pos: GridPosition): string {
  const [v, h] = pos.split("_");
  const vClass =
    v === "top" ? "top-1.5" : v === "bottom" ? "bottom-1.5" : "top-1/2 -translate-y-1/2";
  const hClass =
    h === "left"
      ? "left-1.5"
      : h === "right"
        ? "right-1.5"
        : "left-1/2 -translate-x-1/2";
  return `${vClass} ${hClass}`;
}

const PRESET_COLORS: Array<{ label: string; rgb: [number, number, number] }> = [
  { label: "白", rgb: [255, 255, 255] },
  { label: "米白", rgb: [245, 240, 232] },
  { label: "浅灰", rgb: [200, 200, 200] },
  { label: "深灰", rgb: [48, 48, 48] },
  { label: "黑", rgb: [0, 0, 0] },
];

function ColorPicker({
  tint,
  onChange,
  allowOriginal = true,
}: {
  tint: [number, number, number] | null;
  onChange: (t: [number, number, number] | null) => void;
  /** 是否显示「原色」选项（对水印着色适用；边框色/文字色等纯色场景应设为 false） */
  allowOriginal?: boolean;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const currentHex = tint ? rgbToHex(tint) : "#ffffff";

  const isPreset = (rgb: [number, number, number]) =>
    tint !== null &&
    tint[0] === rgb[0] &&
    tint[1] === rgb[1] &&
    tint[2] === rgb[2];

  const isCustom =
    tint !== null && !PRESET_COLORS.some((c) => isPreset(c.rgb));

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-1.5">
        {allowOriginal && (
          <button
            type="button"
            onClick={() => onChange(null)}
            title="使用签名原色"
            className={cn(
              "relative h-7 w-7 shrink-0 overflow-hidden rounded border transition",
              "bg-[repeating-conic-gradient(#444_0%_25%,#222_0%_50%)] bg-[length:8px_8px]",
              tint === null
                ? "border-primary ring-1 ring-primary/60"
                : "border-border/50 hover:border-primary/60",
            )}
          >
            <span
              className={cn(
                "absolute inset-0 flex items-center justify-center text-[9px] font-medium",
                tint === null ? "text-primary" : "text-muted-foreground",
              )}
            >
              原
            </span>
          </button>
        )}

        {PRESET_COLORS.map((c) => (
          <button
            key={c.label}
            type="button"
            onClick={() => onChange(c.rgb)}
            title={c.label}
            style={{ backgroundColor: rgbToHex(c.rgb) }}
            className={cn(
              "h-7 w-7 shrink-0 rounded border transition",
              isPreset(c.rgb)
                ? "border-primary ring-1 ring-primary/60"
                : "border-border/50 hover:border-primary/60",
            )}
          />
        ))}

        <button
          type="button"
          onClick={() => inputRef.current?.click()}
          title="自定义颜色"
          style={{
            backgroundColor: isCustom ? currentHex : "transparent",
          }}
          className={cn(
            "relative h-7 w-7 shrink-0 rounded border transition",
            isCustom
              ? "border-primary ring-1 ring-primary/60"
              : "border-border/50 border-dashed hover:border-primary/60",
          )}
        >
          {!isCustom && (
            <span className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
              +
            </span>
          )}
        </button>

        <input
          ref={inputRef}
          type="color"
          value={currentHex}
          onChange={(e) => {
            const rgb = hexToRgb(e.target.value);
            if (rgb) onChange(rgb);
          }}
          className="sr-only"
        />
      </div>
    </div>
  );
}

function Slider({
  min,
  max,
  step,
  value,
  onChange,
}: {
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => onChange(parseFloat(e.target.value))}
      className={cn(
        "h-1 w-full cursor-pointer appearance-none rounded-full bg-muted",
        "[&::-webkit-slider-thumb]:appearance-none",
        "[&::-webkit-slider-thumb]:h-3.5 [&::-webkit-slider-thumb]:w-3.5",
        "[&::-webkit-slider-thumb]:rounded-full",
        "[&::-webkit-slider-thumb]:bg-primary",
        "[&::-webkit-slider-thumb]:shadow-md",
        "[&::-webkit-slider-thumb]:cursor-grab",
        "[&::-moz-range-thumb]:h-3.5 [&::-moz-range-thumb]:w-3.5",
        "[&::-moz-range-thumb]:rounded-full",
        "[&::-moz-range-thumb]:bg-primary",
        "[&::-moz-range-thumb]:border-0",
      )}
      style={{
        background: `linear-gradient(to right, oklch(0.922 0 0) 0%, oklch(0.922 0 0) ${
          ((value - min) / (max - min)) * 100
        }%, oklch(0.269 0 0) ${((value - min) / (max - min)) * 100}%, oklch(0.269 0 0) 100%)`,
      }}
    />
  );
}
