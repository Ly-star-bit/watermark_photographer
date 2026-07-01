# Watermark Studio · 签名水印

<p align="right">
  <a href="README.md">简体中文</a> · <b>English</b>
</p>

A **batch signature watermark** desktop tool built for photographers. Powered by Tauri 2 + Rust + React 19. Windows single-file exe is **only 4 MB**.

---

## ✨ Features

- 🎨 **Transparent PNG signature overlay** — tint your signature with any color (original / white / off-white / gray / black / custom) so it stays legible on bright backgrounds
- 📷 **Photographer-first: EXIF / ICC preservation** — camera model, aperture, shutter, ISO, timestamps, color profile all preserved byte-for-byte (via `img-parts` JPEG segment handling)
- 🖼️ **Multi-format input** — JPEG · PNG · TIFF · WebP · BMP (output always JPEG)
- 🎯 **Smart positioning** — nine-grid anchor + margin offset, short-side-based scaling for consistent size across landscape/portrait
- ⚡ **Parallel batch export** — Rust `rayon` saturates all CPU cores, real-time progress events streamed to the UI
- 💾 **Preset management** — save/switch/delete multiple watermark schemes (social/delivery/exhibition)
- 🎭 **Live preview** — instant Canvas response to slider changes, LRU cache of 5 decoded images makes switching feel native
- 🌑 **Professional dark UI** — Lightroom-style density with shadcn dark tokens (OKLCH color space)

---

## 📸 Interface

```
┌─ ⌐ Watermark Studio ─────── [Output Dir] [Export All] ┐
├──────────┬──────────────────────┬────────────────────┤
│ Photos12 │ Preview  DSCF7261.JPG│ Watermark Settings │
│          │                      │                    │
│ ┌──────┐ │    ┌──────────┐      │ Signature [sig.png]│
│ │ Drop │ │    │          │      │ Position ▫▫▫       │
│ │ zone │ │    │ Preview  │      │          ▫▫▫       │
│ └──────┘ │    │          │      │          ▫▫■       │
│ 📷 001   │    └──────────┘      │ Size    ▬▬─ 15%    │
│ 📷 002   │                      │ Opacity ▬▬─ 80%    │
│ 📷 003 ▶ │                      │ Margin  ▬── 30px   │
│   ...    │                      │ Color [swatches]   │
│          │                      │────────────────────│
│          │                      │ Preset  Save As    │
└──────────┴──────────────────────┴────────────────────┘
```

---

## 🚀 Quick Start (End Users)

### Windows

1. Download `签名水印.exe` (4 MB) from [Releases](https://github.com/Ly-star-bit/watermark_photographer/releases)
2. Double-click to run — no installation required. Windows Defender may prompt on first run (unsigned build); choose "Allow"
3. Workflow:
   - Drag (or click) photos into the left panel
   - Pick a transparent PNG signature on the right, adjust position/size/opacity/color
   - Click "Output Directory" in the header → hit "Export All"
   - Click "Open Output Folder" when done

### macOS

No release yet — build from source (see below).

---

## 🛠️ Build from Source

### Prerequisites

- [Node.js 20+](https://nodejs.org)
- [Rust](https://rustup.rs) (1.80+)
- **Windows**: Visual Studio Build Tools (check "Desktop development with C++")
- **macOS**: Xcode Command Line Tools (`xcode-select --install`)

### Development

```bash
git clone https://github.com/Ly-star-bit/watermark_photographer.git
cd watermark_photographer
npm install
npm run tauri dev
```

First compile takes ~5-10 minutes (fetching + compiling ~395 crates). Incremental builds are near-instant afterwards.

### Release Build

**Windows** — produces `src-tauri/target/release/watermark_app.exe` (~4 MB):

```bash
npm run tauri build -- --no-bundle
```

**macOS** — produces `.app` and `.dmg`:

```bash
chmod +x build_mac.sh
./build_mac.sh
```

---

## 🧪 Tests

Rust unit tests:

```bash
cd src-tauri
cargo test --lib
```

34 tests covering:
- Coordinate computation for all nine grid positions
- Landscape/portrait short-side-based scaling
- EXIF/ICC extraction and re-injection (byte-level roundtrip)
- **End-to-end EXIF/ICC preservation** (source JPEG → apply → output → verify metadata byte-identical)
- Watermark tinting (RGB replacement + alpha preservation)
- Parallel batch processing + failure isolation
- Preset JSON persistence roundtrip

---

## 🏗️ Architecture

```
┌────────────── Frontend (React 19 + TS + Tailwind) ──────────┐
│  App.tsx  · DropZone · FileList · PreviewCanvas             │
│  WatermarkPanel · PresetManager · BatchProgress             │
└─────────────────────────┬────────────────────────────────────┘
                Tauri invoke / event
┌─────────────────────────┴────────────────────────────────────┐
│                      Rust Backend                             │
│  commands.rs   → command entrypoints                          │
│  watermark.rs  → compositing (image + Lanczos3 + tint)        │
│  metadata.rs   → EXIF/ICC preservation (img-parts)            │
│  position.rs   → nine-grid + orientation-aware algorithm      │
│  batch.rs      → rayon parallel + progress events             │
│  preset.rs     → JSON persistence                             │
└───────────────────────────────────────────────────────────────┘
```

**Key design decisions**:
- Frontend Canvas preview uses the **mathematically equivalent** positioning/scaling algorithm as the Rust backend — change one side, change both. Guarantees WYSIWYG output.
- EXIF/ICC preservation uses `img-parts` to handle APP1 (0xE1) / APP2 (0xE2) JPEG segments directly, bypassing the `image` crate's limited metadata support.
- Batch tasks run inside `spawn_blocking` to isolate rayon's CPU-heavy work from Tauri's async runtime.

---

## 📦 Tech Stack

| Layer | Stack |
|---|---|
| Frontend | React 19 · Vite 7 · TypeScript · Tailwind CSS 4 · shadcn/ui · lucide-react |
| Backend | Rust · `image` · `img-parts` · `rayon` · `serde` · `thiserror` |
| Desktop shell | Tauri 2 (WebView2 / WKWebView) |
| Packaging | Tauri CLI `tauri build` (no PyInstaller / no Electron) |

---

## 📄 License

MIT

---

## 🙏 Acknowledgements

- [Tauri](https://tauri.app) — lightweight cross-platform shell
- [image](https://github.com/image-rs/image) · [img-parts](https://github.com/paolobarbolini/img-parts) — Rust image + JPEG segment handling
- [shadcn/ui](https://ui.shadcn.com) — dark theme design system
- [lucide-react](https://lucide.dev) — icons
