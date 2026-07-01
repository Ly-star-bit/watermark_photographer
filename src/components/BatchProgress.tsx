import { CheckCircle2, XCircle, Loader2, FolderOpen } from "lucide-react";
import { cn } from "@/lib/utils";
import type { BatchProgress, BatchSummary } from "@/lib/api";
import { openInFileManager } from "@/lib/api";

interface Props {
  running: boolean;
  progress: BatchProgress | null;
  summary: BatchSummary | null;
  outputDir: string | null;
  onClose: () => void;
}

/**
 * 全屏遮罩式的批量进度面板。
 * running=true 时显示实时进度条，
 * running=false && summary 时显示完成汇总。
 */
export function BatchProgressPanel({
  running,
  progress,
  summary,
  outputDir,
  onClose,
}: Props) {
  if (!running && !summary) return null;

  const pct = progress
    ? Math.round((progress.done / Math.max(progress.total, 1)) * 100)
    : 0;

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[440px] rounded-lg border border-border/60 bg-card p-6 shadow-2xl">
        {running ? (
          <RunningView progress={progress} pct={pct} />
        ) : summary ? (
          <SummaryView
            summary={summary}
            outputDir={outputDir}
            onClose={onClose}
          />
        ) : null}
      </div>
    </div>
  );
}

function RunningView({
  progress,
  pct,
}: {
  progress: BatchProgress | null;
  pct: number;
}) {
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-sm font-medium">
        <Loader2 className="h-4 w-4 animate-spin text-primary" />
        正在批量导出
      </div>

      <div className="space-y-1.5">
        <div className="flex items-baseline justify-between text-xs">
          <span className="text-muted-foreground">
            {progress ? `${progress.done} / ${progress.total}` : "准备中..."}
          </span>
          <span className="text-primary tabular-nums">{pct}%</span>
        </div>
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
          <div
            className="h-full bg-primary transition-all duration-150"
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>

      {progress && (
        <div
          className="truncate text-[11px] text-muted-foreground"
          title={progress.filename}
        >
          <span className={cn(progress.ok ? "text-emerald-400" : "text-destructive")}>
            {progress.ok ? "✓" : "✗"}
          </span>{" "}
          {progress.filename}
        </div>
      )}
    </div>
  );
}

function SummaryView({
  summary,
  outputDir,
  onClose,
}: {
  summary: BatchSummary;
  outputDir: string | null;
  onClose: () => void;
}) {
  const allOk = summary.failed === 0;

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-sm font-medium">
        {allOk ? (
          <CheckCircle2 className="h-5 w-5 text-emerald-400" />
        ) : (
          <XCircle className="h-5 w-5 text-destructive" />
        )}
        {allOk ? "全部导出完成" : "导出完成（部分失败）"}
      </div>

      <div className="grid grid-cols-3 gap-2 rounded-md bg-muted/40 p-3">
        <Stat label="总计" value={summary.total} />
        <Stat label="成功" value={summary.success} tone="ok" />
        <Stat label="失败" value={summary.failed} tone={summary.failed > 0 ? "err" : undefined} />
      </div>

      {summary.failed > 0 && (
        <div className="max-h-40 overflow-y-auto rounded-md border border-border/40 bg-black/30 p-2 text-[11px]">
          <div className="mb-1 font-medium text-destructive">失败明细：</div>
          <ul className="space-y-0.5">
            {summary.items
              .filter((it) => it.error)
              .map((it, i) => (
                <li key={i} className="text-muted-foreground">
                  <span className="text-destructive">✗</span> {it.input}
                  <div className="ml-3 text-destructive/80">{it.error}</div>
                </li>
              ))}
          </ul>
        </div>
      )}

      <div className="flex items-center justify-end gap-2 pt-1">
        {outputDir && summary.success > 0 && (
          <button
            type="button"
            onClick={async () => {
              try {
                await openInFileManager(outputDir);
              } catch (e) {
                alert(`打开目录失败: ${e}\n\n路径: ${outputDir}`);
              }
            }}
            className="inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-3 py-1.5 text-xs text-foreground transition hover:bg-accent"
          >
            <FolderOpen className="h-3.5 w-3.5" />
            打开输出目录
          </button>
        )}
        <button
          type="button"
          onClick={onClose}
          className="rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:opacity-90"
        >
          完成
        </button>
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone?: "ok" | "err";
}) {
  return (
    <div className="text-center">
      <div
        className={cn(
          "text-xl font-semibold tabular-nums",
          tone === "ok" && "text-emerald-400",
          tone === "err" && "text-destructive",
        )}
      >
        {value}
      </div>
      <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
    </div>
  );
}
