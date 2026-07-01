# Tauri 2 学习笔记 —— 以「签名水印 · Watermark Studio」为例

> 本文档以实际项目 **签名水印** 为案例，逐文件拆解 Tauri 2 的 Rust 后端架构、IPC 通信、权限系统与构建流程。适合已有 Rust 基础但未接触过 Tauri 的开发者。

---

## 目录

1. [Tauri 是什么](#1-tauri-是什么)
2. [项目全景速览](#2-项目全景速览)
3. [Cargo.toml —— Rust 依赖地图](#3-cargotoml--rust-依赖地图)
4. [main.rs —— 两行入口](#4-mainrs--两行入口)
5. [build.rs —— 构建时代码生成](#5-buildrs--构建时代码生成)
6. [lib.rs —— Tauri Builder 核心枢纽](#6-librs--tauri-builder-核心枢纽)
7. [tauri.conf.json —— 应用配置词典](#7-tauriconfjson--应用配置词典)
8. [capabilities —— 权限系统](#8-capabilities--权限系统)
9. [Commands —— 前后端桥梁](#9-commands--前后端桥梁)
10. [Error —— 统一错误处理](#10-error--统一错误处理)
11. [watermark.rs —— 核心合成流水线](#11-watermarkrs--核心合成流水线)
12. [position.rs —— 九宫格定位系统](#12-positionrs--九宫格定位系统)
13. [metadata.rs —— EXIF/ICC 保留](#13-metadatars--exificc-保留)
14. [batch.rs —— Rayon 并行批处理](#14-batchrs--rayon-并行批处理)
15. [preset.rs —— 配置文件持久化](#15-presetrs--配置文件持久化)
16. [前端 API 层 —— TypeScript ↔ Rust](#16-前端-api-层--typescript--rust)
17. [类型同步 —— preview.ts vs position.rs](#17-类型同步--previewts-vs-positionrs)
18. [构建与发布](#18-构建与发布)
19. [知识地图](#19-知识地图)

---

## 1. Tauri 是什么

Tauri 是一个用 **Rust** 写后端、用 **Web 技术（HTML/CSS/JS）** 写前端的跨平台桌面应用框架。核心架构：

```
┌──────────────────────────────────────────┐
│              前端（WebView）               │
│  React / Vue / Svelte / 原生 HTML         │
│  ← 通过 @tauri-apps/api 调用后端 →        │
├──────────────────────────────────────────┤
│            Tauri Core（Rust）              │
│  ┌─────────┐ ┌──────────┐ ┌───────────┐  │
│  │ Commands│ │  Events  │ │  Plugins  │  │
│  │ (IPC)   │ │ (推送)   │ │ (能力扩展) │  │
│  └─────────┘ └──────────┘ └───────────┘  │
├──────────────────────────────────────────┤
│          系统原生 API（Rust）              │
│  文件系统 / 进程 / 系统对话框 / ……        │
└──────────────────────────────────────────┘
```

**与 Electron 的差异：** Electron 捆绑完整 Chromium + Node.js（体积 100MB+），Tauri 复用操作系统内置 WebView（Windows 用 WebView2，macOS 用 WKWebView），体积通常 < 10MB。

---

## 2. 项目全景速览

**签名水印** 是一个给照片批量打 PNG 签名水印的桌面工具。

```
watermark_app/
├── src/                          # ← 前端（React + TypeScript）
│   ├── App.tsx                   #    主布局（三栏）
│   ├── components/               #    UI 组件
│   ├── lib/
│   │   ├── api.ts                #    Tauri IPC 封装（前后端唯一接触点）
│   │   ├── types.ts              #    共享类型定义
│   │   └── preview.ts            #    前端位置计算（必须与 Rust 一致）
│   └── main.tsx                  #    React 入口
│
├── src-tauri/                    # ← 后端（Rust）
│   ├── Cargo.toml                #    Rust 依赖与构建配置
│   ├── tauri.conf.json           #    Tauri 应用配置（窗口、打包、构建）
│   ├── capabilities/
│   │   └── default.json          #    权限清单（文件读写、对话框等）
│   ├── build.rs                  #    构建脚本（tauri-build 生成代码）
│   ├── src/
│   │   ├── main.rs               #    二进制入口
│   │   ├── lib.rs                #    Tauri Builder 组装（注册插件+命令）
│   │   ├── commands.rs           #    IPC 命令处理器（前后端桥梁）
│   │   ├── error.rs              #    统一错误类型
│   │   ├── watermark.rs          #    水印合成核心（图像处理流水线）
│   │   ├── position.rs           #    九宫格定位数学
│   │   ├── metadata.rs           #    EXIF/ICC 提取与回注
│   │   ├── batch.rs              #    Rayon 并行批处理
│   │   └── preset.rs             #    预设 JSON 持久化
│   └── icons/                    #    应用图标
│
├── package.json                  # 前端依赖
├── vite.config.ts                # Vite 构建配置
└── index.html                    # HTML 入口
```

**数据流路径（一次"批量导出"点击的完整链路）：**

```
用户点击"批量导出"
  → frontend: api.ts → invoke("export_batch", args)
    → Rust: commands::export_batch (参数校验 + 预读水印 PNG)
      → Rust: batch::run (rayon par_iter 并行处理所有照片)
        对每张照片:
          → metadata::extract (保存 EXIF/ICC)
          → watermark::apply (解码→缩放水印→着色→调透明度→定位→合成→编码 JPEG)
          → metadata::inject (回注 EXIF/ICC)
          → 写入文件 + 发送进度事件
      ← 返回 BatchSummary
    ← frontend: 显示汇总结果
```

---

## 3. Cargo.toml —— Rust 依赖地图

```toml
[package]
name = "watermark_app"
version = "0.1.0"
description = "批量签名水印工具"
edition = "2021"

[lib]
name = "watermark_app_lib"
crate-type = ["staticlib", "cdylib", "rlib"]
```

### 为什么 crate-type 有三种？

| 类型 | 用途 |
|------|------|
| `staticlib` | 移动端（iOS/Android）编译静态库 |
| `cdylib` | macOS 动态库（.dylib） |
| `rlib` | 桌面端作为 Rust 内部库链接 |

三者并存保证同一代码可编译到桌面（Windows/macOS/Linux）和移动端。

### 依赖逐项解析

```toml
[dependencies]
# ===== Tauri 框架核心 =====
tauri = { version = "2", features = ["protocol-asset"] }
#   - "protocol-asset" 启用自定义协议，前端可通过 asset:// 协议加载本地资源
#   - Tauri 2 相比 Tauri 1：
#     - 插件系统独立（不再内置于 core）
#     - 权限系统基于 capabilities JSON（声明式）
#     - 事件系统使用 Emitter trait
#     - 移动端支持（tauri::mobile_entry_point）

# ===== 官方插件（按需引入）=====
tauri-plugin-opener = "2"
#   用系统默认应用打开文件/路径/URL
#   对应前端的 @tauri-apps/plugin-opener

tauri-plugin-dialog = "2"
#   系统原生文件选择对话框（打开/保存）
#   对应前端的 @tauri-apps/plugin-dialog

tauri-plugin-fs = "2"
#   文件系统读写能力
#   对应前端的 @tauri-apps/plugin-fs

# ===== 序列化 =====
serde = { version = "1", features = ["derive"] }
serde_json = "1"
#   前端 ↔ 后端 IPC 通信的桥梁
#   所有 #[tauri::command] 的参数和返回值
#   都必须可序列化/反序列化

# ===== 图像处理 =====
image = { version = "0.25", default-features = false,
          features = ["jpeg", "png", "tiff", "webp", "bmp"] }
#   default-features = false → 不引入任何默认编解码器
#   按 feature 精确选择所需的格式 → 减小编译产物体积

# ===== JPEG 段级操作 =====
img-parts = "0.3"
#   JPEG 内部 APP1(EXIF)/APP2(ICC) 段读写
#   image crate 重新编码时会丢弃这些段→需要手动处理

# ===== 并行处理 =====
rayon = "1"
#   par_iter() 利用全部 CPU 核心并行处理照片

# ===== 错误处理 =====
thiserror = "2"
#   用 #[derive(Error)] 宏自动生成 std::error::Error impl

[build-dependencies]
tauri-build = { version = "2", features = [] }
#   构建时生成 Tauri 相关代码（capability schema 等）

[dev-dependencies]
tempfile = "3"
#   测试时创建临时目录/文件

# 发布构建优化
[profile.release]
opt-level = "z"       # 优化代码体积（而非速度）
lto = true            # 链接时优化（跨 crate 内联/死代码消除）
codegen-units = 1     # 单代码生成单元（更好的内联）
panic = "abort"       # panic 时直接终止（不展开栈，减体积）
strip = true          # 剥离符号表
```

**关键认知：** Tauri 的插件和前端 npm 包是一一对应的。`tauri-plugin-dialog` 在 Cargo.toml 中是 `tauri-plugin-dialog = "2"`，前端对应 `@tauri-apps/plugin-dialog`。

---

## 4. main.rs —— 两行入口

```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    watermark_app_lib::run()
}
```

**逐行解释：**

- **`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`**
  - `cfg_attr(条件, 属性)` 是条件编译
  - 条件：`not(debug_assertions)` → 仅在 release 模式
  - 属性：`windows_subsystem = "windows"` → 让 Windows 不启动控制台窗口
  - 效果：Release 版直接显示 GUI 窗口，不伴随黑框
  - Debug 时仍显示控制台，方便 `println!` 和 `eprintln!` 调试

- **`watermark_app_lib::run()`** 调用 lib.rs 的 `run()` 函数
  - Tauri 项目惯例：逻辑放 lib.rs，main.rs 只负责入口

---

## 5. build.rs —— 构建时代码生成

```rust
fn main() {
    tauri_build::build()
}
```

`tauri_build::build()` 在编译前执行：

1. 扫描 `capabilities/*.json`，生成 JSON schema 到 `gen/schemas/`（用于 IDE 自动补全和校验）
2. 生成 icons 相关代码
3. 生成 `tauri::generate_context!()` 宏所需的环境信息

---

## 6. lib.rs —— Tauri Builder 核心枢纽

```rust
mod commands;
mod error;
mod watermark;
mod position;
mod metadata;
mod batch;
mod preset;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::export_batch,
            commands::list_presets,
            commands::save_preset,
            commands::delete_preset,
            commands::create_thumbnail
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

这叫 **Builder 模式** —— Tauri 所有配置通过链式调用组装。

### 6.1 `#[cfg_attr(mobile, tauri::mobile_entry_point)]`

`tauri::mobile_entry_point` 宏在移动端（iOS/Android）生成所需的 JNI / FFI 入口。桌面端忽略此属性。

### 6.2 `tauri::Builder::default()`

创建一个新的 Tauri 应用构建器。所有配置从此开始。

### 6.3 `.plugin(...)` —— 注册插件

```rust
.plugin(tauri_plugin_opener::init())
.plugin(tauri_plugin_dialog::init())
.plugin(tauri_plugin_fs::init())
```

每个插件是一个独立的能力模块，注册后前端才能使用对应功能。插件通过 `init()` 返回初始化器。

**必须注册对应 npm 包才能从前端调用：**
- `tauri-plugin-dialog` → `npm install @tauri-apps/plugin-dialog`
- 两者缺一不可

### 6.4 `.invoke_handler(...)` —— 注册 IPC 命令

```rust
.invoke_handler(tauri::generate_handler![
    commands::ping,
    commands::export_batch,
    commands::list_presets,
    commands::save_preset,
    commands::delete_preset,
    commands::create_thumbnail
])
```

`tauri::generate_handler![]` 是一个宏，批量注册 `#[tauri::command]` 函数为可被前端调用的 IPC 端点。

**注册后的效果：**
```typescript
// 前端可以直接调用
import { invoke } from "@tauri-apps/api/core";
const result = await invoke("export_batch", { inputPaths: [...], ... });
const presets = await invoke("list_presets");
```

### 6.5 `.run(tauri::generate_context!())`

`generate_context!()` 宏编译时读取 `tauri.conf.json`，生成上下文（窗口尺寸、应用名、图标路径等）。`run()` 启动事件循环并打开窗口。

### 模块声明 `mod commands;`

```rust
mod commands;    // 声明子模块（Rust 在 commands.rs 中查找）
mod error;       // 同理
mod watermark;
// ...
```

Rust 的模块系统：`mod X;` 声明 X 为当前 crate 的子模块。默认私有（外部 crate 无法访问），需要 `pub mod` 才可公开。

---

## 7. tauri.conf.json —— 应用配置词典

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "签名水印",
  "version": "0.1.0",
  "identifier": "com.photog.watermark",
  "build": {
    "beforeDevCommand": "npm run dev",   // dev 前先启动前端 Vite
    "devUrl": "http://localhost:1420",   // dev 模式加载此 URL
    "beforeBuildCommand": "npm run build",// build 前先编译前端
    "frontendDist": "../dist"            // 打包时内嵌的静态文件目录
  },
  "app": {
    "windows": [
      {
        "title": "签名水印 · Watermark Studio",
        "width": 1280,
        "height": 800,
        "minWidth": 1024,
        "minHeight": 640,
        "resizable": true,
        "fullscreen": false,
        "dragDropEnabled": true          // 启用系统拖放（文件拖入窗口）
      }
    ],
    "security": {
      "csp": null,                       // 禁用 CSP（开发灵活性优先）
      "assetProtocol": {
        "enable": true,                  // 启用 asset:// 自定义协议
        "scope": ["**"]                  // 允许访问所有路径
      }
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",                    // 所有格式（msi/nsis/dmg/deb/appimage）
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",                 // macOS
      "icons/icon.ico"                   // Windows
    ]
  }
}
```

**关键节点说明：**

| 字段 | 含义 |
|------|------|
| `identifier` | 应用唯一 ID，反向域名格式，跨平台唯一 |
| `build.devUrl` | 开发时 WebView 加载的 URL（本地 Vite dev server） |
| `build.frontendDist` | 生产构建时内嵌的静态文件路径 |
| `app.windows[].dragDropEnabled` | 启用文件拖放到窗口（本项目核心交互） |
| `security.assetProtocol` | 启用 `asset://` 协议加载本地资源 |

---

## 8. capabilities —— 权限系统

Tauri 2 的权限基于 JSON 声明文件（`capabilities/*.json`），替代 Tauri 1 的 allowlist。

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],       // 哪些窗口拥有此权限
  "permissions": [
    "core:default",          // 核心基础权限
    "core:webview:allow-internal-toggle-devtools",  // 允许 DevTools

    "opener:default",        // opener 插件基础权限
    {                        // 允许打开任意路径
      "identifier": "opener:allow-open-path",
      "allow": [{ "path": "**" }]
    },

    "dialog:default",
    "dialog:allow-open",     // 允许弹出"打开文件"对话框
    "dialog:allow-save",     // 允许弹出"保存文件"对话框

    "fs:default",
    "fs:allow-read-file",    // 允许读文件
    "fs:allow-read-text-file",
    {
      "identifier": "fs:scope",
      "allow": [{ "path": "**" }]  // 允许访问所有路径
    }
  ]
}
```

### 权限粒度模型

```
插件级权限（如 fs:default）
  └── 能力级权限（如 fs:allow-read-file）
       └── 范围级权限（如 { "path": "**" } 允许所有路径）
```

每一项 "allow" 权限都需要显式声明。如果没有 `dialog:allow-open`，前端调用文件对话框会收到权限拒绝错误。

### 本项目为什么授权范围宽？

摄影工具需要用户自由选择任意路径的照片和签名图，因此所有路径都设为 `**`。对于对安全性有要求的应用，应该缩小 scope。

---

## 9. Commands —— 前后端桥梁

`commands.rs` 是整个应用的前后端通信中枢。6 个 `#[tauri::command]` 函数各司其职。

### 9.1 基础形态 —— ping

```rust
#[tauri::command]
pub fn ping() -> String {
    "pong from Rust".to_string()
}
```

**最简单的命令：** 无参数、同步返回、返回值实现 Serialize 即可。前端调用：
```typescript
const response = await invoke("ping");
// response → "pong from Rust"
```

### 9.2 带参数 + 异步 —— export_batch

```rust
#[derive(Debug, Deserialize)]
pub struct ExportBatchArgs {
    pub input_paths: Vec<String>,
    pub output_dir: String,
    pub watermark_path: String,
    pub config: WatermarkConfig,
}

#[tauri::command]
pub async fn export_batch(app: AppHandle, args: ExportBatchArgs) -> Result<BatchSummary> {
    // 1. 参数校验（输入非空、配置值合法）
    if args.input_paths.is_empty() {
        return Err(WatermarkError::InvalidParam("输入照片列表为空".to_string()));
    }
    args.config.validate()?;

    // 2. 预读水印 PNG 文件（一次 IO，所有 worker 共享）
    let wm_bytes = std::fs::read(&args.watermark_path)?;

    // 3. 构造批处理任务
    let task = BatchInput { input_paths, output_dir, watermark_bytes: wm_bytes, config: args.config };

    // 4. 在阻塞线程池上执行（避免占用 async runtime 的线程）
    let results = tauri::async_runtime::spawn_blocking(move || {
        batch::run(&task, move |done, total, name, ok| {
            let _ = app_handle.emit("batch-progress", BatchProgress { done, total, filename: name.to_string(), ok });
        })
    }).await.map_err(|e| WatermarkError::InvalidParam(format!("批量任务执行失败: {e}")))?;

    // 5. 统计汇总
    let success = results.iter().filter(|r| r.error.is_none()).count();
    let failed = results.len() - success;
    Ok(BatchSummary { total: results.len(), success, failed, items: results })
}
```

**关键知识点：**

1. **`AppHandle` 参数：** Tauri 会自动注入。用于获取应用路径、发送事件。

2. **`args: ExportBatchArgs`：** `#[derive(Deserialize)]` 的结构体，Tauri 自动将前端传来的 JSON 反序列化为 Rust 结构体。

3. **`async fn`：** 命令可以是异步的。Tauri 的 async runtime 基于 tokio（桌面端）。

4. **`spawn_blocking`：** 图像处理是 CPU 密集型操作，必须移到阻塞线程池。永远不要在 async 线程中执行 CPU 密集型任务，否则会阻塞所有其他 async 任务。

5. **事件推送 `app.emit(...)`：**
   ```rust
   app_handle.emit("batch-progress", BatchProgress { ... })
   ```
   前端通过监听事件获取实时进度：
   ```typescript
   import { listen } from "@tauri-apps/api/event";
   const unlisten = await listen("batch-progress", (event) => {
     console.log(event.payload); // { done: 3, total: 10, filename: "...", ok: true }
   });
   ```

### 9.3 预设 CRUD —— 同步命令

```rust
#[tauri::command]
pub fn list_presets(app: AppHandle) -> Result<Vec<Preset>> {
    preset::load_all(&config_dir(&app)?)
}

#[tauri::command]
pub fn save_preset(app: AppHandle, preset: Preset) -> Result<Vec<Preset>> {
    if preset.name.trim().is_empty() {
        return Err(WatermarkError::InvalidParam("预设名称不能为空".to_string()));
    }
    preset.config.validate()?;
    preset::upsert(&config_dir(&app)?, preset)
}

#[tauri::command]
pub fn delete_preset(app: AppHandle, name: String) -> Result<Vec<Preset>> {
    preset::delete(&config_dir(&app)?, &name)
}
```

**为什么是同步？** 预设 JSON 文件大小 < 10KB，读写是几乎瞬时的磁盘操作，不需要 async。

注意参数：`AppHandle` 仍然是 Tauri 自动注入的，`preset: Preset` 和 `name: String` 是前端传来的数据。

### 9.4 缩略图生成 —— spawn_blocking 模式

```rust
#[tauri::command]
pub async fn create_thumbnail(path: String, max_size: u32) -> Result<Vec<u8>> {
    tauri::async_runtime::spawn_blocking(move || create_thumbnail_impl(&path, max_size))
        .await
        .map_err(|e| WatermarkError::InvalidParam(format!("缩略图任务失败: {e}")))?
}
```

**设计理由：** 24MP 的富士 JPEG 文件可能在 20MB+，解码再缩放到 40x40px 需要图像库在阻塞线程执行。

**返回值是 `Vec<u8>`：** Tauri 自动将 `Vec<u8>` 序列化为 `number[]` 传给前端。前端可将其转为 Blob URL 用于 `<img>` 标签：
```typescript
const bytes = await invoke("create_thumbnail", { path, maxSize: 40 });
const blob = new Blob([new Uint8Array(bytes)], { type: "image/jpeg" });
const url = URL.createObjectURL(blob);
setThumbnailUrl(url);
```

---

## 10. Error —— 统一错误处理

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WatermarkError {
    #[error("图像读写错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("图像解码/编码错误：{0}")]
    Image(#[from] image::ImageError),

    #[error("JPEG 段解析错误：{0}")]
    JpegParts(#[from] img_parts::Error),

    #[error("JSON 序列化错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("参数非法：{0}")]
    InvalidParam(String),

    #[error("水印尺寸大于底图：底图 {img_w}x{img_h}，水印 {wm_w}x{wm_h}")]
    WatermarkTooLarge { img_w: u32, img_h: u32, wm_w: u32, wm_h: u32 },
}

pub type Result<T> = std::result::Result<T, WatermarkError>;

// Tauri 要求错误可序列化
impl serde::Serialize for WatermarkError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
```

**关键设计决策：**

1. **`#[from]` 自动转换：** `?` 操作符自动将 `std::io::Error`、`image::ImageError` 等转为 `WatermarkError`。
   ```rust
   let bytes = std::fs::read(&path)?; // std::io::Error → WatermarkError::Io
   let img = image::open(path)?;       // image::ImageError → WatermarkError::Image
   ```

2. **自定义变体：** `InvalidParam(String)` 和 `WatermarkTooLarge { ... }` 携带上下文，前端可以得到有意义的错误信息。

3. **Serialize 实现：** Tauri 要求 IPC 返回值实现 Serialize。错误以字符串形式传给前端。前端 catch 到的是 JSON 字符串格式的错误。

4. **类型别名：** `pub type Result<T>` 让所有函数返回值简洁统一。

---

## 11. watermark.rs —— 核心合成流水线

这是整个项目的核心。一个 147 行的函数组完成从"输入照片 + 水印 PNG"到"输出带签名 JPEG"的全过程。

### 11.1 主入口 `apply()`

```rust
pub fn apply(
    src_jpeg: &[u8],        // 源照片的完整字节
    watermark_png: &[u8],   // 水印 PNG 的完整字节（已预读一次）
    config: &WatermarkConfig,
) -> Result<Vec<u8>> {      // 返回合成后 JPEG 的完整字节
    config.validate()?;
```

**为什么参数是字节切片而非文件路径？** 将 IO 与处理逻辑分离。`commands.rs` 负责磁盘读，`watermark.rs` 只关心内存中的图像数据。这样测试可以直接用内存数据而不需要临时文件。

```rust
    // 步骤1：提取源 JPEG 的 EXIF/ICC（降级容错）
    let meta = metadata::extract(src_jpeg).unwrap_or_else(|_| Metadata::empty());

    // 步骤2：解码底图
    let base = decode_image(src_jpeg)?;
    let (img_w, img_h) = base.dimensions();

    // 步骤3-4：解码水印 + 按配置缩放到目标宽度
    let watermark = prepare_watermark(watermark_png, img_w, img_h, config)?;
    let (wm_w, wm_h) = watermark.dimensions();
```

**瀑布式处理：** 每一步的输出是下一步的输入，错误通过 `?` 立即向上传播。

```rust
    // 步骤5a：可选着色（将白色签名变红/蓝/黑等）
    let watermark = match config.tint {
        Some(rgb) => apply_tint(watermark, rgb),
        None => watermark,
    };

    // 步骤5b：调整不透明度
    let watermark = apply_opacity(watermark, config.opacity);

    // 步骤6：计算九宫格位置
    let (x, y) = position::compute_position(img_w, img_h, wm_w, wm_h, config)?;

    // 步骤7：Alpha 合成
    let mut canvas = base.to_rgba8();
    image::imageops::overlay(&mut canvas, &watermark, x, y);
    let composed: RgbImage = DynamicImage::ImageRgba8(canvas).to_rgb8();

    // 步骤8：编码为 JPEG（quality=95）
    let encoded = encode_jpeg(&composed)?;

    // 步骤9：回注 EXIF/ICC 元数据
    metadata::inject(encoded, &meta)
}
```

### 11.2 图像解码

```rust
fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()  // 自动嗅探格式（JPEG/PNG/TIFF/WebP/BMP）
        .map_err(image::ImageError::IoError)?;
    Ok(reader.decode()?)
}
```

`with_guessed_format()` 通过文件头 magic bytes 自动判断格式。`Cursor` 将 `&[u8]` 包装为 `Read + Seek` trait 对象。

### 11.3 水印缩放

```rust
fn prepare_watermark(
    png_bytes: &[u8],
    img_w: u32, img_h: u32,
    config: &WatermarkConfig,
) -> Result<RgbaImage> {
    let raw = decode_image(png_bytes)?.to_rgba8();
    let target_w = position::target_watermark_width(img_w, img_h, config.size_ratio);

    let (ow, oh) = raw.dimensions();
    let target_h = ((oh as f32) * (target_w as f32) / (ow as f32)).round() as u32;
    let target_h = target_h.max(1);

    // Lanczos3：最高质量的重采样算法（保留细节）
    let scaled = image::imageops::resize(&raw, target_w, target_h, FilterType::Lanczos3);
    Ok(scaled)
}
```

**Lanczos3 vs 其他滤波器：**
- `Nearest`: 最快，有锯齿
- `Triangle`: 较快，轻微模糊
- `Lanczos3`: 最慢，但最清晰（摄影工具的签名必须高保真）

### 11.4 着色 `apply_tint`

```rust
fn apply_tint(mut wm: RgbaImage, rgb: [u8; 3]) -> RgbaImage {
    for pixel in wm.pixels_mut() {
        if pixel[3] > 0 {      // alpha > 0 → 不透明像素
            pixel[0] = rgb[0]; // R
            pixel[1] = rgb[1]; // G
            pixel[2] = rgb[2]; // B
            // alpha 不变 → 保留抗锯齿边缘
        }
    }
    wm
}
```

**为什么跳过 alpha=0 的像素？** 签名 PNG 通常有透明背景。如果不检查 alpha，透明区域也会被着色，导致签名周围出现彩色方块。

### 11.5 不透明度 `apply_opacity`

```rust
fn apply_opacity(mut wm: RgbaImage, opacity: f32) -> RgbaImage {
    if (opacity - 1.0).abs() < f32::EPSILON {
        return wm;  // 快捷路径：1.0 不需要修改
    }
    let factor = opacity.clamp(0.0, 1.0);
    for pixel in wm.pixels_mut() {
        let a = pixel[3] as f32 * factor;
        pixel[3] = a.round().clamp(0.0, 255.0) as u8;
    }
    wm
}
```

**快捷路径优化：** `opacity == 1.0` 时跳过全图遍历（常见场景：用户使用默认值）。

### 11.6 JPEG 编码

```rust
const JPEG_QUALITY: u8 = 95;

fn encode_jpeg(img: &RgbImage) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(img.as_raw().len() / 4);
    let encoder = JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    encoder.write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)?;
    Ok(buf)
}
```

**Q=95 的理由：** 摄影师后期链路通常 90+ 保画质；95 在体积和视觉质量间的平衡被广泛认可。

**`with_capacity` 预估：** RGB 每像素 3 字节，JPEG 通常压缩到 1/4 以下，预分配减少 realloc。

---

## 12. position.rs —— 九宫格定位系统

### 12.1 数据结构

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridPosition {
    TopLeft, TopCenter, TopRight,
    MiddleLeft, Center, MiddleRight,
    BottomLeft, BottomCenter, BottomRight,
}
```

`#[serde(rename_all = "snake_case")]` 让 Rust 的 PascalCase 枚举变体在 JSON 中自动变成 snake_case。前端看到的是 `"bottom_right"` 而非 `"BottomRight"`。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkConfig {
    pub position: GridPosition,
    pub size_ratio: f32,       // 0.01-1.0
    pub opacity: f32,          // 0.0-1.0
    pub margin_x: u32,         // 像素
    pub margin_y: u32,         // 像素
    #[serde(default)]
    pub landscape_override: Option<GridPosition>,  // 可选：横构图使用不同锚点
    #[serde(default)]
    pub tint: Option<[u8; 3]>,  // 可选：[R, G, B] 着色
}
```

`#[serde(default)]` 让序列化时 None 值被省略（而非写入 `null`），减小 JSON 体积。

### 12.2 尺寸基准 —— 短边逻辑

```rust
#[inline]
pub fn is_landscape(width: u32, height: u32) -> bool {
    width >= height
}

#[inline]
pub fn scale_base(width: u32, height: u32) -> u32 {
    width.min(height)  // 取短边
}

pub fn target_watermark_width(img_w: u32, img_h: u32, size_ratio: f32) -> u32 {
    let base = scale_base(img_w, img_h) as f32;
    ((base * size_ratio).round() as u32).max(1)
}
```

**为什么用短边？** 一张 6000x4000 的横图和一张 4000x6000 的竖图，用短边做基准能保证水印在两图中的视觉大小一致。如果按长边计算，横图水印会太大。

### 12.3 位置计算

```rust
pub fn compute_position(
    img_w: u32, img_h: u32, wm_w: u32, wm_h: u32, config: &WatermarkConfig,
) -> crate::error::Result<(i64, i64)> {
    // 尺寸检查：水印不能比底图大
    if wm_w > img_w || wm_h > img_h {
        return Err(crate::error::WatermarkError::WatermarkTooLarge { ... });
    }

    // 横构图可选覆盖锚点
    let anchor = if is_landscape(img_w, img_h) {
        config.landscape_override.unwrap_or(config.position)
    } else {
        config.position
    };

    // 九宫格公式
    let mx = config.margin_x as i64;
    let my = config.margin_y as i64;
    let iw = img_w as i64; let ih = img_h as i64;
    let ww = wm_w as i64; let wh = wm_h as i64;

    let (x, y) = match anchor {
        GridPosition::TopLeft      => (mx,              my),
        GridPosition::TopCenter    => ((iw - ww) / 2,   my),
        GridPosition::TopRight     => (iw - ww - mx,    my),
        GridPosition::MiddleLeft   => (mx,              (ih - wh) / 2),
        GridPosition::Center       => ((iw - ww) / 2,   (ih - wh) / 2),
        GridPosition::MiddleRight  => (iw - ww - mx,    (ih - wh) / 2),
        GridPosition::BottomLeft   => (mx,              ih - wh - my),
        GridPosition::BottomCenter => ((iw - ww) / 2,   ih - wh - my),
        GridPosition::BottomRight  => (iw - ww - mx,    ih - wh - my),
    };

    // 防越界 clamp
    let x = x.clamp(0, iw - ww);
    let y = y.clamp(0, ih - wh);
    Ok((x, y))
}
```

**九宫格公式可视化：**
```
┌──────────────┬──────────────┬──────────────┐
│  TopLeft     │  TopCenter   │  TopRight    │
│  (mx, my)    │  (half, my)  │  (w-mx, my)  │
├──────────────┼──────────────┼──────────────┤
│  MiddleLeft  │  Center      │  MiddleRight │
│  (mx, half)  │  (half,half) │  (w-mx,half) │
├──────────────┼──────────────┼──────────────┤
│  BottomLeft  │  BottomCenter│  BottomRight │
│  (mx,h-my)   │  (half,h-my) │  (w-mx,h-my) │
└──────────────┴──────────────┴──────────────┘
```

---

## 13. metadata.rs —— EXIF/ICC 保留

这是摄影师最关心的功能。`image` crate 重新编码 JPEG 时会丢弃所有的 APP 段（EXIF、ICC、XMP 等）。

### 13.1 问题演示

```
源 JPEG = [SOI][APP1(EXIF)][APP2(ICC)][DQT][SOF][SOS][图像数据][EOI]
         ↓ image crate 解码 + 重新编码
输出 JPEG = [SOI][DQT][SOF][SOS][新图像数据][EOI]
         ↓ EXIF 和 ICC 都丢了！
```

### 13.2 解决方案 —— img-parts

```rust
use img_parts::jpeg::Jpeg;
use img_parts::{Bytes, ImageEXIF, ImageICC};

#[derive(Debug, Default, Clone)]
pub struct Metadata {
    pub exif: Option<Bytes>,  // APP1 段原始字节
    pub icc: Option<Bytes>,   // APP2 段原始字节
}

/// 提取
pub fn extract(src_bytes: &[u8]) -> Result<Metadata> {
    let jpeg = Jpeg::from_bytes(Bytes::copy_from_slice(src_bytes))?;
    Ok(Metadata {
        exif: jpeg.exif(),          // ImageEXIF trait 方法
        icc: jpeg.icc_profile(),    // ImageICC trait 方法
    })
}

/// 回注
pub fn inject(encoded_jpeg: Vec<u8>, meta: &Metadata) -> Result<Vec<u8>> {
    if !meta.has_any() {
        return Ok(encoded_jpeg);  // 没有元数据可注入，原样返回
    }

    let mut jpeg = Jpeg::from_bytes(Bytes::from(encoded_jpeg))?;
    if let Some(ref exif) = meta.exif {
        jpeg.set_exif(Some(exif.clone()));
    }
    if let Some(ref icc) = meta.icc {
        jpeg.set_icc_profile(Some(icc.clone()));
    }
    let mut out = Vec::new();
    jpeg.encoder().write_to(&mut out)?;
    Ok(out)
}
```

**关键：** `extract` 在图像处理之前执行（先保存元数据），`inject` 在图像处理之后执行（回注元数据）。降级策略：源 JPEG 可能没有 EXIF/ICC（例如手机截图），`extract` 失败时用 `unwrap_or_else(|_| Metadata::empty())` 降级处理。

---

## 14. batch.rs —— Rayon 并行批处理

### 14.1 核心设计

```rust
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct BatchInput {
    pub input_paths: Vec<PathBuf>,    // 所有要处理的文件
    pub output_dir: PathBuf,
    pub watermark_bytes: Vec<u8>,     // 水印 PNG 已预读，所有线程共享引用
    pub config: WatermarkConfig,
}

pub fn run<F>(task: &BatchInput, on_progress: F) -> Vec<ItemResult>
where
    F: Fn(usize, usize, &str, bool) + Sync + Send,
{
    std::fs::create_dir_all(&task.output_dir).ok();  // 先创建输出目录
    let total = task.input_paths.len();
    let counter = AtomicUsize::new(0);

    task.input_paths
        .par_iter()                    // ← Rayon：自动拆分为多个线程
        .map(|src| {
            let result = process_one(src, &task.output_dir, &task.watermark_bytes, &task.config);
            let done = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            (on_progress)(done, total, name, result.error.is_none());
            result
        })
        .collect()                     // ← 收集回 Vec<ItemResult>
}
```

### 14.2 关键知识点

| 要点 | 实现方式 |
|------|---------|
| **并行迭代** | `par_iter().map()` 替换 `iter().map()` |
| **水印数据共享** | `&task.watermark_bytes` 不可变引用，多个线程同时读 |
| **线程安全计数** | `AtomicUsize` 的 `fetch_add()` 保证原子性 |
| **进度回调** | 泛型闭包 `F: Fn(...) + Sync + Send`，每处理完一个文件调用一次 |
| **失败隔离** | `process_one` 的 `catch` 确保一张失败不影响其他文件 |

### 14.3 为什么不在 async runtime 中跑？

```rust
// commands.rs 中的调用
let results = tauri::async_runtime::spawn_blocking(move || {
    batch::run(&task, move |...| { ... })
}).await?;
```

Rayon 的 `par_iter()` 会阻塞当前线程直到所有任务完成。如果在 tokio 的 async worker 线程上阻塞，会导致整个 async runtime 停止工作。`spawn_blocking` 将任务移到专门的阻塞线程池。

**线程模型：**
```
tokio async runtime (少量线程)
    ├── 处理 IPC 请求、事件分发
    └── 遇到 CPU 密集任务时 → spawn_blocking
         └── 阻塞线程池
              └── rayon 全局线程池 (所有 CPU 核心)
```

### 14.4 单张照片处理

```rust
fn do_one(src: &Path, out_dir: &Path, wm: &[u8], config: &WatermarkConfig) -> Result<PathBuf> {
    let src_bytes = std::fs::read(src)?;              // 1. 读文件
    let out_bytes = watermark::apply(&src_bytes, wm, config)?;  // 2. 合成
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let out_name = format!("{stem}_wm.jpg");           // 3. 构造输出名
    let out_path = out_dir.join(out_name);
    std::fs::write(&out_path, out_bytes)?;             // 4. 写文件
    Ok(out_path)
}
```

---

## 15. preset.rs —— 配置文件持久化

### 15.1 存储位置

```rust
fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path().app_config_dir()
        .map_err(|e| WatermarkError::InvalidParam(format!("无法定位配置目录: {e}")))
}
```

`app.path().app_config_dir()` 返回各平台的配置目录：
- Windows: `C:\Users\<用户名>\AppData\Roaming\com.photog.watermark\`
- macOS: `~/Library/Application Support/com.photog.watermark/`
- Linux: `~/.config/com.photog.watermark/`

### 15.2 CRUD 实现

```rust
const PRESETS_FILE: &str = "presets.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub config: WatermarkConfig,
    #[serde(default)]
    pub watermark_path: Option<String>,  // 可选：签名图路径
}

// 读取：文件不存在或为空 → 返回 []
pub fn load_all(config_dir: &Path) -> Result<Vec<Preset>> {
    let path = config_dir.join(PRESETS_FILE);
    if !path.exists() { return Ok(Vec::new()); }
    let s = std::fs::read_to_string(&path)?;
    if s.trim().is_empty() { return Ok(Vec::new()); }
    Ok(serde_json::from_str(&s)?)
}

// 写入：自动创建目录 + 格式化 JSON
fn save_all(config_dir: &Path, presets: &[Preset]) -> Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = config_dir.join(PRESETS_FILE);
    let json = serde_json::to_string_pretty(presets)?;
    std::fs::write(path, json)?;
    Ok(())
}

// 插入或更新：同名覆盖，否则追加
pub fn upsert(config_dir: &Path, preset: Preset) -> Result<Vec<Preset>> {
    let mut all = load_all(config_dir)?;
    match all.iter_mut().find(|p| p.name == preset.name) {
        Some(existing) => *existing = preset,  // 覆盖
        None => all.push(preset),              // 新增
    }
    save_all(config_dir, &all)?;
    Ok(all)
}

// 删除：retain 过滤掉 name 匹配的项
pub fn delete(config_dir: &Path, name: &str) -> Result<Vec<Preset>> {
    let mut all = load_all(config_dir)?;
    all.retain(|p| p.name != name);
    save_all(config_dir, &all)?;
    Ok(all)
}
```

**设计模式：每次操作返回完整列表。** 这样前端不需要维护本地状态，直接显示返回的列表即可。

---

## 16. 前端 API 层 —— TypeScript ↔ Rust

`src/lib/api.ts` 是前后端的唯一接触点，封装所有 IPC 调用。

### 16.1 命令调用

```typescript
import { invoke } from "@tauri-apps/api/core";

// 同步命令
const presets = await invoke<Preset[]>("list_presets");
const result = await invoke<Preset[]>("save_preset", { preset: newPreset });

// 异步命令（带参数对象）
const summary = await invoke<BatchSummary>("export_batch", {
  inputPaths: selectedFiles.map(f => f.path),
  outputDir: outputPath,
  watermarkPath: watermarkFile.path,
  config: currentConfig,
});
```

`invoke<T>("命令名", 参数对象)` 的泛型参数 `T` 是返回值类型。Tauri 自动将 Rust 返回的 JSON 反序列化为该类型。

### 16.2 事件监听

```typescript
import { listen } from "@tauri-apps/api/event";

export function onBatchProgress(cb: (p: BatchProgress) => void) {
  return listen<BatchProgress>("batch-progress", (event) => {
    cb(event.payload);
  });
}
```

`listen` 返回 `Promise<UnlistenFn>`，调用返回的函数即可取消监听。

### 16.3 系统对话框

```typescript
import { open } from "@tauri-apps/plugin-dialog";

export async function pickImageFiles(): Promise<string[] | null> {
  const files = await open({
    multiple: true,
    filters: [{ name: "图片", extensions: ["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp"] }],
  });
  return files; // string[] | null
}
```

### 16.4 文件拖放

```typescript
import { getCurrentWebview } from "@tauri-apps/api/webview";

export async function onImageDrop(cb: (paths: string[]) => void) {
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      const paths = event.payload.paths.filter(isSupportedImagePath);
      cb(paths);
    }
  });
}
```

Tauri 2 的拖放事件通过 WebView 的 `onDragDropEvent` 监听，替代了 Tauri 1 的 `listen("tauri://drag-drop", ...)`。

---

## 17. 类型同步 —— preview.ts vs position.rs

前端 Canvas 预览必须和后端图像处理使用完全相同的数学公式，否则预览和实际输出会不一致。

### 17.1 Rust 端（position.rs）

```rust
pub fn is_landscape(width: u32, height: u32) -> bool {
    width >= height
}

pub fn scale_base(width: u32, height: u32) -> u32 {
    width.min(height)
}

pub fn target_watermark_width(img_w: u32, img_h: u32, size_ratio: f32) -> u32 {
    let base = scale_base(img_w, img_h) as f32;
    ((base * size_ratio).round() as u32).max(1)
}
```

### 17.2 TypeScript 端（preview.ts）

```typescript
export function isLandscape(width: number, height: number): boolean {
  return width >= height;
}

export function scaleBase(width: number, height: number): number {
  return Math.min(width, height);
}

export function targetWatermarkWidth(
  imgW: number, imgH: number, sizeRatio: number,
): number {
  const base = scaleBase(imgW, imgH);
  return Math.max(1, Math.round(base * sizeRatio));
}
```

**逐行对应，数学完全一致。** `width.min(height)` ↔ `Math.min(width, height)`，`(base * ratio).round() as u32` ↔ `Math.round(base * ratio)`。

### 17.3 maintainability 建议

对这种需要前端后端双端同步的代码：
1. 两边都写单元测试，覆盖相同的 case
2. 在注释中标注"必须与 XXX 保持同步"
3. 如果有更复杂的算法，考虑用 WASM 共享同一份 Rust 代码到前端

---

## 18. 构建与发布

### 18.1 开发环境

```bash
# 安装 Rust + Tauri CLI
cargo install tauri-cli --version "^2"

# 进入项目
cd watermark_app

# 启动开发模式（同时启动 Vite dev server + Tauri 窗口）
cargo tauri dev
```

### 18.2 生产构建

```bash
# 构建当前平台的安装包
cargo tauri build
```

产物位于 `src-tauri/target/release/bundle/`：
- Windows: `.msi` 安装包 + `.exe` 便携版
- macOS: `.dmg` 磁盘映像 + `.app` 文件夹
- Linux: `.deb` + `.AppImage`

### 18.3 Release profile 优化解读

```toml
[profile.release]
opt-level = "z"       # 优化体积（s=speed, z=size, 0-3）
lto = true            # 链接时优化，跨 crate 消除死代码
codegen-units = 1     # 单个 CGU，最大化内联机会
panic = "abort"       # panic 直接终止，不打栈回溯字符串
strip = true          # 剥离符号表
```

这些设置使最终二进制体积显著减小。本项目 Windows .exe 约 5-6MB。

---

## 19. 知识地图

下面是从本项目可以学到的 Tauri 核心概念及其对应文件：

```
Tauri 概念              →  项目中位置               →  做了什么
══════════════════════════════════════════════════════════════════════
tauri::Builder          →  lib.rs:14                →  应用组装入口
.plugin()               →  lib.rs:15-17              →  注册官方插件
.invoke_handler()       →  lib.rs:18-25              →  注册 IPC 命令
tauri.conf.json         →  tauri.conf.json           →  窗口/构建/打包配置
capabilities/*.json     →  capabilities/default.json →  权限声明系统
#[tauri::command]       →  commands.rs:18,47         →  定义可被前端调用的函数
AppHandle               →  commands.rs:48            →  访问应用资源和路径
app.emit()              →  commands.rs:73            →  推送事件到前端
spawn_blocking          →  commands.rs:71-83         →  分离 CPU 密集任务
app.path().app_config_dir() → commands.rs:101        →  获取平台配置目录
#[derive(Deserialize)]  →  commands.rs:31            →  JSON → Rust 结构体
#[derive(Serialize)]    →  commands.rs:23            →  Rust 结构体 → JSON
thiserror               →  error.rs                  →  统一错误类型
rayon par_iter()        →  batch.rs:43               →  多核并行处理
AtomicUsize             →  batch.rs:41,47            →  线程安全计数器
serde rename_all        →  position.rs:11            →  序列化驼峰转蛇形
#[serde(default)]       →  position.rs:37-42         →  可选字段默认值

前端 API                →  项目中位置               →  对应 Rust 端
══════════════════════════════════════════════════════════════════════
invoke("cmd", args)     →  api.ts:各种               →  #[tauri::command]
listen("event", cb)     →  api.ts:onBatchProgress    →  app.emit("event", ...)
open() 文件对话框        →  api.ts:pickImageFiles     →  tauri-plugin-dialog
onDragDropEvent()       →  api.ts:onImageDrop        →  WebView drag-drop
getCurrentWebview()     →  api.ts                    →  WebView 对象
```

---

## 附录：推荐学习路径

1. **零基础入门 Tauri 2：** 从 [tauri.app](https://tauri.app) 官方文档的 Quick Start 开始，跑通一个 Hello World
2. **理解本项目：** 按本文档的文件顺序阅读源代码，从 `main.rs` → `lib.rs` → `commands.rs` → `watermark.rs`
3. **实践：** 在此项目基础上尝试加一个新功能（如：支持文字水印），贯穿 lib.rs 注册命令 → commands.rs 写处理器 → 前端 api.ts 调用 的完整流程
4. **深入：** 学习 Tauri Plugin 开发（如何将自定义 Rust 能力封装为可复用的插件）

---

> 本文档基于 Watermark Studio v0.1.0 源代码编写。
> 所有代码路径和行号对应该版本快照，后续版本可能有所变化。
