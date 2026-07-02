#!/usr/bin/env bash
# 在 macOS 上一键打包 .app 和 .dmg。
# 用法：把整个项目 clone/copy 到 Mac，然后：
#   chmod +x build_mac.sh
#   ./build_mac.sh
#
# 前置依赖（首次运行前手动装）：
#   1. Xcode Command Line Tools:  xcode-select --install
#   2. Rust:                       curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
#   3. Node.js 20+                 brew install node
#   4. create-dmg（可选，用于生成美观的 dmg）:
#                                  brew install create-dmg

set -euo pipefail

# 加载 cargo（首次装完 rustup 后未 source 时兜底）
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

cd "$(dirname "$0")"

echo "▶ 安装 npm 依赖..."
npm install

echo "▶ 执行 tauri build (Universal binary if targeted)..."
# 默认构建当前架构（Intel 或 Apple Silicon）
# 想同时支持两种架构，改为：
#   npm run tauri build -- --target universal-apple-darwin
# 需先：rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run tauri build

echo ""
echo "✅ 完成"
echo ""
echo "产物位置："
echo "  .app  →  src-tauri/target/release/bundle/macos/签名水印.app"
echo "  .dmg  →  src-tauri/target/release/bundle/dmg/签名水印_0.1.0_{arch}.dmg"
echo ""
echo "首次打开 .app 或 .dmg 时，因未做代码签名，macOS 会拦截："
echo "  在 Finder 中右键 .app → 打开 → 确认，或"
echo "  系统设置 → 隐私与安全性 → 允许打开"
