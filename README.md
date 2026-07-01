# 签名水印 · Watermark Studio

<p align="right">
  <b>简体中文</b> · <a href="README.en.md">English</a>
</p>

一个给摄影师用的**批量签名水印**桌面工具。基于 Tauri 2 + Rust + React 19 构建，Windows 单文件 exe **仅 4 MB**。

---

## ✨ 特性

- 🎨 **PNG 透明签名图水印** — 支持透明底签名，可任意着色（原色 / 白 / 米白 / 灰阶 / 黑 / 自定义），撞白底也不糊
- 📷 **摄影师专属：EXIF / ICC 保留** — 相机型号、光圈、快门、ISO、拍摄时间、色彩空间全部原样保留（用 `img-parts` 精确搬运 JPEG 段）
- 🖼️ **多格式输入** — JPEG · PNG · TIFF · WebP · BMP（输出统一 JPEG）
- 🎯 **智能位置** — 九宫格锚点 + 边距偏移，横竖构图短边基准缩放视觉一致
- ⚡ **并行批量导出** — Rust `rayon` 满载 CPU，实时进度事件推送到前端
- 💾 **预设管理** — 多套水印方案（社交/交付/展览）保存、切换、删除
- 🎭 **实时预览** — Canvas 秒级响应参数调整，LRU 缓存 5 张原图，切换瞬时
- 🌑 **深色专业界面** — 摄影工具质感，shadcn 深色主题（OKLCH 色彩空间）

---

## 📸 界面

```
┌─ ⌐ 签名水印 · Watermark Studio ─── [输出目录] [批量导出] ┐
├───────────┬────────────────────────┬───────────────────┤
│ 照片(12) │ 预览      DSCF7261.JPG │ 水印设置          │
│           │                        │                   │
│ ┌───────┐ │      ┌──────────┐     │ 签名图 [sig.png] │
│ │ 拖拽  │ │      │          │     │ 位置 ▫▫▫         │
│ │ 图片  │ │      │  预览图  │     │      ▫▫▫         │
│ └───────┘ │      │          │     │      ▫▫■         │
│  📷 001   │      └──────────┘     │ 大小  ▬▬─ 15%    │
│  📷 002   │                        │ 不透明 ▬▬─ 80%   │
│  📷 003 ▶ │                        │ 边距  ▬── 30px   │
│    ...    │                        │ 颜色 [色板×6]    │
│           │                        │──────────────────│
│           │                        │ 微博发图  另存  │
└───────────┴────────────────────────┴──────────────────┘
```

---

## 🚀 快速开始（普通用户）

### Windows

1. 从 [Releases](https://github.com/Ly-star-bit/watermark_photographer/releases) 下载 `签名水印.exe`（4 MB）
2. 双击运行（无需安装。首次可能被 Windows Defender / 360 询问，选"允许"）
3. 使用流程：
   - 拖拽（或点击选择）照片到左栏
   - 右栏选 PNG 签名图，调位置/大小/透明度/颜色
   - 顶栏「选择输出目录」→ 点「批量导出」
   - 完成后可一键打开输出目录查看

### macOS

暂无 Release，可自行编译（见下方"从源码构建"）。

---

## 🛠️ 从源码构建

### 前置依赖

- [Node.js 20+](https://nodejs.org)
- [Rust](https://rustup.rs) (1.80+)
- **Windows**: Visual Studio Build Tools（勾选 "Desktop development with C++"）
- **macOS**: Xcode Command Line Tools (`xcode-select --install`)

### 开发模式

```bash
git clone https://github.com/Ly-star-bit/watermark_photographer.git
cd watermark_photographer
npm install
npm run tauri dev
```

首次编译约 5-10 分钟（拉取并编译约 395 个 crate），之后增量编译秒级。

### Release 打包

**Windows** — 产出 `src-tauri/target/release/watermark_app.exe`（约 4 MB）:

```bash
npm run tauri build -- --no-bundle
```

**macOS** — 产出 `.app` 和 `.dmg`:

```bash
chmod +x build_mac.sh
./build_mac.sh
```

---

## 🧪 测试

Rust 单元测试：

```bash
cd src-tauri
cargo test --lib
```

覆盖 34 个测试，含：
- 九宫格所有位置的坐标计算
- 横竖构图短边基准适配
- EXIF/ICC 提取与回注（字节级往返）
- **端到端 EXIF/ICC 保留**（源 JPEG → apply → 输出 → 验证元数据字节一致）
- 水印着色（RGB 替换 + alpha 保留）
- 批量并行处理 + 失败隔离
- 预设 JSON 持久化往返

---

## 🏗️ 架构

```
┌──────────────────────── 前端 (React 19 + TS + Tailwind) ─────┐
│  App.tsx  · DropZone · FileList · PreviewCanvas             │
│  WatermarkPanel · PresetManager · BatchProgress             │
└──────────────────────────────┬───────────────────────────────┘
                     Tauri invoke / event
┌──────────────────────────────┴───────────────────────────────┐
│                      Rust 后端                                │
│  commands.rs   → 命令入口 (export_batch / list_presets / …)  │
│  watermark.rs  → 合成核心 (image + Lanczos3 + tint)          │
│  metadata.rs   → EXIF/ICC 保留 (img-parts)                   │
│  position.rs   → 九宫格 + 横竖构图算法                       │
│  batch.rs      → rayon 并行调度 + 进度事件                   │
│  preset.rs     → JSON 持久化                                  │
└───────────────────────────────────────────────────────────────┘
```

**关键设计**：
- 前端 Canvas 预览的定位/缩放算法与 Rust 后端**数学等价**（改一处两边都要改），保证所见即所得
- EXIF/ICC 保留：`img-parts` 按 APP1(0xE1) / APP2(0xE2) 段级搬运，不依赖 image crate 的元数据支持
- 批量任务用 `spawn_blocking` 隔离 rayon CPU 密集操作，不阻塞 Tauri async runtime

---

## 📦 技术栈

| 层 | 技术 |
|---|---|
| 前端 | React 19 · Vite 7 · TypeScript · Tailwind CSS 4 · shadcn/ui · lucide-react |
| 后端 | Rust · `image` · `img-parts` · `rayon` · `serde` · `thiserror` |
| 桌面外壳 | Tauri 2 (WebView2 / WKWebView) |
| 打包 | PyInstaller-free · Tauri CLI `tauri build` |

---

## 📄 许可

MIT

---

## 🙏 致谢

- [Tauri](https://tauri.app) — 轻量跨平台外壳
- [image](https://github.com/image-rs/image) · [img-parts](https://github.com/paolobarbolini/img-parts) — Rust 图像与 JPEG 段处理
- [shadcn/ui](https://ui.shadcn.com) — 深色主题设计系统
- [lucide-react](https://lucide.dev) — 图标
