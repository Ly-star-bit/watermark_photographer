import { cn } from "@/lib/utils";
import type { ExportOptions, OutputFormat } from "@/lib/types";

interface Props {
  options: ExportOptions;
  filenameTemplate: string;
  onOptionsChange: (opts: ExportOptions) => void;
  onFilenameTemplateChange: (tpl: string) => void;
}

const FORMATS: { value: OutputFormat; label: string }[] = [
  { value: "jpeg", label: "JPEG" },
  { value: "png", label: "PNG" },
  { value: "webp", label: "WebP" },
];

/** 变量提示 */
const VARIABLES = [
  { key: "{stem}", desc: "原文件名" },
  { key: "{n}", desc: "序号（001）" },
  { key: "{date}", desc: "日期（YYYYMMDD）" },
];

export function ExportSettings({
  options,
  filenameTemplate,
  onOptionsChange,
  onFilenameTemplateChange,
}: Props) {
  return (
    <div className="space-y-6">


      {/* 格式选择 */}

      {/* 格式选择 */}
      <div className="space-y-2">
        <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          格式
        </label>
        <div className="flex gap-1">
          {FORMATS.map((f) => (
            <button
              key={f.value}
              type="button"
              onClick={() => onOptionsChange({ ...options, format: f.value })}
              className={cn(
                "flex-1 rounded-md border px-2 py-1.5 text-xs transition",
                options.format === f.value
                  ? "border-primary/70 bg-primary/15 text-primary ring-1 ring-primary/50"
                  : "border-border/40 bg-card/40 text-muted-foreground hover:border-primary/40",
              )}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      {/* 质量滑块（JPEG/WebP 时显示） */}
      {options.format !== "png" && (
        <div className="space-y-2">
          <div className="flex items-baseline justify-between">
            <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              质量
            </label>
            <span className="text-[11px] tabular-nums text-muted-foreground/80">
              {options.quality}
            </span>
          </div>
          <input
            type="range"
            min={1}
            max={100}
            step={1}
            value={options.quality}
            onChange={(e) =>
              onOptionsChange({
                ...options,
                quality: parseInt(e.target.value, 10),
              })
            }
            className="h-1 w-full cursor-pointer appearance-none rounded-full bg-muted [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:h-3.5 [&::-webkit-slider-thumb]:w-3.5 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-primary [&::-webkit-slider-thumb]:shadow-md [&::-webkit-slider-thumb]:cursor-grab"
            style={{
              background: `linear-gradient(to right, oklch(0.922 0 0) 0%, oklch(0.922 0 0) ${options.quality}%, oklch(0.269 0 0) ${options.quality}%, oklch(0.269 0 0) 100%)`,
            }}
          />
        </div>
      )}

      {/* 长边限制 */}
      <div className="space-y-2">
        <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          长边限制
        </label>
        <div className="flex items-center gap-1.5">
          <input
            type="number"
            placeholder="原尺寸"
            value={options.max_long_side ?? ""}
            onChange={(e) => {
              const v = e.target.value;
              onOptionsChange({
                ...options,
                max_long_side:
                  v === "" ? null : Math.max(1, parseInt(v, 10) || 1),
              });
            }}
            className="h-8 w-full rounded-md border border-border/60 bg-card/40 px-2.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:border-primary/60 focus:outline-none"
          />
          <span className="text-[11px] text-muted-foreground shrink-0">px</span>
        </div>
        <p className="text-[10px] text-muted-foreground/60">
          留空=原尺寸。常用值：微博/Ins 2048px
        </p>
      </div>

      {/* 文件名模板 */}
      <div className="space-y-2">
        <label className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          文件名模板
        </label>
        <input
          type="text"
          value={filenameTemplate}
          onChange={(e) => onFilenameTemplateChange(e.target.value)}
          placeholder="{stem}_wm"
          className="h-8 w-full rounded-md border border-border/60 bg-card/40 px-2.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:border-primary/60 focus:outline-none font-mono"
        />
        <div className="flex flex-wrap gap-x-3 gap-y-0.5">
          {VARIABLES.map((v) => (
            <span
              key={v.key}
              className="text-[10px] text-muted-foreground/70"
            >
              <code className="rounded bg-muted/50 px-0.5 text-[10px]">
                {v.key}
              </code>
              {" "}{v.desc}
            </span>
          ))}
        </div>
        <PreviewFilename template={filenameTemplate} />
      </div>
    </div>
  );
}

/** 文件名实时预览 */
function PreviewFilename({ template }: { template: string }) {
  const stem = "DSCF0001";
  const preview = template
    .replace(/{stem}/g, stem)
    .replace(/{n}/g, "001")
    .replace(/{date}/g, new Date().toISOString().slice(0, 10).replace(/-/g, ""));

  return (
    <p className="truncate text-[10px] text-muted-foreground/60">
      预览：{preview}.jpg
    </p>
  );
}
