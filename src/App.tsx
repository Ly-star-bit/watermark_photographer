import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Aperture,
  FolderOpen,
  ImagePlus,
  Play,
  Settings2,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { DropZone } from "@/components/DropZone";
import { FileList } from "@/components/FileList";
import { PreviewCanvas } from "@/components/PreviewCanvas";
import { WatermarkPanel } from "@/components/WatermarkPanel";
import { ExportSettings } from "@/components/ExportSettings";
import { BatchProgressPanel } from "@/components/BatchProgress";
import { PresetManager } from "@/components/PresetManager";
import {
  basename,
  createThumbnail,
  exportBatch,
  onBatchProgress,
  pickOutputDir,
  toPhotoFile,
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

  // 订阅 Rust 端进度事件
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onBatchProgress((p) => setProgress(p)).then((fn) => (unlisten = fn));
    return () => unlisten?.();
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
          <button
            disabled={!canExport}
            className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
            onClick={handleExport}
          >
            <Play className="h-3.5 w-3.5" />
            批量导出
          </button>
        </div>
      </header>

      {/* 主体三栏 */}
      <div className="relative flex flex-1 min-h-0">
        {/* 左栏：文件列表 */}
        <aside className="flex w-64 shrink-0 flex-col border-r border-border/60 bg-card/20">
          <div className="flex h-9 shrink-0 items-center justify-between border-b border-border/40 px-3">
            <div className="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              <ImagePlus className="h-3.5 w-3.5" />
              照片
              <span className="ml-1 rounded bg-muted px-1.5 py-0.5 text-[10px] font-normal normal-case tracking-normal">
                {photos.length}
              </span>
            </div>
            {photos.length > 0 && (
              <button
                type="button"
                onClick={clearAllPhotos}
                className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-destructive transition"
                title="清空全部照片"
              >
                <Trash2 className="h-3 w-3" />
                清空
              </button>
            )}
          </div>
          <div className="flex-1 overflow-y-auto p-3">
            <DropZone onFiles={addPhotos} />
            <FileList
              photos={photos}
              selectedIndex={selectedIndex}
              onSelect={setSelectedIndex}
              onRemove={removePhoto}
            />
          </div>
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
