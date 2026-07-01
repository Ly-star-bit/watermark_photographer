import { X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { PhotoFile } from "@/lib/types";

interface Props {
  photos: PhotoFile[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  onRemove: (index: number) => void;
}

/** 已导入照片列表（缩略图 + 文件名 + 删除按钮） */
export function FileList({ photos, selectedIndex, onSelect, onRemove }: Props) {
  if (photos.length === 0) return null;

  return (
    <ul className="mt-3 space-y-1.5">
      {photos.map((p, i) => (
        <li
          key={p.path}
          className={cn(
            "group flex items-center gap-2 rounded-md p-1.5 pr-2 cursor-pointer transition-colors",
            i === selectedIndex
              ? "bg-primary/15 ring-1 ring-primary/40"
              : "hover:bg-card/60",
          )}
          onClick={() => onSelect(i)}
        >
          <div className="h-10 w-10 shrink-0 overflow-hidden rounded bg-black/40">
            {p.thumbnailUrl ? (
              <img
                src={p.thumbnailUrl}
                alt={p.name}
                className="h-full w-full object-cover"
                loading="lazy"
                decoding="async"
              />
            ) : (
              <div className="h-full w-full animate-pulse bg-muted/50" />
            )}
          </div>
          <span
            className="flex-1 truncate text-[11px] text-muted-foreground"
            title={p.path}
          >
            {p.name}
          </span>
          <button
            type="button"
            className="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive transition"
            onClick={(e) => {
              e.stopPropagation();
              onRemove(i);
            }}
            aria-label="移除"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </li>
      ))}
    </ul>
  );
}
