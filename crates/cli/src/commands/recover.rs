//! `recover` — recover files and folders by logical extent layout (rewrite plan
//! Phases 16, 17, 19, 24).
//!
//! File data is reconstructed from `FILE_EXTENT` records sorted by logical
//! address, zero-filling sparse holes and gaps, stopping exactly at the logical
//! file size. Output is written to a temporary file and atomically renamed; the
//! input image is never modified. Encrypted volumes are refused unless
//! `--raw-extents` is given. Folder recovery walks the directory tree, recreates
//! structure, preserves symlinks and hard links, refuses path traversal, and
//! reports per-file status.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::Options;
use apfsrelic_core::apfs::btree::{BtreeReader, Record};
use apfsrelic_core::apfs::extract::Written;
use apfsrelic_core::apfs::jrec::Inode;
use apfsrelic_core::apfs::path as apath;
use apfsrelic_core::apfs::vol::Volume;
use apfsrelic_core::error::{Error, ErrorKind, Result};
use apfsrelic_core::json::{Envelope, Json};

const MAX_DIR_DEPTH: u32 = 256;

/// Per-file recovery outcome.
struct FileResult {
    path: String,
    fsoid: u64,
    status: &'static str,
    bytes: u64,
    note: Option<String>,
}

impl FileResult {
    fn to_json(&self) -> Json {
        let mut o = Json::obj()
            .set("path", self.path.as_str())
            .set("fsoid", format!("{:#x}", self.fsoid))
            .set("status", self.status)
            .set("bytes", self.bytes);
        if let Some(n) = &self.note {
            o.insert("note", n.as_str());
        }
        o
    }
}

pub fn run(opts: &Options) -> Result<i32> {
    let opened = super::open(opts)?;
    let (vol, snap) = super::open_volume_view(&opened, opts)?;
    let index = super::resolve_volume_index(&opened, opts)?;
    let bt = vol.btree();

    let fsoid = if let Some(path) = &opts.path {
        apath::resolve(&vol, &bt, path)?.fsoid
    } else if let Some(fsoid) = opts.fsoid {
        fsoid
    } else {
        return Err(Error::new(
            ErrorKind::Usage,
            "`recover` needs `--path <p>` or `--fsoid <id>`",
        ));
    };

    // Refuse plaintext recovery of encrypted data (Phase 19).
    if vol.apsb.is_encrypted() && !opts.raw_extents {
        return Err(Error::new(
            ErrorKind::EncryptedUnsupported,
            "volume is encrypted; refusing plaintext recovery (use --raw-extents for a forensic dump)",
        ));
    }

    let records = vol.records(&bt, fsoid)?;
    let inode = Volume::inode_from_records(&records)?.ok_or_else(|| {
        Error::new(
            ErrorKind::ObjectNotFound,
            format!("no inode for FSOID {fsoid:#x}"),
        )
    })?;

    let mut results: Vec<FileResult> = Vec::new();
    let mut state = HardlinkState::default();

    let recover_type;
    if inode.is_dir() {
        recover_type = "folder";
        let out_dir = opts.output.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                "folder recovery requires `--output <dir>`",
            )
        })?;
        let out_dir = PathBuf::from(out_dir);
        recover_folder(
            &vol,
            &bt,
            fsoid,
            &inode,
            &out_dir,
            opts,
            0,
            &mut results,
            &mut state,
        )?;
    } else {
        recover_type = "file";
        let res = recover_single_file(&vol, &bt, fsoid, &inode, &records, opts)?;
        results.push(res);
    }

    let mut warnings = opened.container.warnings.clone();
    warnings.extend(vol.warnings.clone());
    warnings.extend(bt.take_warnings());

    let partial = results
        .iter()
        .any(|r| r.status == "partial" || r.status == "error");
    let total_bytes: u64 = results.iter().map(|r| r.bytes).sum();

    if opts.json {
        let result = Json::obj()
            .set("type", recover_type)
            .set("dry_run", opts.dry_run)
            .set("files_total", results.len())
            .set("bytes_total", total_bytes)
            .set(
                "files",
                Json::Array(results.iter().map(FileResult::to_json).collect()),
            );
        let mut env = Envelope::new("recover")
            .image(super::image_json(&opened))
            .volume(super::volume_json(&vol, index))
            .checkpoint_xid(opened.container.checkpoint_xid)
            .result(result)
            .warnings(warnings);
        if let Some(s) = &snap {
            env = env.snapshot(super::snapshot_json(s));
        }
        if partial {
            env = env.error(
                "partial-recovery",
                "one or more files were not fully recovered",
            );
        }
        super::print_json(env.build());
    } else {
        for r in &results {
            eprintln!(
                "{}  {}  {} bytes{}",
                r.status,
                r.path,
                r.bytes,
                r.note
                    .as_deref()
                    .map(|n| format!("  ({n})"))
                    .unwrap_or_default()
            );
        }
    }

    Ok(if partial {
        ErrorKind::PartialRecovery.exit_code()
    } else {
        0
    })
}

/// Recover one regular file (or symlink) to `--output` (or stdout).
fn recover_single_file(
    vol: &Volume,
    _bt: &BtreeReader,
    fsoid: u64,
    inode: &Inode,
    records: &[Record],
    opts: &Options,
) -> Result<FileResult> {
    let size = inode.logical_size().unwrap_or(0);

    // Symlink: write the link target rather than extents.
    if inode.is_symlink() {
        if let Some(target) = Volume::symlink_target(records)? {
            if let Some(out) = &opts.output {
                if opts.dry_run {
                    return Ok(FileResult {
                        path: out.clone(),
                        fsoid,
                        status: "dry-run",
                        bytes: target.len() as u64,
                        note: Some(format!("symlink -> {target}")),
                    });
                }
                let p = PathBuf::from(out);
                guard_overwrite(&p, opts)?;
                let _ = fs::remove_file(&p);
                std::os::unix::fs::symlink(&target, &p)?;
                return Ok(FileResult {
                    path: out.clone(),
                    fsoid,
                    status: "recovered",
                    bytes: target.len() as u64,
                    note: Some(format!("symlink -> {target}")),
                });
            }
        }
    }

    let out_desc = opts.output.clone().unwrap_or_else(|| "-".into());
    if opts.dry_run {
        return Ok(FileResult {
            path: out_desc,
            fsoid,
            status: "dry-run",
            bytes: size,
            note: None,
        });
    }

    match &opts.output {
        None => {
            // Stream to stdout.
            let stdout = io::stdout();
            let mut w = stdout.lock();
            let written = vol.write_file_data(records, size, &mut w)?;
            let status = if written.bytes == size {
                "recovered"
            } else {
                "partial"
            };
            Ok(FileResult {
                path: out_desc,
                fsoid,
                status,
                bytes: written.bytes,
                note: written.note(),
            })
        }
        Some(out) => {
            let final_path = PathBuf::from(out);
            guard_overwrite(&final_path, opts)?;
            let written = write_file_atomic(vol, records, size, &final_path)?;
            let status = if written.bytes == size {
                "recovered"
            } else {
                "partial"
            };
            Ok(FileResult {
                path: out.clone(),
                fsoid,
                status,
                bytes: written.bytes,
                note: written.note(),
            })
        }
    }
}

/// Recursively recover a directory tree.
#[allow(clippy::too_many_arguments)]
fn recover_folder(
    vol: &Volume,
    bt: &BtreeReader,
    dir_fsoid: u64,
    _dir_inode: &Inode,
    out_dir: &Path,
    opts: &Options,
    depth: u32,
    results: &mut Vec<FileResult>,
    state: &mut HardlinkState,
) -> Result<()> {
    if depth > MAX_DIR_DEPTH {
        results.push(FileResult {
            path: out_dir.display().to_string(),
            fsoid: dir_fsoid,
            status: "error",
            bytes: 0,
            note: Some("max directory depth exceeded".into()),
        });
        return Ok(());
    }

    if !opts.dry_run {
        fs::create_dir_all(out_dir)?;
    }

    let entries = vol.list_dir(bt, dir_fsoid)?;
    for entry in entries {
        // Path-traversal guard: reject dangerous names outright.
        if !is_safe_name(&entry.name) {
            results.push(FileResult {
                path: format!("{}/{}", out_dir.display(), entry.name),
                fsoid: entry.file_id,
                status: "error",
                bytes: 0,
                note: Some("unsafe entry name; skipped".into()),
            });
            continue;
        }
        let child_path = out_dir.join(&entry.name);
        let child_records = vol.records(bt, entry.file_id)?;
        let child_inode = match Volume::inode_from_records(&child_records)? {
            Some(i) => i,
            None => {
                results.push(FileResult {
                    path: child_path.display().to_string(),
                    fsoid: entry.file_id,
                    status: "error",
                    bytes: 0,
                    note: Some("no inode".into()),
                });
                continue;
            }
        };

        if child_inode.is_dir() {
            recover_folder(
                vol,
                bt,
                entry.file_id,
                &child_inode,
                &child_path,
                opts,
                depth + 1,
                results,
                state,
            )?;
            continue;
        }

        // Hard link: if we've already recovered this inode, link instead.
        if child_inode.nchildren_or_nlink > 1 {
            if let Some(first) = state.get(entry.file_id) {
                if !opts.dry_run {
                    let _ = fs::remove_file(&child_path);
                    if let Err(e) = fs::hard_link(first, &child_path) {
                        results.push(FileResult {
                            path: child_path.display().to_string(),
                            fsoid: entry.file_id,
                            status: "error",
                            bytes: 0,
                            note: Some(format!("hardlink failed: {e}")),
                        });
                        continue;
                    }
                }
                results.push(FileResult {
                    path: child_path.display().to_string(),
                    fsoid: entry.file_id,
                    status: "recovered",
                    bytes: 0,
                    note: Some("hardlink".into()),
                });
                continue;
            }
        }

        // Resume support: skip an existing file unless --overwrite.
        if !opts.overwrite && !opts.dry_run && child_path.exists() {
            results.push(FileResult {
                path: child_path.display().to_string(),
                fsoid: entry.file_id,
                status: "skipped-exists",
                bytes: 0,
                note: None,
            });
            continue;
        }

        let file_opts = Options {
            output: Some(child_path.display().to_string()),
            ..opts.clone()
        };
        match recover_single_file(
            vol,
            bt,
            entry.file_id,
            &child_inode,
            &child_records,
            &file_opts,
        ) {
            Ok(mut r) => {
                if child_inode.nchildren_or_nlink > 1 && !opts.dry_run {
                    state.insert(entry.file_id, child_path.clone());
                }
                r.path = child_path.display().to_string();
                results.push(r);
            }
            Err(e) => {
                if !opts.best_effort {
                    return Err(e);
                }
                results.push(FileResult {
                    path: child_path.display().to_string(),
                    fsoid: entry.file_id,
                    status: "error",
                    bytes: 0,
                    note: Some(e.to_string()),
                });
            }
        }
    }
    Ok(())
}

/// Tracks the first on-disk path recovered for each multi-linked inode.
#[derive(Default)]
struct HardlinkState {
    map: std::collections::HashMap<u64, PathBuf>,
}
impl HardlinkState {
    fn get(&self, oid: u64) -> Option<&PathBuf> {
        self.map.get(&oid)
    }
    fn insert(&mut self, oid: u64, path: PathBuf) {
        self.map.entry(oid).or_insert(path);
    }
}

/// Write `file_size` bytes to `final_path` via a temp file + atomic rename.
fn write_file_atomic(
    vol: &Volume,
    records: &[Record],
    file_size: u64,
    final_path: &Path,
) -> Result<Written> {
    let dir = final_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
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
            .open(&tmp)?;
        let w = vol.write_file_data(records, file_size, &mut f)?;
        f.flush()?;
        w
    };

    fs::rename(&tmp, final_path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(written)
}

/// Reject empty/`.`/`..` names and names containing a path separator or NUL.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// Refuse to overwrite an existing final path unless `--overwrite`.
fn guard_overwrite(path: &Path, opts: &Options) -> Result<()> {
    if path.exists() && !opts.overwrite {
        return Err(Error::new(
            ErrorKind::Usage,
            format!(
                "`{}` already exists; pass --overwrite to replace it",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_name_rejects_traversal() {
        assert!(is_safe_name("file.txt"));
        assert!(!is_safe_name(".."));
        assert!(!is_safe_name("a/b"));
        assert!(!is_safe_name(""));
    }
}
