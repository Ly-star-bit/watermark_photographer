// 水印工具后端库入口
// P1 阶段仅注册基础插件；后续阶段会挂接 commands 模块

mod commands;
mod error;
mod watermark;
mod position;
mod metadata;
mod batch;
mod preset;
mod export;
mod exif_text;
mod frame;
mod canvas_expand;
mod watch;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(std::sync::Mutex::new(commands::WatchState::default()))
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::export_batch,
            commands::list_presets,
            commands::save_preset,
            commands::delete_preset,
            commands::create_thumbnail,
            commands::preview_exif_text,
            commands::preview_frame,
            commands::start_watch,
            commands::stop_watch,
            commands::get_watch_status,
            commands::update_watch_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
