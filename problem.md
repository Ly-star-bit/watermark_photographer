# 已知问题与修复记录

## 2026-07-01 · 文字水印导出后不可见（重叠成小团）

### 症状

导出的照片上，文字水印区域只在图片左上角出现一个白色小团，看起来像所有字符**叠在同一位置**。
用户输入自定义文字 `123456` 时表现最明显：可以清晰看出 6 个数字全部堆在同一格宽度里。

同时预览面板（Canvas fillText）显示正常，只有 Rust 导出结果异常——说明 bug 在 Rust 渲染路径。

### 根本原因

`src-tauri/src/exif_text.rs::render_text` 里逐字调用 `OutlinedGlyph::draw` 时，
把回调收到的 `(px, py)` 当成了**画布绝对坐标**：

```rust
outlined.draw(|px, py, coverage| {
    let ix = px as u32;     // ❌ 错：px 是 glyph 局部坐标
    let iy = py as u32;     // ❌ 错：py 是 glyph 局部坐标
    ...
});
```

但 `ab_glyph 0.2.32` 的实际语义是：`draw` 内部创建的 `Rasterizer::new(w, h)`
尺寸就是 `px_bounds().width() × px_bounds().height()`，然后 `for_each_pixel_2d(o)`
遍历这个局部矩阵，回调收到的 **`(x, y)` 是 0..w、0..h 的局部像素索引**，
而不是画布上的绝对坐标。

因此不管 `x_cursor` 怎么推进、`Glyph::position` 怎么设置，
每个字符的像素都被写到画布 `(0..bounds_w, 0..bounds_h)` 那一小块——
79 个字符全部叠成一个小团。

来源验证（ab-glyph 源码 `glyph/src/outlined.rs`）：

```rust
let (w, h) = (
    self.px_bounds.width() as usize,
    self.px_bounds.height() as usize,
);
self.outline.curves.iter()
    .fold(Rasterizer::new(w, h), |...| ...)
    .for_each_pixel_2d(o);   // 回调坐标域是 [0, w) × [0, h)
```

### 修复

把回调坐标手动加上 `px_bounds().min` 偏移，转成画布绝对坐标：

```rust
let bb = outlined.px_bounds();
let offset_x = bb.min.x as i32;
let offset_y = bb.min.y as i32;
outlined.draw(|px, py, coverage| {
    let ax = px as i32 + offset_x;
    let ay = py as i32 + offset_y;
    if ax < 0 || ay < 0 { return; }
    let ix = ax as u32;
    let iy = ay as u32;
    ...
});
```

### 诊断过程中发现的次要 bug（已顺带修复）

1. **字号是绝对像素而非相对图片比例**
   `font_size: f32` 默认 36px，在 24MP 照片上文字只占 0.6% 宽度、几乎不可见。
   改为 `font_size_ratio: f32`（默认 0.03 = 长边 3%），`render` 新增 `img_w, img_h`
   参数按 `长边 × ratio` 换算实际字号。

2. **EXIF 字符串带引号**
   Make/Model/LensModel 用了 `f.display_value().to_string()`，
   `exif` crate 会自动把 ASCII 字段加双引号（显示成 `"FUJIFILM"`）。
   新增 `extract_ascii()` 从 `Value::Ascii(Vec<Vec<u8>>)` 提取原始字节。

3. **`{iso}` 不替换**
   只匹配了 `Tag::ISOSpeed`（0x8833，冷门），主流相机（含富士）写的是
   `Tag::PhotographicSensitivity`（0x8827）。补上多标签匹配，
   PhotographicSensitivity 命中时覆盖，ISOSpeed 命中时作为后备。

4. **alpha 阈值过滤把抗锯齿边缘全丢了**
   原代码 `if alpha > pixel[3]` 里 `pixel[3]` 是背景 80，
   要求 `coverage > 0.37` 才画像素，导致 92px 字体每笔画只有中心 1 像素被绘制。
   改为标准 source-over 合成（`out = src·α_s + dst·α_d·(1−α_s)`）。

### 教训

- **不要假设第三方 API 的坐标系**——`OutlinedGlyph::draw` 的文档说的是
  "pixel coordinates within the glyph's bounding box"，字面意义就是**局部坐标**，
  但直觉容易误解为"在位置化后的画布坐标系里的绝对坐标"。
  遇到分布式渲染 bug 优先怀疑坐标系。

- **单元测试要覆盖"多字符"场景**——原有测试 `custom_text_renders_ignoring_exif`
  只检查了 `result.is_some()`，没有验证像素分布，所以叠字 bug 一直没被暴露。
  后续可以加：渲染 `"ABCDEF"` 后检查图像右半区应有非背景像素。
