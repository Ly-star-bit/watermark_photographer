import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Aperture,
  FolderOpen,
  ImagePlus,
  Play,
  Radio,
  Settings2,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { join } from "@tauri-apps/api/path";
import { DropZone } from "@/components/DropZone";
import { FileList } from "@/components/FileList";
import { PreviewCanvas } from "@/components/PreviewCanvas";
import { WatermarkPanel } from "@/components/WatermarkPanel";
import { ExportSettings } from "@/components/ExportSettings";
import { BatchProgressPanel } from "@/components/BatchProgress";
import { PresetManager } from "@/components/PresetManager";
import { WatchPanel, type WatchLogEntry } from "@/components/WatchPanel";
import { cn, subscribeAsync } from "@/lib/utils";
import {
  basename,
  createThumbnail,
  exportBatch,
  onBatchProgress,
  onWatchFileProcessed,
  onWatchFileStarted,
  pickInputDir,
  pickOutputDir,
  startWatch,
  stopWatch,
  toPhotoFile,
  updateWatchConfig,
  type BatchProgress,
  type BatchSummary,
  type Preset,
} from "@/lib/api";
import {
  DEFAULT_CONFIG,
  DEFAULT_EXPORT_OPTIONS,
  DEFAULT_FILENAME_TEMPLATE,
  type ExportOptions,
  type PhotoFile,
  type WatermarkConfig,
} from "@/lib/types";

function App() {
  const [photos, setPhotos] = useState<PhotoFile[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [watermarkPath, setWatermarkPathState] = useState<string | null>(null);
  const [config, setConfig] = useState<WatermarkConfig>(DEFAULT_CONFIG);
  const [outputDir, setOutputDir] = useState<string | null>(null);
  const [activePresetName, setActivePresetName] = useState<string | null>(null);
  const [exportOptions, setExportOptions] = useState<ExportOptions>(DEFAULT_EXPORT_OPTIONS);
  const [filenameTemplate, setFilenameTemplate] = useState(DEFAULT_FILENAME_TEMPLATE);
  const [showExportSettings, setShowExportSettings] = useState(false);

  // 批量导出状态
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<BatchProgress | null>(null);
  const [summary, setSummary] = useState<BatchSummary | null>(null);

  // 左栏模式：手动选照片 / 监听文件夹
  const [mode, setMode] = useState<"manual" | "watch">("manual");
  const [watchInputDir, setWatchInputDir] = useState<string | null>(null);
  // 监听模式的输出目录独立于顶部"批量导出"用的 outputDir（两种工作流不必绑定同一目标），
  // 选定监听文件夹后自动默认为其下的 sign-output 子目录，用户可再手动改
  const [watchOutputDir, setWatchOutputDir] = useState<string | null>(null);
  const [watching, setWatching] = useState(false);
  const [watchLog, setWatchLog] = useState<WatchLogEntry[]>([]);

  // 订阅 Rust 端进度事件
  useEffect(
    () => subscribeAsync(() => onBatchProgress((p) => setProgress(p))),
    [],
  );

  // 订阅监听文件夹"开始处理"事件：先插入一条 processing 占位，给用户及时反馈
  useEffect(
    () =>
      subscribeAsync(() =>
        onWatchFileStarted(({ input }) => {
          setWatchLog((prev) => [
            {
              id: `${Date.now()}-${Math.random()}`,
              input,
              output: null,
              error: null,
              status: "processing",
              timestamp: Date.now(),
            },
            ...prev,
          ]);
        }),
      ),
    [],
  );

  // 订阅监听文件夹的处理结果事件：把对应的 processing 占位替换为最终结果
  useEffect(
    () =>
      subscribeAsync(() =>
        onWatchFileProcessed((r) => {
          setWatchLog((prev) => {
            const idx = prev.findIndex(
              (e) => e.status === "processing" && e.input === r.input,
            );
            if (idx === -1) {
              return [
                {
                  ...r,
                  id: `${Date.now()}-${Math.random()}`,
                  status: "done",
                  timestamp: Date.now(),
                },
                ...prev,
              ];
            }
            const next = [...prev];
            next[idx] = { ...next[idx], ...r, status: "done", timestamp: Date.now() };
            return next;
          });
        }),
      ),
    [],
  );

  // 监听中：右侧面板改水印设置时防抖同步到正在运行的监听任务，
  // 避免"面板显示的设置"和"实际生效的设置"不一致（此前的设计缺陷）
  useEffect(() => {
    if (!watching || !watermarkPath) return;
    const timer = setTimeout(() => {
      updateWatchConfig({
        watermarkPath,
        config,
        exportOptions,
        filenameTemplate,
      }).catch(() => {
        // 监听可能恰好被停止，静默忽略
      });
    }, 400);
    return () => clearTimeout(timer);
  }, [watching, watermarkPath, config, exportOptions, filenameTemplate]);

  // 组件卸载兜底：若还在监听则通知 Rust 停止（用 ref 避免每次 watching 变化都重新绑定卸载逻辑）
  const watchingRef = useRef(watching);
  watchingRef.current = watching;
  useEffect(() => {
    return () => {
      if (watchingRef.current) stopWatch().catch(() => {});
    };
  }, []);

  const addPhotos = useCallback((paths: string[]) => {
    let toGenerate: string[] = [];
    setPhotos((prev) => {
      const existing = new Set(prev.map((p) => p.path));
      const additions = paths.filter((p) => !existing.has(p)).map(toPhotoFile);
      toGenerate = additions.map((p) => p.path);
      const next = [...prev, ...additions];
      if (prev.length === 0 && next.length > 0) setSelectedIndex(0);
      return next;
    });

    // 异步为新加入的照片生成缩略图，不阻塞界面
    for (const path of toGenerate) {
      createThumbnail(path, 240)
        .then((url) => {
          setPhotos((cur) =>
            cur.map((p) => (p.path === path ? { ...p, thumbnailUrl: url } : p)),
          );
        })
        .catch(() => {
          // 静默失败：无缩略图不影响主功能
        });
    }
  }, []);

  const removePhoto = useCallback((idx: number) => {
    setPhotos((prev) => {
      const target = prev[idx];
      if (target?.thumbnailUrl) URL.revokeObjectURL(target.thumbnailUrl);
      return prev.filter((_, i) => i !== idx);
    });
    setSelectedIndex((prev) => {
      if (idx < prev) return prev - 1;
      if (idx === prev) return Math.max(0, prev);
      return prev;
    });
  }, []);

  const clearAllPhotos = useCallback(() => {
    setPhotos((prev) => {
      prev.forEach((p) => p.thumbnailUrl && URL.revokeObjectURL(p.thumbnailUrl));
      return [];
    });
    setSelectedIndex(0);
  }, []);

  // 手动改配置/水印图 → 视为脱离当前预设
  const patchConfig = useCallback((patch: Partial<WatermarkConfig>) => {
    setConfig((prev) => ({ ...prev, ...patch }));
    setActivePresetName(null);
  }, []);

  const setWatermarkPath = useCallback((path: string | null) => {
    setWatermarkPathState(path);
    setActivePresetName(null);
  }, []);

  // 应用预设：一次性覆盖 config + 水印路径，不触发脱离逻辑
  const applyPreset = useCallback((preset: Preset) => {
    setConfig(preset.config);
    setWatermarkPathState(preset.watermark_path);
  }, []);

  const selectedPhoto = photos[selectedIndex] ?? null;
  const watermarkUrl = useMemo(
    () => (watermarkPath ? convertFileSrc(watermarkPath) : null),
    [watermarkPath],
  );

  const canExport =
    photos.length > 0 && watermarkPath !== null && outputDir !== null && !running;

  const handleExport = async () => {
    if (!canExport || !watermarkPath || !outputDir) return;
    setRunning(true);
    setProgress(null);
    setSummary(null);
    try {
      const result = await exportBatch({
        inputPaths: photos.map((p) => p.path),
        outputDir,
        watermarkPath,
        config,
        exportOptions,
        filenameTemplate,
      });
      setSummary(result);
    } catch (e) {
      setSummary({
        total: photos.length,
        success: 0,
        failed: photos.length,
        items: photos.map((p) => ({
          input: p.path,
          output: null,
          error: String(e),
        })),
      });
    } finally {
      setRunning(false);
    }
  };

  const handleChooseOutputDir = async () => {
    const dir = await pickOutputDir();
    if (dir) setOutputDir(dir);
  };

  const canStartWatch =
    watchInputDir !== null &&
    watchOutputDir !== null &&
    watermarkPath !== null &&
    !watching;

  const handlePickWatchInputDir = async () => {
    const dir = await pickInputDir();
    if (!dir) return;
    setWatchInputDir(dir);
    // 默认输出到监听文件夹下的 sign-output 子目录（非递归监听不会把这个子目录里
    // 新写入的文件当成"新输入"，不会造成循环处理），用户可在面板里再手动改
    try {
      setWatchOutputDir(await join(dir, "sign-output"));
    } catch {
      // 拼接失败（极少见）就留给用户手动选择输出目录
    }
  };

  const handlePickWatchOutputDir = async () => {
    const dir = await pickOutputDir();
    if (dir) setWatchOutputDir(dir);
  };

  const handleStartWatch = async () => {
    if (!canStartWatch || !watchInputDir || !watermarkPath || !watchOutputDir) return;
    try {
      await startWatch({
        inputDir: watchInputDir,
        outputDir: watchOutputDir,
        watermarkPath,
        config,
        exportOptions,
        filenameTemplate,
      });
      setWatchLog([]);
      setWatching(true);
    } catch (e) {
      alert(`启动监听失败: ${e}`);
    }
  };

  const handleStopWatch = async () => {
    try {
      await stopWatch();
    } finally {
      setWatching(false);
    }
  };

  return (
    <div className="dark flex h-screen flex-col bg-background text-foreground">
      {/* 顶部标题栏 */}
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-border/60 bg-card/40 px-4 backdrop-blur">
        <div className="flex items-center gap-2">
          <Aperture className="h-5 w-5 text-primary" />
          <span className="text-sm font-semibold tracking-wide">
            签名水印{" "}
            <span className="text-muted-foreground font-normal">
              · Watermark Studio
            </span>
          </span>
        </div>
        <div className="flex items-center gap-2">
          <OutputDirButton dir={outputDir} onPick={handleChooseOutputDir} />
          <button
            type="button"
            onClick={() => setShowExportSettings(true)}
            className="inline-flex items-center gap-1.5 rounded-md border border-border/60 bg-card/40 px-2.5 py-1.5 text-[11px] text-muted-foreground transition hover:border-primary/50 hover:text-foreground"
            title="导出设置"
          >
            <SlidersHorizontal className="h-3.5 w-3.5" />
            设置
          </button>
          {mode === "manual" && (
            <button
              disabled={!canExport}
              className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
              onClick={handleExport}
            >
              <Play className="h-3.5 w-3.5" />
              批量导出
            </button>
          )}
        </div>
      </header>

      {/* 主体三栏 */}
      <div className="relative flex flex-1 min-h-0">
        {/* 左栏：手动照片列表 / 监听文件夹 */}
        <aside className="flex w-64 shrink-0 flex-col border-r border-border/60 bg-card/20">
          <div className="flex h-9 shrink-0 items-center border-b border-border/40 px-2">
            <div className="flex rounded-md bg-muted/40 p-0.5">
              <button
                type="button"
                onClick={() => setMode("manual")}
                className={cn(
                  "flex items-center gap-1 rounded-sm px-2 py-1 text-[11px] transition",
                  mode === "manual"
                    ? "bg-card text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <ImagePlus className="h-3 w-3" />
                照片
                {photos.length > 0 && (
                  <span className="ml-0.5 rounded bg-muted px-1 text-[10px] font-normal">
                    {photos.length}
                  </span>
                )}
              </button>
              <button
                type="button"
                onClick={() => setMode("watch")}
                className={cn(
                  "flex items-center gap-1 rounded-sm px-2 py-1 text-[11px] transition",
                  mode === "watch"
                    ? "bg-card text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Radio className="h-3 w-3" />
                监听文件夹
                {watching && (
                  <span className="ml-0.5 h-1.5 w-1.5 rounded-full bg-emerald-400" />
                )}
              </button>
            </div>
            {mode === "manual" && photos.length > 0 && (
              <button
                type="button"
                onClick={clearAllPhotos}
                className="ml-auto inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-destructive transition"
                title="清空全部照片"
              >
                <Trash2 className="h-3 w-3" />
                清空
              </button>
            )}
          </div>
          {mode === "manual" ? (
            <div className="flex-1 overflow-y-auto p-3">
              <DropZone onFiles={addPhotos} />
              <FileList
                photos={photos}
                selectedIndex={selectedIndex}
                onSelect={setSelectedIndex}
                onRemove={removePhoto}
              />
            </div>
          ) : (
            <div className="flex-1 overflow-y-auto p-3">
              <WatchPanel
                inputDir={watchInputDir}
                onPickInputDir={handlePickWatchInputDir}
                outputDir={watchOutputDir}
                onPickOutputDir={handlePickWatchOutputDir}
                watermarkSelected={watermarkPath !== null}
                watching={watching}
                canStart={canStartWatch}
                onStart={handleStartWatch}
                onStop={handleStopWatch}
                log={watchLog}
              />
            </div>
          )}
        </aside>

        {/* 中栏：预览 */}
        <main className="flex flex-1 flex-col bg-background">
          <SectionHeader
            title="预览"
            hint={
              selectedPhoto
                ? selectedPhoto.name
                : photos.length > 0
                  ? "选择一张照片"
                  : "拖拽照片到左侧开始"
            }
          />
          <div className="relative flex flex-1 items-center justify-center p-8 min-h-0">
            <PreviewCanvas
              photo={selectedPhoto}
              watermarkUrl={watermarkUrl}
              config={config}
            />
          </div>
        </main>

        {/* 右栏：水印设置 */}
        <aside className="flex w-80 shrink-0 flex-col border-l border-border/60 bg-card/20">
          <SectionHeader
            icon={<Settings2 className="h-3.5 w-3.5" />}
            title="水印设置"
          />
          <div className="flex-1 overflow-y-auto p-4">
            <WatermarkPanel
              watermarkPath={watermarkPath}
              onWatermarkChange={setWatermarkPath}
              config={config}
              onConfigChange={patchConfig}
            />
          </div>
          <div className="border-t border-border/60 p-3">
            <PresetManager
              currentConfig={config}
              currentWatermarkPath={watermarkPath}
              activePresetName={activePresetName}
              onApply={applyPreset}
              onActiveChange={setActivePresetName}
            />
          </div>
        </aside>

        {/* 批量进度遮罩 */}
        <BatchProgressPanel
          running={running}
          progress={progress}
          summary={summary}
          outputDir={outputDir}
          onClose={() => setSummary(null)}
        />

        {/* 导出设置弹窗 */}
        {showExportSettings && (
          <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <div className="w-[420px] max-h-[80vh] rounded-lg border border-border/60 bg-card p-6 shadow-2xl flex flex-col">
              <div className="flex items-center justify-between mb-4 shrink-0">
                <div className="flex items-center gap-2">
                  <SlidersHorizontal className="h-4 w-4 text-primary" />
                  <span className="text-sm font-semibold">导出设置</span>
                </div>
                <button
                  type="button"
                  onClick={() => setShowExportSettings(false)}
                  className="rounded p-0.5 text-muted-foreground hover:text-foreground transition"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
              <div className="flex-1 overflow-y-auto">
                <ExportSettings
                  options={exportOptions}
                  filenameTemplate={filenameTemplate}
                  onOptionsChange={setExportOptions}
                  onFilenameTemplateChange={setFilenameTemplate}
                />
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 底部状态栏 */}
      <footer className="flex h-6 shrink-0 items-center justify-between border-t border-border/60 bg-card/40 px-4 text-[11px] text-muted-foreground">
        <span>
          {photos.length > 0
            ? `${photos.length} 张${watermarkPath ? " · 已选签名图" : " · 未选签名图"}${
                outputDir ? " · 已选输出目录" : " · 未选输出目录"
              }`
            : "就绪"}
        </span>
        <span>P5 · Tauri 2 · React 19</span>
      </footer>
    </div>
  );
}

function OutputDirButton({
  dir,
  onPick,
}: {
  dir: string | null;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onPick}
      className="inline-flex max-w-[240px] items-center gap-1.5 rounded-md border border-border/60 bg-card/40 px-2.5 py-1.5 text-[11px] text-muted-foreground transition hover:border-primary/50 hover:text-foreground"
      title={dir ?? "选择输出目录"}
    >
      <FolderOpen className="h-3.5 w-3.5 shrink-0" />
      <span className="truncate">
        {dir ? basename(dir) : "选择输出目录"}
      </span>
    </button>
  );
}

function SectionHeader({
  icon,
  title,
  count,
  hint,
}: {
  icon?: React.ReactNode;
  title: string;
  count?: number;
  hint?: string;
}) {
  return (
    <div className="flex h-9 shrink-0 items-center justify-between border-b border-border/40 px-3">
      <div className="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {icon}
        {title}
        {typeof count === "number" && (
          <span className="ml-1 rounded bg-muted px-1.5 py-0.5 text-[10px] font-normal normal-case tracking-normal">
            {count}
          </span>
        )}
      </div>
      {hint && (
        <span
          className="max-w-[60%] truncate text-[11px] text-muted-foreground/70"
          title={hint}
        >
          {hint}
        </span>
      )}
    </div>
  );
}

export default App;
