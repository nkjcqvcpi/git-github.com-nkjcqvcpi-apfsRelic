//! `ls` — machine-readable directory listing (rewrite plan Phase 14).
//!
//! The `--json` form keeps the legacy GUI-compatible shape: `entries` and
//! `fsoid` appear at the top level (so existing consumers that read
//! `data["entries"]` keep working), alongside the stable envelope fields
//! (`schema_version`, `command`, `image`, `volume`, `checkpoint_xid`,
//! `warnings`). Per-entry rich metadata (sizes, times, mode, flags) is filled in
//! when `--sizes` is given, which already implies a per-entry inode lookup.

use crate::cli::Options;
use apfsrelic_core::apfs::jrec::Inode;
use apfsrelic_core::apfs::path as apath;
use apfsrelic_core::apfs::time;
use apfsrelic_core::apfs::vol::Volume;
use apfsrelic_core::error::{Error, ErrorKind, Result};
use apfsrelic_core::json::{Json, SCHEMA_VERSION};

pub fn run(opts: &Options) -> Result<i32> {
    let opened = super::open(opts)?;
    let (vol, snap) = super::open_volume_view(&opened, opts)?;
    let index = super::resolve_volume_index(&opened, opts)?;
    let bt = vol.btree();

    // Determine the directory FSOID and its base path.
    let (dir_fsoid, base_path) = if let Some(path) = &opts.path {
        let r = apath::resolve(&vol, &bt, path)?;
        (r.fsoid, path.clone())
    } else if let Some(fsoid) = opts.fsoid {
        (fsoid, String::new())
    } else {
        return Err(Error::new(
            ErrorKind::Usage,
            "`ls` needs `--path <dir>` or `--fsoid <id>`",
        ));
    };

    let mut dir_entries = vol.list_dir(&bt, dir_fsoid)?;

    // Sort: APFS order by default, by name with `--sort name`.
    if opts.sort.as_deref() == Some("name") {
        dir_entries.sort_by(|a, b| a.name.cmp(&b.name));
    }

    let mut entries = Vec::with_capacity(dir_entries.len());
    for d in &dir_entries {
        let mut e = Json::obj()
            .set("name", d.name.as_str())
            .set("fsoid", format!("{:#x}", d.file_id))
            .set("type", d.type_name());
        if !base_path.is_empty() {
            e.insert("path", join_path(&base_path, &d.name));
        }
        e.insert("parent_fsoid", format!("{dir_fsoid:#x}"));

        if opts.sizes {
            if let Some(inode) = vol.inode(&bt, d.file_id)? {
                add_inode_fields(&mut e, &inode, &vol);
            }
            // Recoverability hint.
            let recoverable = if vol.apsb.is_encrypted() {
                "encrypted"
            } else if d.type_name() == "file" || d.type_name() == "dir" {
                "ok"
            } else {
                "metadata-only"
            };
            e.insert("recoverable", recoverable);
        }
        entries.push(e);
    }

    let mut warnings = opened.container.warnings.clone();
    warnings.extend(vol.warnings.clone());
    warnings.extend(bt.take_warnings());

    if opts.json {
        // Top-level shape preserves legacy `fsoid`/`entries`; envelope fields are
        // added alongside.
        let mut o = Json::obj()
            .set("schema_version", SCHEMA_VERSION as u64)
            .set("command", "ls")
            .set("image", super::image_json(&opened))
            .set("volume", super::volume_json(&vol, index))
            .set("checkpoint_xid", opened.container.checkpoint_xid);
        if let Some(s) = &snap {
            o.insert("snapshot", super::snapshot_json(s));
        }
        o.insert("fsoid", format!("{dir_fsoid:#x}"));
        o.insert("entries", Json::Array(entries));
        o.insert(
            "warnings",
            Json::Array(warnings.into_iter().map(Json::Str).collect()),
        );
        super::print_json(o);
    } else {
        for d in &dir_entries {
            if opts.sizes && d.type_name() == "file" {
                let size = vol
                    .inode(&bt, d.file_id)?
                    .and_then(|i| i.logical_size())
                    .unwrap_or(0);
                println!(
                    "{:<9}  {:>#12x}  {}  ({} bytes)",
                    d.type_name(),
                    d.file_id,
                    d.name,
                    size
                );
            } else {
                println!("{:<9}  {:>#12x}  {}", d.type_name(), d.file_id, d.name);
            }
        }
    }
    Ok(0)
}

/// Add inode-derived fields to an entry object.
fn add_inode_fields(e: &mut Json, inode: &Inode, vol: &Volume) {
    if let Some(size) = inode.logical_size() {
        e.insert("size", size);
    }
    if let Some(a) = inode.allocated_size() {
        e.insert("allocated_size", a);
    }
    e.insert("mode", Json::UInt(inode.mode as u64));
    e.insert("uid", inode.owner);
    e.insert("gid", inode.group);
    e.insert("flags", Json::hex(inode.internal_flags));
    e.insert("bsd_flags", Json::hex(inode.bsd_flags as u64));
    e.insert("link_count", Json::Int(inode.nchildren_or_nlink as i64));
    e.insert("has_xattrs", !inode.xfields.is_empty());
    e.insert("has_rsrc_fork", inode.has_rsrc_fork());
    e.insert("sparse", inode.is_sparse());
    e.insert("encrypted", vol.apsb.is_encrypted());
    insert_time(e, "birth_time", inode.create_time);
    insert_time(e, "modified_time", inode.mod_time);
    insert_time(e, "changed_time", inode.change_time);
    insert_time(e, "accessed_time", inode.access_time);
}

fn insert_time(e: &mut Json, key: &str, raw: u64) {
    if let Some(t) = time::iso8601(raw) {
        e.insert(key, t);
        e.insert(&format!("{key}_raw"), raw);
    }
}

/// Join a base directory path and a child name, normalizing the separator.
fn join_path(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}
