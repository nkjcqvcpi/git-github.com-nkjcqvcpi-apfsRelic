//! Recently-opened images, persisted as JSON in the per-user app-config dir.
//!
//! The list feeds the start page. It is best-effort state, never trusted:
//! entries are re-validated (`exists`) when read, a corrupt or unreadable
//! file is treated as an empty list, and writes go through a temp file +
//! atomic rename so a crash can't leave a half-written list.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MAX_RECENTS: usize = 10;

#[derive(Serialize, Deserialize, Clone)]
pub struct RecentImage {
    pub path: String,
    /// Unix seconds of the last successful open.
    pub last_opened: u64,
}

fn recents_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("recents.json"))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load the persisted list; missing or corrupt files read as empty.
pub fn load(app: &AppHandle) -> Vec<RecentImage> {
    let Some(file) = recents_file(app) else {
        return Vec::new();
    };
    let Ok(data) = fs::read(&file) else {
        return Vec::new();
    };
    serde_json::from_slice(&data).unwrap_or_default()
}

fn save(app: &AppHandle, recents: &[RecentImage]) {
    let Some(file) = recents_file(app) else {
        return;
    };
    let Some(dir) = file.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!(".recents.json.{}", std::process::id()));
    if let Ok(data) = serde_json::to_vec_pretty(recents) {
        if fs::write(&tmp, data).is_ok() && fs::rename(&tmp, &file).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}

/// Record a successful open of `path`, moving it to the front of the list.
pub fn record(app: &AppHandle, path: &Path) {
    let path = path.to_string_lossy().to_string();
    let mut recents = load(app);
    recents.retain(|r| r.path != path);
    recents.insert(
        0,
        RecentImage {
            path,
            last_opened: now_epoch(),
        },
    );
    recents.truncate(MAX_RECENTS);
    save(app, &recents);
}

/// Drop `path` from the list (the start page's "remove" action).
pub fn remove(app: &AppHandle, path: &str) {
    let mut recents = load(app);
    recents.retain(|r| r.path != path);
    save(app, &recents);
}
