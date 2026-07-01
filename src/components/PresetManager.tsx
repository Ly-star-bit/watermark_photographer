import { useEffect, useRef, useState } from "react";
import { Bookmark, Check, Plus, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { WatermarkConfig } from "@/lib/types";
import {
  deletePreset,
  listPresets,
  savePreset,
  type Preset,
} from "@/lib/api";

interface Props {
  currentConfig: WatermarkConfig;
  currentWatermarkPath: string | null;
  activePresetName: string | null;
  onApply: (preset: Preset) => void;
  onActiveChange: (name: string | null) => void;
}

/**
 * 预设管理器（右栏底部）。
 * - 点击预设名称：弹出列表，可切换
 * - 保存：把当前 config + watermarkPath 存为预设
 * - 同名预设直接覆盖（Rust 层 upsert 逻辑）
 */
export function PresetManager({
  currentConfig,
  currentWatermarkPath,
  activePresetName,
  onApply,
  onActiveChange,
}: Props) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const [menuOpen, setMenuOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveName, setSaveName] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);

  // 启动时加载
  useEffect(() => {
    listPresets().then(setPresets).catch(() => setPresets([]));
  }, []);

  // 点击外部关闭菜单
  useEffect(() => {
    if (!menuOpen && !saving) return;
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
        setSaving(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [menuOpen, saving]);

  const handleApply = (preset: Preset) => {
    onApply(preset);
    onActiveChange(preset.name);
    setMenuOpen(false);
  };

  const handleSaveConfirm = async () => {
    const name = saveName.trim();
    if (!name) return;
    try {
      const updated = await savePreset({
        name,
        config: currentConfig,
        watermark_path: currentWatermarkPath,
      });
      setPresets(updated);
      onActiveChange(name);
      setSaving(false);
      setSaveName("");
      setMenuOpen(false);
    } catch (e) {
      alert(`保存失败: ${e}`);
    }
  };

  const handleDelete = async (name: string) => {
    try {
      const updated = await deletePreset(name);
      setPresets(updated);
      if (activePresetName === name) onActiveChange(null);
    } catch (e) {
      alert(`删除失败: ${e}`);
    }
  };

  return (
    <div ref={menuRef} className="relative">
      {/* 触发条 */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={() => {
            setMenuOpen((v) => !v);
            setSaving(false);
          }}
          className="flex flex-1 items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition"
        >
          <Bookmark
            className={cn(
              "h-3.5 w-3.5",
              activePresetName && "fill-primary/60 text-primary",
            )}
          />
          <span className="truncate">
            {activePresetName ?? "无预设"}
          </span>
        </button>
        <button
          type="button"
          onClick={() => {
            setSaving(true);
            setMenuOpen(false);
            setSaveName(activePresetName ?? "");
          }}
          className="text-[11px] text-primary/80 hover:text-primary transition"
        >
          {activePresetName ? "另存" : "保存为预设"}
        </button>
      </div>

      {/* 保存表单 */}
      {saving && (
        <div className="absolute bottom-full left-0 right-0 mb-2 rounded-md border border-border bg-card p-3 shadow-xl">
          <label className="text-[11px] uppercase tracking-wider text-muted-foreground">
            预设名称
          </label>
          <input
            autoFocus
            type="text"
            value={saveName}
            onChange={(e) => setSaveName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSaveConfirm();
              if (e.key === "Escape") setSaving(false);
            }}
            placeholder="例如：微博发图"
            className="mt-1.5 w-full rounded border border-input bg-background px-2 py-1.5 text-xs outline-none focus:border-primary/60"
          />
          <div className="mt-2 flex items-center justify-end gap-1.5">
            <button
              type="button"
              onClick={() => setSaving(false)}
              className="rounded px-2 py-1 text-[11px] text-muted-foreground hover:text-foreground"
            >
              取消
            </button>
            <button
              type="button"
              onClick={handleSaveConfirm}
              disabled={!saveName.trim()}
              className="inline-flex items-center gap-1 rounded bg-primary px-2.5 py-1 text-[11px] font-medium text-primary-foreground disabled:opacity-40"
            >
              <Check className="h-3 w-3" />
              保存
            </button>
          </div>
        </div>
      )}

      {/* 预设列表 */}
      {menuOpen && (
        <div className="absolute bottom-full left-0 right-0 mb-2 max-h-80 overflow-y-auto rounded-md border border-border bg-card p-1 shadow-xl">
          {presets.length === 0 ? (
            <div className="px-3 py-6 text-center text-xs text-muted-foreground">
              还没有预设
              <div className="mt-1 text-[10px] text-muted-foreground/60">
                调好参数后点右侧「保存为预设」
              </div>
            </div>
          ) : (
            <ul>
              {presets.map((p) => (
                <li
                  key={p.name}
                  className={cn(
                    "group flex items-center gap-2 rounded px-2 py-1.5 text-xs cursor-pointer transition-colors",
                    p.name === activePresetName
                      ? "bg-primary/15 text-foreground"
                      : "text-muted-foreground hover:bg-accent hover:text-foreground",
                  )}
                  onClick={() => handleApply(p)}
                >
                  {p.name === activePresetName ? (
                    <Check className="h-3 w-3 shrink-0 text-primary" />
                  ) : (
                    <span className="h-3 w-3 shrink-0" />
                  )}
                  <span className="flex-1 truncate">{p.name}</span>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleDelete(p.name);
                    }}
                    className="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive transition"
                    aria-label="删除"
                  >
                    <Trash2 className="h-3 w-3" />
                  </button>
                </li>
              ))}
              <li className="mt-1 border-t border-border/40 pt-1">
                <button
                  type="button"
                  onClick={() => {
                    setSaving(true);
                    setMenuOpen(false);
                    setSaveName("");
                  }}
                  className="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-xs text-primary hover:bg-accent"
                >
                  <Plus className="h-3 w-3" />
                  新建预设
                </button>
              </li>
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
