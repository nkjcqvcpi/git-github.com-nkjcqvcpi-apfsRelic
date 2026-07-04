//! apfsRelic desktop GUI (Tauri v2).
//!
//! A read-only APFS file explorer that links `apfsrelic-core` directly and
//! recovers files through native dialogs. With no image selected the frontend
//! shows a start page (recent images + an open dialog); `$APFSRELIC_CONTAINER`
//! / `$APFSRELIC_VOLUME` preseed the selection for development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod recents;

use std::path::PathBuf;

use commands::AppState;

fn main() {
    let container = std::env::var("APFSRELIC_CONTAINER").ok().map(PathBuf::from);
    let volume: u32 = std::env::var("APFSRELIC_VOLUME")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(container, volume))
        .invoke_handler(tauri::generate_handler![
            commands::config,
            commands::ls,
            commands::stat,
            commands::inspect,
            commands::recover,
            commands::recover_batch,
            commands::pick_container,
            commands::open_container,
            commands::close_container,
            commands::set_volume,
            commands::recent_images,
            commands::remove_recent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running apfsRelic GUI");
}
