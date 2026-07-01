// 预设持久化
//
// 存储位置：{app_config_dir}/presets.json
// 数据结构：一个 Preset 数组，同名视为更新
// 一个 Preset 包含：名称、水印配置、可选的水印图路径（方便一键还原签名素材）

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::Result;
use crate::position::WatermarkConfig;

const PRESETS_FILE: &str = "presets.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub config: WatermarkConfig,
    #[serde(default)]
    pub watermark_path: Option<String>,
}

/// 读取全部预设。文件不存在或为空视为 []。
pub fn load_all(config_dir: &Path) -> Result<Vec<Preset>> {
    let path = config_dir.join(PRESETS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let s = std::fs::read_to_string(&path)?;
    if s.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&s)?)
}

fn save_all(config_dir: &Path, presets: &[Preset]) -> Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = config_dir.join(PRESETS_FILE);
    let json = serde_json::to_string_pretty(presets)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// 插入或按 name 更新。返回更新后的完整列表。
pub fn upsert(config_dir: &Path, preset: Preset) -> Result<Vec<Preset>> {
    let mut all = load_all(config_dir)?;
    match all.iter_mut().find(|p| p.name == preset.name) {
        Some(existing) => *existing = preset,
        None => all.push(preset),
    }
    save_all(config_dir, &all)?;
    Ok(all)
}

/// 按 name 删除。返回更新后的完整列表。若不存在则等价于 no-op。
pub fn delete(config_dir: &Path, name: &str) -> Result<Vec<Preset>> {
    let mut all = load_all(config_dir)?;
    all.retain(|p| p.name != name);
    save_all(config_dir, &all)?;
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::GridPosition;
    use tempfile::tempdir;

    fn sample(name: &str) -> Preset {
        Preset {
            name: name.to_string(),
            config: WatermarkConfig {
                position: GridPosition::BottomRight,
                size_ratio: 0.15,
                opacity: 0.8,
                margin_x: 30,
                margin_y: 30,
                landscape_override: None,
                tint: None,
                exif_text: None,
            },
            watermark_path: Some("C:/sig.png".to_string()),
        }
    }

    #[test]
    fn load_from_missing_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let list = load_all(dir.path()).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn upsert_creates_and_updates() {
        let dir = tempdir().unwrap();
        let list = upsert(dir.path(), sample("微博发图")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "微博发图");

        // 同名 upsert 应替换，非追加
        let mut updated = sample("微博发图");
        updated.config.size_ratio = 0.25;
        let list = upsert(dir.path(), updated).unwrap();
        assert_eq!(list.len(), 1);
        assert!((list[0].config.size_ratio - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn upsert_multiple_and_persists() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), sample("A")).unwrap();
        upsert(dir.path(), sample("B")).unwrap();
        let list = upsert(dir.path(), sample("C")).unwrap();
        assert_eq!(list.len(), 3);

        // 重新从磁盘加载应仍有 3 条
        let reloaded = load_all(dir.path()).unwrap();
        assert_eq!(reloaded.len(), 3);
        let names: Vec<&str> = reloaded.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"A") && names.contains(&"B") && names.contains(&"C"));
    }

    #[test]
    fn delete_removes_by_name() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), sample("A")).unwrap();
        upsert(dir.path(), sample("B")).unwrap();
        let list = delete(dir.path(), "A").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "B");
    }

    #[test]
    fn delete_nonexistent_is_noop() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), sample("A")).unwrap();
        let list = delete(dir.path(), "不存在").unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn preset_json_roundtrip_preserves_config() {
        let dir = tempdir().unwrap();
        let mut p = sample("测试");
        p.config.opacity = 0.5;
        p.config.margin_x = 42;
        upsert(dir.path(), p).unwrap();

        let reloaded = load_all(dir.path()).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert!((reloaded[0].config.opacity - 0.5).abs() < f32::EPSILON);
        assert_eq!(reloaded[0].config.margin_x, 42);
        assert_eq!(reloaded[0].watermark_path.as_deref(), Some("C:/sig.png"));
    }
}
