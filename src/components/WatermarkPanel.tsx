import { useRef } from "react";
import { FileImage, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { GridPosition, WatermarkConfig } from "@/lib/types";
import { GRID_POSITIONS, hexToRgb, rgbToHex } from "@/lib/types";
import { pickPngFile, basename } from "@/lib/api";
import { convertFileSrc } from "@tauri-apps/api/core";

interface Props {
  watermarkPath: string | null;
  onWatermarkChange: (path: string | null) => void;
  config: WatermarkConfig;
  onConfigChange: (patch: Partial<WatermarkConfig>) => void;
}

export function WatermarkPanel({
  watermarkPath,
  onWatermarkChange,
  config,
  onConfigChange,
}: Props) {
  return (
    <div className="space-y-6">
      <Section title="签名图">
        <WatermarkPicker
          path={watermarkPath}
          onPick={onWatermarkChange}
        />
      </Section>

      <Section title="位置">
        <NineGrid
          selected={config.position}
          onSelect={(pos) => onConfigChange({ position: pos })}
        />
      </Section>

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

/** 常用颜色速选（覆盖多数摄影师签名场景） */
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
}: {
  tint: [number, number, number] | null;
  onChange: (t: [number, number, number] | null) => void;
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
        {/* 原色 */}
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

        {/* 预设色 */}
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

        {/* 自定义颜色 */}
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

        {/* 隐藏的原生 color 输入 */}
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
