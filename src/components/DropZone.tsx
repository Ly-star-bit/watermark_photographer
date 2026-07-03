import { useEffect, useState } from "react";
import { ImagePlus } from "lucide-react";
import { cn, subscribeAsync } from "@/lib/utils";
import { onImageDrop, pickImageFiles } from "@/lib/api";

interface Props {
  onFiles: (paths: string[]) => void;
}

/** 拖拽区 + 点击选择。窗口级 OS 拖入事件由 Tauri webview 提供。 */
export function DropZone({ onFiles }: Props) {
  const [dragging, setDragging] = useState(false);

  // 注册 OS 级别拖入监听（整窗口范围）
  useEffect(
    () => subscribeAsync(() => onImageDrop((paths) => onFiles(paths))),
    [onFiles],
  );

  const handleClick = async () => {
    const paths = await pickImageFiles();
    if (paths.length > 0) onFiles(paths);
  };

  return (
    <button
      type="button"
      onClick={handleClick}
      onDragEnter={() => setDragging(true)}
      onDragLeave={() => setDragging(false)}
      onDrop={() => setDragging(false)}
      className={cn(
        "flex h-40 w-full flex-col items-center justify-center rounded-lg border border-dashed bg-card/30 text-center transition-colors",
        dragging
          ? "border-primary bg-primary/10"
          : "border-border/60 hover:border-primary/60 hover:bg-card/50",
      )}
    >
      <ImagePlus className="h-6 w-6 text-muted-foreground mb-2" />
      <p className="text-xs text-muted-foreground">拖拽图片到此处</p>
      <p className="mt-0.5 text-[10px] text-muted-foreground/60">
        JPG / PNG / TIFF / WebP / BMP
      </p>
    </button>
  );
}
