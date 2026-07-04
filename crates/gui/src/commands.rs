//! Tauri commands — the GUI's bridge to `apfsrelic-core`.
//!
//! Every command opens the configured container read-only through the core
//! engine and returns JSON to the webview. Nothing here shells out to the CLI:
//! the GUI links the same audited library the CLI does (`core -> gui`). The
//! read-only guarantee therefore holds exactly as in the CLI — the only writes
//! are to the user-chosen recovery destination (temp file + atomic rename) and
//! the recently-opened list in the app-config dir.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use apfsrelic_core::apfs::btree::{split_obj_id_and_type, BtreeReader, Record};
use apfsrelic_core::apfs::extract::Written;
use apfsrelic_core::apfs::jrec::{self, FileExtent, Inode, Xattr};
use apfsrelic_core::apfs::path as apath;
use apfsrelic_core::apfs::vol::Volume;
use apfsrelic_core::apfs::{raw, snapshot, time};
use apfsrelic_core::device::partition::PartitionSelector;
use apfsrelic_core::device::SparseBundleDevice;
use apfsrelic_core::source::{self, ImageKind, OpenOptions as ImageOpenOptions, OpenedImage};

use crate::recents;

/// Deepest folder nesting `recover_batch` will walk (mirrors the CLI guard).
const MAX_DIR_DEPTH: u32 = 256;
/// Per-file results reported back to the webview before truncating (totals
/// keep counting past this — it only bounds the JSON payload).
const MAX_BATCH_RESULTS: usize = 1000;

/// The container/volume the GUI is currently browsing. `container == None`
/// means nothing is open yet and the frontend shows the start page.
pub struct Config {
    pub container: Option<PathBuf>,
    pub volume: u32,
}

/// Shared, mutable selection guarded by a mutex.
pub struct AppState {
    pub inner: Mutex<Config>,
}

impl AppState {
    pub fn new(container: Option<PathBuf>, volume: u32) -> Self {
        AppState {
            inner: Mutex::new(Config { container, volume }),
        }
    }

    /// A cheap snapshot of the current selection (clone so the lock isn't held
    /// across the blocking image I/O).
    fn snapshot(&self) -> (Option<PathBuf>, u32) {
        let c = self.inner.lock().unwrap();
        (c.container.clone(), c.volume)
    }
}

/// The current selection, or an error the webview can show when no image is
/// open (commands should never race the start page, but don't panic if so).
fn require_container(state: &State<AppState>) -> Result<(PathBuf, u32), String> {
    let (container, volume) = state.snapshot();
    container
        .map(|c| (c, volume))
        .ok_or_else(|| "no disk image is open".to_string())
}

/// Open the image read-only: GPT/partition detection + container superblock.
fn open_image(container: &Path) -> Result<OpenedImage, String> {
    source::open(&ImageOpenOptions {
        container_path: container.to_path_buf(),
        partition: PartitionSelector::Auto,
        offset: None,
        max_xid: u64::MAX,
    })
    .map_err(|e| e.to_string())
}

/// Open the configured container read-only and return the selected volume.
///
/// The returned [`Volume`] owns its own device handle (an `Arc`), so the
/// short-lived `OpenedImage` can be dropped here.
fn open_volume(container: &Path, volume: u32) -> Result<Volume, String> {
    let opened = open_image(container)?;
    Volume::open(&opened.container, volume).map_err(|e| e.to_string())
}

/// The last path component, used as the default recovery filename.
fn basename(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("recovered")
        .to_string()
}

/// Build the config JSON for a snapshot (shared by several commands).
fn config_json(container: &Option<PathBuf>, volume: u32) -> Value {
    match container {
        Some(c) => {
            let cstr = c.to_string_lossy().to_string();
            json!({
                "container": cstr,
                "volume": volume,
                "container_exists": c.exists(),
                "is_sparsebundle": cstr.ends_with(".sparsebundle"),
            })
        }
        None => json!({
            "container": Value::Null,
            "volume": volume,
            "container_exists": false,
            "is_sparsebundle": false,
        }),
    }
}

/// Report the current container/volume selection.
#[tauri::command]
pub fn config(state: State<AppState>) -> Value {
    let (container, volume) = state.snapshot();
    config_json(&container, volume)
}

/// List a directory: `{ path, fsoid, entries: [{ name, type, fsoid, size? }] }`.
/// Entries are sorted directories-first, then case-insensitively by name.
// `async` so Tauri runs it off the main thread: the blocking image I/O below
// must not freeze the webview's event loop.
#[tauri::command(async)]
pub fn ls(state: State<AppState>, path: String, sizes: bool) -> Result<Value, String> {
    let (container, volume) = require_container(&state)?;
    let vol = open_volume(&container, volume)?;
    let bt = vol.btree();

    let dir = apath::resolve(&vol, &bt, &path).map_err(|e| e.to_string())?;
    let mut dir_entries = vol.list_dir(&bt, dir.fsoid).map_err(|e| e.to_string())?;
    dir_entries.sort_by(|a, b| {
        let a_is_file = a.type_name() != "dir";
        let b_is_file = b.type_name() != "dir";
        a_is_file
            .cmp(&b_is_file)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let mut entries = Vec::with_capacity(dir_entries.len());
    for d in &dir_entries {
        let mut obj = json!({
            "name": d.name,
            "type": d.type_name(),
            "fsoid": format!("{:#x}", d.file_id),
        });
        if sizes {
            if let Some(inode) = vol.inode(&bt, d.file_id).map_err(|e| e.to_string())? {
                if let Some(size) = inode.logical_size() {
                    obj["size"] = json!(size);
                }
            }
        }
        entries.push(obj);
    }

    Ok(json!({
        "path": path,
        "fsoid": format!("{:#x}", dir.fsoid),
        "entries": entries,
    }))
}

// ---------------------------------------------------------------------------
// Image selection (start page)
// ---------------------------------------------------------------------------

/// The recently-opened images for the start page, most recent first.
#[tauri::command]
pub fn recent_images(app: AppHandle) -> Value {
    let list: Vec<Value> = recents::load(&app)
        .into_iter()
        .map(|r| {
            json!({
                "path": r.path,
                "last_opened": r.last_opened,
                "exists": Path::new(&r.path).exists(),
                "is_sparsebundle": r.path.ends_with(".sparsebundle"),
            })
        })
        .collect();
    json!(list)
}

/// Drop one entry from the recent list and return the updated list.
#[tauri::command]
pub fn remove_recent(app: AppHandle, path: String) -> Value {
    recents::remove(&app, &path);
    recent_images(app)
}

/// Open `path` as the browsed container: validate it parses as an APFS image
/// (GPT scan + container superblock), reset the volume to 1, and record it in
/// the recent list. On error the previous selection is left untouched.
// `async` so the blocking image I/O runs off the main thread.
#[tauri::command(async)]
pub fn open_container(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<Value, String> {
    let p = PathBuf::from(&path);
    open_image(&p)?;
    {
        let mut cfg = state.inner.lock().unwrap();
        cfg.container = Some(p.clone());
        cfg.volume = 1;
    }
    recents::record(&app, &p);
    Ok(config(state))
}

/// Pick a new container with a native dialog and switch to it. Uses a *file*
/// picker (not a folder picker) because a `.sparsebundle` is a macOS file
/// package that a directories-only panel won't let the user select — and this
/// also accepts raw image files. `async` because `blocking_pick_file()` on the
/// main thread would deadlock: the dialog needs the event loop it's blocking.
#[tauri::command(async)]
pub fn pick_container(app: AppHandle, state: State<AppState>) -> Result<Value, String> {
    match app.dialog().file().blocking_pick_file() {
        Some(fp) => {
            let p = fp.into_path().map_err(|e| e.to_string())?;
            open_container(app, state, p.to_string_lossy().to_string())
        }
        None => Ok(json!({ "cancelled": true })),
    }
}

/// Close the current image and return to the start page.
#[tauri::command]
pub fn close_container(state: State<AppState>) -> Value {
    state.inner.lock().unwrap().container = None;
    config(state)
}

/// Change the 1-based volume index.
#[tauri::command]
pub fn set_volume(state: State<AppState>, volume: u32) -> Value {
    if volume >= 1 {
        state.inner.lock().unwrap().volume = volume;
    }
    config(state)
}

// ---------------------------------------------------------------------------
// Metadata (dashboard side panel)
// ---------------------------------------------------------------------------

/// Everything the dashboard shows about the open image: backing device,
/// container superblock, per-volume summaries, sparsebundle band statistics,
/// and the selected volume's snapshots.
#[tauri::command(async)]
pub fn inspect(state: State<AppState>) -> Result<Value, String> {
    let (container_path, volume) = require_container(&state)?;
    let opened = open_image(&container_path)?;
    let nx = &opened.container.nx;

    let mut result = json!({
        "image": {
            "path": container_path.to_string_lossy(),
            "kind": if opened.kind == ImageKind::SparseBundle { "sparsebundle" } else { "raw" },
            "size": opened.image.size(),
            "container_offset": opened.container_offset,
            "partition_index": opened.partition_index,
        },
        "container": {
            "uuid": nx.uuid,
            "block_size": nx.block_size,
            "block_count": nx.block_count,
            "total_bytes": nx.block_count.saturating_mul(nx.block_size as u64),
            "features": format!("{:#x}", nx.features),
            "readonly_compatible_features": format!("{:#x}", nx.readonly_compatible_features),
            "incompatible_features": format!("{:#x}", nx.incompatible_features),
            "keylocker": nx.has_keylocker(),
            "max_file_systems": nx.max_file_systems,
            "checkpoint_xid": format!("{:#x}", opened.container.checkpoint_xid),
            "checkpoint_index": opened.container.checkpoint_index,
        },
        "warnings": opened.container.warnings,
    });

    let slots = opened.container.list_volumes().map_err(|e| e.to_string())?;
    let volumes: Vec<Value> = slots
        .iter()
        .map(|s| {
            let a = &s.apsb;
            json!({
                "index": s.index,
                "name": a.volname,
                "role": a.role_name(),
                "uuid": a.vol_uuid,
                "encrypted": a.is_encrypted(),
                "sealed": a.is_sealed(),
                "case_insensitive": a.is_case_insensitive(),
                "num_files": a.num_files,
                "num_directories": a.num_directories,
                "num_symlinks": a.num_symlinks,
                "num_snapshots": a.num_snapshots,
                "last_modified": time::iso8601(a.last_mod_time),
            })
        })
        .collect();
    result["volumes"] = json!(volumes);

    if opened.kind == ImageKind::SparseBundle {
        if let Ok(dev) = SparseBundleDevice::open(&container_path) {
            if let Ok(stats) = dev.band_stats() {
                result["sparsebundle"] = json!({
                    "band_size": stats.band_size,
                    "logical_size": stats.logical_size,
                    "expected_bands": stats.expected_bands,
                    "present_bands": stats.present_bands,
                    "missing_band_count": stats.expected_bands - stats.present_bands,
                    "missing_truncated": stats.missing_truncated,
                    "short_band_count": stats.short_bands.len(),
                    "uuid": dev.uuid(),
                    "backingstore_version": dev.backingstore_version(),
                });
            }
        }
    }

    // Snapshots of the selected volume — best-effort: an unreadable volume
    // should not hide the rest of the dashboard.
    match Volume::open(&opened.container, volume) {
        Ok(vol) => {
            let bt = vol.btree();
            match snapshot::list_snapshots(&vol, &bt) {
                Ok(snaps) => {
                    let snaps: Vec<Value> = snaps
                        .iter()
                        .map(|s| {
                            json!({
                                "name": s.name,
                                "xid": format!("{:#x}", s.xid),
                                "create_time": time::iso8601(s.create_time),
                            })
                        })
                        .collect();
                    result["snapshots"] = json!(snaps);
                }
                Err(e) => result["snapshots_error"] = json!(e.to_string()),
            }
        }
        Err(e) => result["snapshots_error"] = json!(e.to_string()),
    }

    Ok(result)
}

/// All known metadata for one file/dir/symlink: the inode, times, ownership,
/// xattrs, symlink target, and whether the data looks recoverable. Feeds the
/// "Selected item" section of the dashboard.
#[tauri::command(async)]
pub fn stat(state: State<AppState>, path: String) -> Result<Value, String> {
    let (container, volume) = require_container(&state)?;
    let vol = open_volume(&container, volume)?;
    let bt = vol.btree();

    let resolved = apath::resolve(&vol, &bt, &path).map_err(|e| e.to_string())?;
    let fsoid = resolved.fsoid;
    let records = vol.records(&bt, fsoid).map_err(|e| e.to_string())?;
    if records.is_empty() {
        return Err(format!("no records for FSOID {fsoid:#x}"));
    }
    let inode = Volume::inode_from_records(&records).map_err(|e| e.to_string())?;

    let mut result = json!({ "path": path, "fsoid": format!("{fsoid:#x}") });

    if let Some(i) = &inode {
        let kind = if i.is_dir() {
            "dir"
        } else if i.is_symlink() {
            "symlink"
        } else if i.is_regular() {
            "file"
        } else {
            "other"
        };
        let mut o = json!({
            "kind": kind,
            "mode": format!("{:#o}", i.mode),
            "uid": i.owner,
            "gid": i.group,
            "nchildren_or_nlink": i.nchildren_or_nlink,
            "sparse": i.is_sparse(),
            "has_rsrc_fork": i.has_rsrc_fork(),
            "has_finder_info": i.has_finder_info(),
            "internal_flags": format!("{:#x}", i.internal_flags),
            "bsd_flags": format!("{:#x}", i.bsd_flags),
        });
        if let Some(sz) = i.logical_size() {
            o["size"] = json!(sz);
        }
        if let Some(a) = i.allocated_size() {
            o["allocated_size"] = json!(a);
        }
        for (key, nanos) in [
            ("create_time", i.create_time),
            ("mod_time", i.mod_time),
            ("change_time", i.change_time),
            ("access_time", i.access_time),
        ] {
            if let Some(t) = time::iso8601(nanos) {
                o[key] = json!(t);
            }
        }
        result["inode"] = o;
        result["recoverability"] = recoverability(&vol, i, &records)?;

        if i.is_symlink() {
            if let Some(target) = Volume::symlink_target(&records).map_err(|e| e.to_string())? {
                result["symlink_target"] = json!(target);
            }
        }
    } else {
        result["inode"] = Value::Null;
    }

    let mut xattrs = Vec::new();
    for rec in &records {
        let (_oid, ty) =
            split_obj_id_and_type(raw::u64_at(&rec.key, 0).map_err(|e| e.to_string())?);
        if ty == jrec::APFS_TYPE_XATTR {
            if let Ok(x) = Xattr::parse(&rec.key, &rec.val) {
                xattrs.push(json!({ "name": x.name, "data_len": x.data.len() }));
            }
        }
    }
    result["xattrs"] = json!(xattrs);

    Ok(result)
}

/// Explain whether (and why) the object's data is recoverable (mirrors the
/// CLI `stat` report).
fn recoverability(vol: &Volume, inode: &Inode, records: &[Record]) -> Result<Value, String> {
    if vol.apsb.is_encrypted() {
        return Ok(json!({
            "status": "encrypted",
            "reason": "volume is encrypted; data cannot be decrypted",
        }));
    }
    if inode.is_dir() {
        return Ok(json!({
            "status": "ok",
            "reason": "directory (contents recovered recursively)",
        }));
    }
    if inode.is_symlink() {
        return Ok(json!({ "status": "ok", "reason": "symlink" }));
    }
    let size = inode.logical_size().unwrap_or(0);
    let mut has_extent = false;
    let mut has_hole = false;
    for rec in records {
        let (_oid, ty) =
            split_obj_id_and_type(raw::u64_at(&rec.key, 0).map_err(|e| e.to_string())?);
        if ty == jrec::APFS_TYPE_FILE_EXTENT {
            let fe = FileExtent::parse(&rec.key, &rec.val).map_err(|e| e.to_string())?;
            has_extent = true;
            if fe.is_hole() {
                has_hole = true;
            }
        }
    }
    let status = if size == 0 || (has_extent && !has_hole) {
        "ok"
    } else if has_extent {
        "ok-sparse"
    } else {
        "no-extents"
    };
    Ok(json!({ "status": status, "size": size, "has_extents": has_extent }))
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

/// Recover one file (or symlink) to a location chosen via a native save
/// dialog. Folders and multi-selections go through [`recover_batch`].
// `async` so Tauri runs it off the main thread. A synchronous command runs on
// the main thread, where `blocking_save_file()` (below) would deadlock: the
// native dialog needs the main event loop that the command is blocking.
#[tauri::command(async)]
pub fn recover(app: AppHandle, state: State<AppState>, path: String) -> Result<Value, String> {
    let (container, volume) = require_container(&state)?;
    let vol = open_volume(&container, volume)?;

    if vol.apsb.is_encrypted() {
        return Err("volume is encrypted; refusing plaintext recovery".into());
    }

    let bt = vol.btree();
    let resolved = apath::resolve(&vol, &bt, &path).map_err(|e| e.to_string())?;
    let fsoid = resolved.fsoid;
    let records = vol.records(&bt, fsoid).map_err(|e| e.to_string())?;
    let inode = Volume::inode_from_records(&records)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no inode for {fsoid:#x}"))?;

    if inode.is_dir() {
        return Err(
            "folders are recovered via batch recovery (select the row and use “Recover selected”)"
                .into(),
        );
    }

    // Ask where to save (Rust-side dialog; no plugin permission needed).
    let name = basename(&path);
    let dest = match app
        .dialog()
        .file()
        .set_file_name(&name)
        .blocking_save_file()
    {
        Some(fp) => fp.into_path().map_err(|e| e.to_string())?,
        None => return Ok(json!({ "saved": false, "cancelled": true })),
    };

    // Symlink: recreate the link rather than writing extents.
    if inode.is_symlink() {
        if let Some(target) = Volume::symlink_target(&records).map_err(|e| e.to_string())? {
            let _ = fs::remove_file(&dest);
            std::os::unix::fs::symlink(&target, &dest).map_err(|e| e.to_string())?;
            return Ok(json!({
                "saved": true,
                "path": dest.display().to_string(),
                "symlink": target,
                "bytes": target.len(),
                "complete": true,
            }));
        }
    }

    let size = inode.logical_size().unwrap_or(0);
    let written = write_atomic(&vol, &records, size, &dest).map_err(|e| e.to_string())?;
    Ok(json!({
        "saved": true,
        "path": dest.display().to_string(),
        "bytes": written.bytes,
        "complete": written.bytes == size,
    }))
}

/// Accumulates per-file outcomes for a batch recovery and streams progress
/// events to the webview.
struct BatchCtx<'a> {
    app: &'a AppHandle,
    results: Vec<Value>,
    files_done: u64,
    bytes_total: u64,
    recovered: u64,
    partial: u64,
    errors: u64,
}

impl BatchCtx<'_> {
    fn new(app: &AppHandle) -> BatchCtx<'_> {
        BatchCtx {
            app,
            results: Vec::new(),
            files_done: 0,
            bytes_total: 0,
            recovered: 0,
            partial: 0,
            errors: 0,
        }
    }

    fn push(&mut self, shown: &str, status: &str, bytes: u64, note: Option<String>) {
        self.files_done += 1;
        self.bytes_total += bytes;
        match status {
            "recovered" => self.recovered += 1,
            "partial" => self.partial += 1,
            _ => self.errors += 1,
        }
        if self.results.len() < MAX_BATCH_RESULTS {
            let mut o = json!({ "path": shown, "status": status, "bytes": bytes });
            if let Some(n) = note {
                o["note"] = json!(n);
            }
            self.results.push(o);
        }
        // Stream progress; thin out events once a big tree gets going so a
        // huge folder doesn't flood the IPC channel.
        if self.files_done <= 100 || self.files_done.is_multiple_of(25) {
            let _ = self.app.emit(
                "recover-progress",
                json!({
                    "done": self.files_done,
                    "bytes": self.bytes_total,
                    "current": shown,
                }),
            );
        }
    }
}

/// Recover any mix of files, symlinks, and folders into a destination folder
/// chosen via a native dialog. Folders are walked recursively; top-level names
/// that already exist in the destination are uniquified ("name (2)") instead
/// of skipped or overwritten. Emits `recover-progress` events as it goes.
#[tauri::command(async)]
pub fn recover_batch(
    app: AppHandle,
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Value, String> {
    let (container, volume) = require_container(&state)?;
    if paths.is_empty() {
        return Err("nothing selected".into());
    }
    let vol = open_volume(&container, volume)?;
    if vol.apsb.is_encrypted() {
        return Err("volume is encrypted; refusing plaintext recovery".into());
    }

    let dest_dir = match app.dialog().file().blocking_pick_folder() {
        Some(fp) => fp.into_path().map_err(|e| e.to_string())?,
        None => return Ok(json!({ "saved": false, "cancelled": true })),
    };

    let bt = vol.btree();
    let mut ctx = BatchCtx::new(&app);

    for path in &paths {
        let dest = unique_dest(&dest_dir, &basename(path));
        let shown = dest.display().to_string();

        let fsoid = match apath::resolve(&vol, &bt, path) {
            Ok(r) => r.fsoid,
            Err(e) => {
                ctx.push(&shown, "error", 0, Some(e.to_string()));
                continue;
            }
        };
        let records = match vol.records(&bt, fsoid) {
            Ok(r) => r,
            Err(e) => {
                ctx.push(&shown, "error", 0, Some(e.to_string()));
                continue;
            }
        };
        let inode = match Volume::inode_from_records(&records) {
            Ok(Some(i)) => i,
            Ok(None) => {
                ctx.push(&shown, "error", 0, Some("no inode".into()));
                continue;
            }
            Err(e) => {
                ctx.push(&shown, "error", 0, Some(e.to_string()));
                continue;
            }
        };

        if inode.is_dir() {
            recover_tree(&vol, &bt, fsoid, &dest, 0, &mut ctx);
        } else {
            recover_one(&vol, &records, &inode, &dest, &mut ctx);
        }
    }

    let truncated = ctx.files_done as usize > ctx.results.len();
    Ok(json!({
        "saved": true,
        "dest": dest_dir.display().to_string(),
        "files_total": ctx.files_done,
        "bytes_total": ctx.bytes_total,
        "recovered": ctx.recovered,
        "partial": ctx.partial,
        "errors": ctx.errors,
        "results": ctx.results,
        "results_truncated": truncated,
    }))
}

/// Recover one non-directory inode to `dest`, recording the outcome.
fn recover_one(vol: &Volume, records: &[Record], inode: &Inode, dest: &Path, ctx: &mut BatchCtx) {
    let shown = dest.display().to_string();

    if inode.is_symlink() {
        match Volume::symlink_target(records) {
            Ok(Some(target)) => {
                let _ = fs::remove_file(dest);
                match std::os::unix::fs::symlink(&target, dest) {
                    Ok(()) => ctx.push(
                        &shown,
                        "recovered",
                        target.len() as u64,
                        Some(format!("symlink -> {target}")),
                    ),
                    Err(e) => ctx.push(&shown, "error", 0, Some(e.to_string())),
                }
            }
            Ok(None) => ctx.push(&shown, "error", 0, Some("symlink without target".into())),
            Err(e) => ctx.push(&shown, "error", 0, Some(e.to_string())),
        }
        return;
    }

    let size = inode.logical_size().unwrap_or(0);
    match write_atomic(vol, records, size, dest) {
        Ok(w) => {
            let status = if w.bytes == size {
                "recovered"
            } else {
                "partial"
            };
            ctx.push(&shown, status, w.bytes, w.note());
        }
        Err(e) => ctx.push(&shown, "error", 0, Some(e)),
    }
}

/// Recursively recover the directory `dir_fsoid` into `out_dir` (which is
/// created). Mirrors the CLI's folder walk: bounded depth, path-traversal
/// guard, best-effort per entry.
fn recover_tree(
    vol: &Volume,
    bt: &BtreeReader,
    dir_fsoid: u64,
    out_dir: &Path,
    depth: u32,
    ctx: &mut BatchCtx,
) {
    let shown = out_dir.display().to_string();
    if depth > MAX_DIR_DEPTH {
        ctx.push(
            &shown,
            "error",
            0,
            Some("max directory depth exceeded".into()),
        );
        return;
    }
    if let Err(e) = fs::create_dir_all(out_dir) {
        ctx.push(&shown, "error", 0, Some(e.to_string()));
        return;
    }
    let entries = match vol.list_dir(bt, dir_fsoid) {
        Ok(e) => e,
        Err(e) => {
            ctx.push(&shown, "error", 0, Some(e.to_string()));
            return;
        }
    };
    for entry in entries {
        // Path-traversal guard: reject dangerous names outright.
        if !is_safe_name(&entry.name) {
            ctx.push(
                &format!("{}/{}", out_dir.display(), entry.name),
                "error",
                0,
                Some("unsafe entry name; skipped".into()),
            );
            continue;
        }
        let child = out_dir.join(&entry.name);
        let child_shown = child.display().to_string();
        let records = match vol.records(bt, entry.file_id) {
            Ok(r) => r,
            Err(e) => {
                ctx.push(&child_shown, "error", 0, Some(e.to_string()));
                continue;
            }
        };
        let inode = match Volume::inode_from_records(&records) {
            Ok(Some(i)) => i,
            Ok(None) => {
                ctx.push(&child_shown, "error", 0, Some("no inode".into()));
                continue;
            }
            Err(e) => {
                ctx.push(&child_shown, "error", 0, Some(e.to_string()));
                continue;
            }
        };
        if inode.is_dir() {
            recover_tree(vol, bt, entry.file_id, &child, depth + 1, ctx);
        } else {
            recover_one(vol, &records, &inode, &child, ctx);
        }
    }
}

/// First free destination path for `name` in `dir`: the name itself, else
/// "stem (2).ext", "stem (3).ext", … (`symlink_metadata` so dangling symlinks
/// still count as taken).
fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2u32..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|cand| fs::symlink_metadata(cand).is_err())
        .expect("some suffixed name is free")
}

/// Reject empty/`.`/`..` names and names containing a path separator or NUL.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// Write `file_size` bytes to `final_path` via a temp file + atomic rename,
/// delegating the extent reconstruction to the shared core primitive.
fn write_atomic(
    vol: &Volume,
    records: &[Record],
    file_size: u64,
    final_path: &Path,
) -> Result<Written, String> {
    let dir = final_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "recovered".into());
    let tmp = dir.join(format!(
        "_com.apfsrelic.recover_{}_{}",
        std::process::id(),
        name
    ));

    let written = {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        let w = vol
            .write_file_data(records, file_size, &mut f)
            .map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())?;
        w
    };

    fs::rename(&tmp, final_path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(written)
}
