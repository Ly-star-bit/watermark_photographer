import { CheckCircle2, FolderInput, FolderOutput, Loader2, Radio, XCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import { basename } from "@/lib/api";

/**
 * 监听日志条目：
 * - status "processing"：检测到新文件、正在等待写入稳定/合成中（output/error 均为 null）
 * - status "done"：处理已结束，success 由 error 是否为 null 判断
 */
export interface WatchLogEntry {
  id: string;
  input: string;
  output: string | null;
  error: string | null;
  status: "processing" | "done";
  timestamp: number;
}

interface Props {
  inputDir: string | null;
  onPickInputDir: () => void;
  outputDir: string | null;
  onPickOutputDir: () => void;
  watermarkSelected: boolean;
  watching: boolean;
  canStart: boolean;
  onStart: () => void;
  onStop: () => void;
  log: WatchLogEntry[];
}

/**
 * 监听文件夹面板：选输入/输出文件夹 + 开始/停止 + 处理日志。
 * 输出目录独立于顶部"批量导出"用的目录，选定监听文件夹后自动默认为其下的
 * sign-output 子目录（非递归监听不会把这个子目录当成新输入，不会循环处理），
 * 可点击手动改成别的目录。水印配置复用 App 顶层的 config/watermarkPath，
 * 与手动批量导出用同一份，切到本面板即生效，不引入单独的预设选择。
 */
export function WatchPanel({
  inputDir,
  onPickInputDir,
  outputDir,
  onPickOutputDir,
  watermarkSelected,
  watching,
  canStart,
  onStart,
  onStop,
  log,
}: Props) {
  return (
    <div className="flex h-full flex-col gap-3">
      {/* 输入文件夹选择 */}
      <button
        type="button"
        onClick={onPickInputDir}
        disabled={watching}
        className={cn(
          "flex h-16 w-full flex-col items-center justify-center gap-1 rounded-lg border border-dashed text-center transition-colors",
          watching
            ? "cursor-not-allowed border-border/40 bg-card/20 opacity-60"
            : "border-border/60 bg-card/30 hover:border-primary/60 hover:bg-card/50",
        )}
      >
        <div className="flex items-center gap-1.5 text-xs text-foreground">
          <FolderInput className="h-3.5 w-3.5" />
          {inputDir ? basename(inputDir) : "选择监听文件夹"}
        </div>
        {inputDir && (
          <p className="max-w-full truncate px-3 text-[10px] text-muted-foreground/60">
            {inputDir}
          </p>
        )}
      </button>

      {/* 输出文件夹：选好输入文件夹后自动默认为 {输入文件夹}/sign-output，可手动改 */}
      <button
        type="button"
        onClick={onPickOutputDir}
        disabled={watching || !inputDir}
        className={cn(
          "flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-left text-[11px] transition-colors",
          watching || !inputDir
            ? "cursor-not-allowed border-border/40 bg-card/20 text-muted-foreground/50"
            : "border-border/60 bg-card/30 text-muted-foreground hover:border-primary/50 hover:text-foreground",
        )}
      >
        <FolderOutput className="h-3.5 w-3.5 shrink-0" />
        <span className="truncate">
          {outputDir ? `输出到 ${basename(outputDir)}（点击修改）` : "先选监听文件夹"}
        </span>
      </button>

      {/* 开始/停止 */}
      {watching ? (
        <button
          type="button"
          onClick={onStop}
          className="flex items-center justify-center gap-1.5 rounded-md bg-destructive/90 px-3 py-2 text-xs font-medium text-white transition hover:opacity-90"
        >
          <Radio className="h-3.5 w-3.5 animate-pulse" />
          监听中 · 点击停止
        </button>
      ) : (
        <button
          type="button"
          onClick={onStart}
          disabled={!canStart}
          className="flex items-center justify-center gap-1.5 rounded-md bg-primary px-3 py-2 text-xs font-medium text-primary-foreground transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Radio className="h-3.5 w-3.5" />
          开始监听
        </button>
      )}

      {!watching && !canStart && (
        <p className="text-[10px] leading-relaxed text-amber-400/90">
          还需要：
          {!inputDir && "选择监听文件夹 "}
          {!outputDir && "选择输出文件夹 "}
          {!watermarkSelected && "在右侧选一张签名图 "}
        </p>
      )}
      <p className="text-[10px] leading-relaxed text-muted-foreground/60">
        启动后仅处理之后新写入的图片文件（不处理文件夹里已有的旧文件），用右侧当前水印设置处理。
      </p>

      {/* 处理日志 */}
      <div className="flex-1 overflow-y-auto rounded-md border border-border/40 bg-black/20 p-2">
        {log.length === 0 ? (
          <p className="p-2 text-center text-[11px] text-muted-foreground/50">
            {watching ? "等待新文件写入监听文件夹…" : "尚未开始监听"}
          </p>
        ) : (
          <ul className="space-y-1">
            {log.map((entry) => (
              <li key={entry.id} className="text-[11px]">
                <div className="flex items-center gap-1.5">
                  {entry.status === "processing" ? (
                    <Loader2 className="h-3 w-3 shrink-0 animate-spin text-muted-foreground" />
                  ) : entry.error ? (
                    <XCircle className="h-3 w-3 shrink-0 text-destructive" />
                  ) : (
                    <CheckCircle2 className="h-3 w-3 shrink-0 text-emerald-400" />
                  )}
                  <span className="truncate text-foreground" title={entry.input}>
                    {basename(entry.input)}
                  </span>
                  {entry.status === "processing" && (
                    <span className="shrink-0 text-[10px] text-muted-foreground/60">
                      处理中…
                    </span>
                  )}
                  <span className="ml-auto shrink-0 text-[10px] text-muted-foreground/50">
                    {formatTime(entry.timestamp)}
                  </span>
                </div>
                {entry.error && (
                  <div className="ml-[18px] text-[10px] text-destructive/80">
                    {entry.error}
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function formatTime(ts: number): string {
  const d = new Date(ts);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}
