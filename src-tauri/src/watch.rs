// 监听文件夹模式
//
// 职责：持续监听输入文件夹，一旦有新图片文件写入完成，自动用当前水印配置
// 处理并输出到输出文件夹（典型用法：Lightroom Classic 导出 JPG 到该文件夹，
// 本 App 自动打水印，无需手动切换 App 操作）。
//
// 设计要点：
// - notify 后台线程 + std mpsc channel 接收文件系统事件（Create/Modify）
// - 稳定性检测：轮询文件大小直到连续两次不变，规避"文件还在写入中"读到半截数据
//   （常见于 Lightroom 导出这种"先写临时内容再落盘完成"的写入方式）
// - 去重：Windows 对同一次写入常连续触发多个事件（Create + 多次 Modify），
//   仅"处理中"去重不够——若第一个事件已处理完、稍晚到达的第二个事件会被误判为新文件。
//   因此处理完成后再进入一段冷却期（DEDUP_COOLDOWN），冷却期内的同路径事件一律忽略。
// - 实时配置：水印配置/参数通过 Arc<Mutex<LiveConfig>> 共享，
//   每次真正处理文件前都读取最新值（而非启动监听时的一次性快照），
//   这样用户在右侧面板改设置能立刻对监听中的任务生效，无需停止重启。
// - 单文件处理直接复用 batch::run（构造只有 1 个路径的 BatchInput），
//   零改动合成/编码/文件名模板逻辑
// - 只处理"启动监听之后新出现"的文件：watcher 建立前已存在的文件不会产生事件，
//   天然满足"不处理旧文件"的需求，无需额外过滤逻辑
// - stop_flag(AtomicBool) 控制后台事件循环退出；WatchHandle 被 drop 时
//   notify::RecommendedWatcher 随之析构，自动停止监听（RAII）

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::batch::{self, BatchInput, ItemResult};
use crate::error::{Result, WatermarkError};
use crate::export::ExportOptions;
use crate::position::WatermarkConfig;

/// 支持监听处理的图片扩展名（与前端 `SUPPORTED_INPUT_EXTS` 保持一致）
const WATCHED_EXTS: [&str; 7] = ["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp"];

/// 稳定性检测：文件大小连续两次轮询不变则认为写入完成
const STABLE_POLL_INTERVAL: Duration = Duration::from_millis(300);
const STABLE_TIMEOUT: Duration = Duration::from_secs(15);

/// 同一路径处理完成后的去重冷却期：期间到达的同路径事件视为同一次写入的尾随事件，忽略
const DEDUP_COOLDOWN: Duration = Duration::from_secs(5);

/// 会随用户在面板里调整设置而实时变化的部分（水印图/水印参数/导出参数/文件名模板）。
/// 输入/输出文件夹在监听期间不允许变更，不放在这里。
#[derive(Clone)]
pub struct LiveConfig {
    pub watermark_bytes: Vec<u8>,
    pub config: WatermarkConfig,
    pub export_options: ExportOptions,
    pub filename_template: String,
}

/// 启动一次监听所需的全部参数
pub struct WatchArgs {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub live: LiveConfig,
}

/// 监听任务句柄：持有即保活，drop 即停止（RAII）
pub struct WatchHandle {
    _watcher: RecommendedWatcher,
    stop_flag: Arc<AtomicBool>,
    live: Arc<Mutex<LiveConfig>>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

impl WatchHandle {
    /// 用最新的水印配置覆盖正在运行的监听任务的 LiveConfig，
    /// 下一个被处理的文件（包括当前正在等待稳定性检测的文件）会用上新值。
    pub fn update_live(&self, live: LiveConfig) {
        *self.live.lock().unwrap() = live;
    }
}

/// 处理完成事件 payload：成功/失败结果之外附带 timestamp，方便前端排序展示
#[derive(Debug, Clone, Serialize)]
struct WatchFileStarted {
    input: String,
}

/// 启动监听：校验参数、建立 notify watcher、起后台线程处理事件循环。
pub fn start(app: AppHandle, args: WatchArgs) -> Result<WatchHandle> {
    if args.input_dir == args.output_dir {
        return Err(WatermarkError::InvalidParam(
            "输入文件夹和输出文件夹不能相同（会导致输出文件被重新监听、循环处理）".to_string(),
        ));
    }
    if !args.input_dir.is_dir() {
        return Err(WatermarkError::InvalidParam(format!(
            "输入文件夹不存在: {}",
            args.input_dir.display()
        )));
    }
    std::fs::create_dir_all(&args.output_dir)?;

    let (tx, rx) = mpsc::channel::<std::result::Result<Event, notify::Error>>();
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| WatermarkError::InvalidParam(format!("创建文件监听器失败: {e}")))?;
    watcher
        .watch(&args.input_dir, RecursiveMode::NonRecursive)
        .map_err(|e| WatermarkError::InvalidParam(format!("监听文件夹失败: {e}")))?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_bg = stop_flag.clone();
    let live = Arc::new(Mutex::new(args.live));
    let live_bg = live.clone();
    let output_dir = args.output_dir;

    std::thread::spawn(move || {
        run_event_loop(app, rx, output_dir, live_bg, stop_flag_bg);
    });

    Ok(WatchHandle {
        _watcher: watcher,
        stop_flag,
        live,
    })
}

/// 去重状态：`active` 记录正在处理中的路径，`recently_done` 记录最近处理完成的路径 + 完成时间，
/// 用于在冷却期内吞掉同一次写入触发的尾随事件。
#[derive(Default)]
struct DedupState {
    active: HashSet<PathBuf>,
    recently_done: HashMap<PathBuf, Instant>,
}

impl DedupState {
    /// 判断该路径此刻是否应该被处理：不在处理中、且不在冷却期内则登记为"处理中"并返回 true。
    fn try_claim(&mut self, path: &Path) -> bool {
        if self.active.contains(path) {
            return false;
        }
        if let Some(done_at) = self.recently_done.get(path) {
            if done_at.elapsed() < DEDUP_COOLDOWN {
                return false;
            }
        }
        self.active.insert(path.to_path_buf());
        true
    }

    /// 标记处理完成：移出 active，进入冷却期；顺带清理已过期的冷却记录避免无限增长。
    fn mark_done(&mut self, path: &Path) {
        self.active.remove(path);
        self.recently_done.insert(path.to_path_buf(), Instant::now());
        self.recently_done
            .retain(|_, t| t.elapsed() < DEDUP_COOLDOWN);
    }
}

/// 后台事件循环：用带超时的 recv 而非阻塞 recv，方便定期检查 stop_flag 及时退出。
fn run_event_loop(
    app: AppHandle,
    rx: mpsc::Receiver<std::result::Result<Event, notify::Error>>,
    output_dir: PathBuf,
    live: Arc<Mutex<LiveConfig>>,
    stop_flag: Arc<AtomicBool>,
) {
    let dedup: Arc<Mutex<DedupState>> = Arc::new(Mutex::new(DedupState::default()));

    while !stop_flag.load(Ordering::SeqCst) {
        let event = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(ev)) => ev,
            Ok(Err(_)) => continue, // notify 内部错误：忽略，继续监听
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // watcher 已被 drop
        };

        if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
            continue;
        }

        for path in event.paths {
            if !is_watched_ext(&path) {
                continue;
            }
            {
                let mut d = dedup.lock().unwrap();
                if !d.try_claim(&path) {
                    continue; // 处理中或处于冷却期，视为同一次写入的尾随事件
                }
            }

            let _ = app.emit(
                "watch-file-started",
                WatchFileStarted {
                    input: path.display().to_string(),
                },
            );

            let app2 = app.clone();
            let dedup2 = dedup.clone();
            let live2 = live.clone();
            let output_dir2 = output_dir.clone();
            let stop_flag2 = stop_flag.clone();
            let path2 = path.clone();

            // 每个文件独立线程处理：互不阻塞，且稳定性等待不卡住事件循环本身
            std::thread::spawn(move || {
                process_new_file(&app2, &path2, &output_dir2, &live2, &stop_flag2);
                dedup2.lock().unwrap().mark_done(&path2);
            });
        }
    }
}

/// 处理单个新文件：等待写入稳定 → 读取最新 LiveConfig → 复用 batch::run 合成/编码/写出 →
/// 推送结果事件。配置在这一刻才读取（而非事件到达时），保证用的是用户当前的最新设置。
fn process_new_file(
    app: &AppHandle,
    path: &Path,
    output_dir: &Path,
    live: &Mutex<LiveConfig>,
    stop_flag: &AtomicBool,
) {
    if !wait_until_stable(path, STABLE_TIMEOUT, STABLE_POLL_INTERVAL) {
        let _ = app.emit(
            "watch-file-processed",
            ItemResult {
                input: path.display().to_string(),
                output: None,
                error: Some("等待文件写入完成超时，已跳过".to_string()),
            },
        );
        return;
    }
    // 等待期间监听可能已被用户停止，此时放弃处理（避免停止后仍产出文件造成困惑）
    if stop_flag.load(Ordering::SeqCst) {
        return;
    }

    let snapshot = live.lock().unwrap().clone();
    let task = BatchInput {
        input_paths: vec![path.to_path_buf()],
        output_dir: output_dir.to_path_buf(),
        watermark_bytes: snapshot.watermark_bytes,
        config: snapshot.config,
        export_options: snapshot.export_options,
        filename_template: snapshot.filename_template,
    };
    let results = batch::run(&task, |_, _, _, _| {});
    if let Some(result) = results.into_iter().next() {
        let _ = app.emit("watch-file-processed", result);
    }
}

/// 轮询文件大小，连续两次（间隔 `poll_interval`）不变即视为写入完成。
/// 超时未稳定返回 `false`，调用方应放弃该文件，避免无限等待卡住处理线程。
fn wait_until_stable(path: &Path, timeout: Duration, poll_interval: Duration) -> bool {
    let start = Instant::now();
    let mut last_size: Option<u64> = None;

    while start.elapsed() < timeout {
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => {
                // 文件可能处于"改名落盘"过渡瞬间，稍后重试
                std::thread::sleep(poll_interval);
                continue;
            }
        };
        if Some(size) == last_size {
            return true;
        }
        last_size = Some(size);
        std::thread::sleep(poll_interval);
    }
    false
}

/// 扩展名是否属于支持监听处理的图片格式（大小写不敏感）
fn is_watched_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| WATCHED_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn stable_file_detected_quickly() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        fs::write(&path, b"already complete").unwrap();

        let ok = wait_until_stable(&path, Duration::from_secs(2), Duration::from_millis(20));
        assert!(ok, "已写完的文件应很快被判定为稳定");
    }

    #[test]
    fn growing_file_waits_until_stable() {
        // 第二次写入必须发生在第一个轮询周期内（远小于 poll_interval），
        // 保证函数第二次读取文件大小时能"看到"变化、正确重置稳定计时，
        // 而不是在写入完成前就误判为稳定（poll_interval 与写入延迟需要有足够安全边际，
        // 避免 Windows 默认约 15ms 的计时器粒度导致测试抖动）。
        let dir = tempdir().unwrap();
        let path = dir.path().join("b.jpg");
        fs::write(&path, b"part1").unwrap();

        let path2 = path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            let mut f = fs::OpenOptions::new().append(true).open(&path2).unwrap();
            f.write_all(b"part2-more-bytes").unwrap();
        });

        let ok = wait_until_stable(&path, Duration::from_secs(2), Duration::from_millis(150));
        assert!(ok, "写入完成后应最终判定为稳定");
        let final_size = fs::metadata(&path).unwrap().len();
        assert_eq!(final_size, b"part1part2-more-bytes".len() as u64);
    }

    #[test]
    fn missing_file_times_out() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does_not_exist.jpg");
        let ok = wait_until_stable(&path, Duration::from_millis(100), Duration::from_millis(20));
        assert!(!ok, "文件始终不存在应超时返回 false");
    }

    #[test]
    fn extension_filter_matches_known_image_types() {
        assert!(is_watched_ext(Path::new("a.jpg")));
        assert!(is_watched_ext(Path::new("A.JPG")));
        assert!(is_watched_ext(Path::new("a.png")));
        assert!(is_watched_ext(Path::new("a.tiff")));
        assert!(!is_watched_ext(Path::new("a.xmp")));
        assert!(!is_watched_ext(Path::new("a.tmp")));
        assert!(!is_watched_ext(Path::new("noext")));
    }

    #[test]
    fn dedup_blocks_while_active_and_during_cooldown() {
        let mut d = DedupState::default();
        let p = PathBuf::from("/tmp/a.jpg");

        assert!(d.try_claim(&p), "第一次应能成功登记");
        assert!(!d.try_claim(&p), "处理中的同一路径应被拒绝");

        d.mark_done(&p);
        assert!(
            !d.try_claim(&p),
            "刚完成、仍在冷却期内的同一路径应被拒绝（防止尾随事件重复处理）"
        );
    }

    #[test]
    fn dedup_allows_after_cooldown_expires() {
        let mut d = DedupState::default();
        let p = PathBuf::from("/tmp/b.jpg");
        d.try_claim(&p);
        // 手动把完成时间设为很久以前，模拟冷却期已过
        d.recently_done
            .insert(p.clone(), Instant::now() - Duration::from_secs(60));
        d.active.remove(&p);
        assert!(d.try_claim(&p), "冷却期已过应允许重新处理");
    }
}
