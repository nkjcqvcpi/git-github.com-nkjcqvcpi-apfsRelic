//! `stat` — show all known metadata for a file or directory (rewrite plan
//! Phase 15), with optional raw records, extents, and xattrs.

use crate::cli::Options;
use apfsrelic_core::apfs::btree::split_obj_id_and_type;
use apfsrelic_core::apfs::jrec::{self, FileExtent, Inode, Xattr};
use apfsrelic_core::apfs::path as apath;
use apfsrelic_core::apfs::raw;
use apfsrelic_core::apfs::time;
use apfsrelic_core::error::{Error, ErrorKind, Result};
use apfsrelic_core::json::{Envelope, Json};

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
            "`stat` needs `--path <p>` or `--fsoid <id>`",
        ));
    };

    let records = vol.records(&bt, fsoid)?;
    if records.is_empty() {
        return Err(Error::new(
            ErrorKind::ObjectNotFound,
            format!("no records for FSOID {fsoid:#x}"),
        ));
    }

    // Inode metadata.
    let inode = apfsrelic_core::apfs::vol::Volume::inode_from_records(&records)?;
    let mut result = Json::obj().set("fsoid", format!("{fsoid:#x}"));
    if let Some(i) = &inode {
        result.insert("inode", inode_json(i));
        result.insert("recoverability", recoverability(i, &vol, &records)?);
    } else {
        result.insert("inode", Json::Null);
    }

    // Extents.
    if opts.extents {
        let mut extents = Vec::new();
        let mut total = 0u64;
        for rec in &records {
            let (_oid, ty) = split_obj_id_and_type(raw::u64_at(&rec.key, 0)?);
            if ty == jrec::APFS_TYPE_FILE_EXTENT {
                let fe = FileExtent::parse(&rec.key, &rec.val)?;
                total += fe.len;
                extents.push(
                    Json::obj()
                        .set("logical_addr", fe.logical_addr)
                        .set("len", fe.len)
                        .set("phys_block_num", fe.phys_block_num)
                        .set("hole", fe.is_hole()),
                );
            }
        }
        result.insert("extents", Json::Array(extents));
        result.insert("extents_total_bytes", total);
    }

    // Xattrs.
    if opts.xattrs {
        let mut xattrs = Vec::new();
        for rec in &records {
            let (_oid, ty) = split_obj_id_and_type(raw::u64_at(&rec.key, 0)?);
            if ty == jrec::APFS_TYPE_XATTR {
                let x = Xattr::parse(&rec.key, &rec.val)?;
                xattrs.push(
                    Json::obj()
                        .set("name", x.name.as_str())
                        .set("embedded", x.is_embedded())
                        .set("stream", x.is_stream())
                        .set("data_len", x.data.len()),
                );
            }
        }
        result.insert("xattrs", Json::Array(xattrs));
    }

    // Raw records.
    if opts.records {
        let mut raws = Vec::new();
        for rec in &records {
            let (oid, ty) = split_obj_id_and_type(raw::u64_at(&rec.key, 0)?);
            raws.push(
                Json::obj()
                    .set("obj_id", format!("{oid:#x}"))
                    .set("type", jrec_type_name(ty))
                    .set("key_len", rec.key.len())
                    .set("val_len", rec.val.len())
                    .set("key_hex", hex(&rec.key)),
            );
        }
        result.insert("records", Json::Array(raws));
    }

    let mut warnings = opened.container.warnings.clone();
    warnings.extend(vol.warnings.clone());
    warnings.extend(bt.take_warnings());

    if opts.json {
        let mut env = Envelope::new("stat")
            .image(super::image_json(&opened))
            .volume(super::volume_json(&vol, index))
            .checkpoint_xid(opened.container.checkpoint_xid)
            .result(result)
            .warnings(warnings);
        if let Some(s) = &snap {
            env = env.snapshot(super::snapshot_json(s));
        }
        super::print_json(env.build());
    } else {
        println!("FSOID {fsoid:#x}");
        if let Some(i) = &inode {
            println!("  mode {:#o} uid {} gid {}", i.mode, i.owner, i.group);
            if let Some(sz) = i.logical_size() {
                println!("  size {sz} bytes");
            }
            if let Some(t) = time::iso8601(i.mod_time) {
                println!("  modified {t}");
            }
        }
    }
    Ok(0)
}

fn inode_json(i: &Inode) -> Json {
    let mut o = Json::obj()
        .set("parent_id", i.parent_id)
        .set("private_id", i.private_id)
        .set("mode", Json::UInt(i.mode as u64))
        .set("uid", i.owner)
        .set("gid", i.group)
        .set("internal_flags", Json::hex(i.internal_flags))
        .set("bsd_flags", Json::hex(i.bsd_flags as u64))
        .set("nchildren_or_nlink", Json::Int(i.nchildren_or_nlink as i64))
        .set("is_dir", i.is_dir())
        .set("is_symlink", i.is_symlink())
        .set("is_regular", i.is_regular())
        .set("sparse", i.is_sparse())
        .set("has_rsrc_fork", i.has_rsrc_fork())
        .set("has_finder_info", i.has_finder_info());
    if let Some(sz) = i.logical_size() {
        o.insert("size", sz);
    }
    if let Some(a) = i.allocated_size() {
        o.insert("allocated_size", a);
    }
    if let Some(name) = i.name() {
        o.insert("name", name);
    }
    for (k, raw) in [
        ("create_time", i.create_time),
        ("mod_time", i.mod_time),
        ("change_time", i.change_time),
        ("access_time", i.access_time),
    ] {
        if let Some(t) = time::iso8601(raw) {
            o.insert(k, t);
            o.insert(&format!("{k}_raw"), raw);
        }
    }
    o
}

/// Explain whether (and why) the object is recoverable.
fn recoverability(
    inode: &Inode,
    vol: &apfsrelic_core::apfs::vol::Volume,
    records: &[apfsrelic_core::apfs::btree::Record],
) -> Result<Json> {
    if vol.apsb.is_encrypted() {
        return Ok(Json::obj()
            .set("status", "encrypted")
            .set("reason", "volume is encrypted; data cannot be decrypted"));
    }
    if inode.is_dir() {
        return Ok(Json::obj()
            .set("status", "ok")
            .set("reason", "directory (recurse to recover contents)"));
    }
    if inode.is_symlink() {
        return Ok(Json::obj().set("status", "ok").set("reason", "symlink"));
    }
    let size = inode.logical_size().unwrap_or(0);
    let mut has_extent = false;
    let mut has_hole = false;
    for rec in records {
        let (_oid, ty) = split_obj_id_and_type(raw::u64_at(&rec.key, 0)?);
        if ty == jrec::APFS_TYPE_FILE_EXTENT {
            let fe = FileExtent::parse(&rec.key, &rec.val)?;
            has_extent = true;
            if fe.is_hole() {
                has_hole = true;
            }
        }
    }
    let status = if size == 0 {
        "ok"
    } else if has_extent {
        if has_hole {
            "ok-sparse"
        } else {
            "ok"
        }
    } else {
        "no-extents"
    };
    Ok(Json::obj()
        .set("status", status)
        .set("size", size)
        .set("has_extents", has_extent))
}

fn jrec_type_name(ty: u8) -> &'static str {
    match ty {
        jrec::APFS_TYPE_SNAP_METADATA => "snap_metadata",
        jrec::APFS_TYPE_EXTENT => "extent",
        jrec::APFS_TYPE_INODE => "inode",
        jrec::APFS_TYPE_XATTR => "xattr",
        jrec::APFS_TYPE_SIBLING_LINK => "sibling_link",
        jrec::APFS_TYPE_DSTREAM_ID => "dstream_id",
        jrec::APFS_TYPE_CRYPTO_STATE => "crypto_state",
        jrec::APFS_TYPE_FILE_EXTENT => "file_extent",
        jrec::APFS_TYPE_DIR_REC => "dir_rec",
        jrec::APFS_TYPE_DIR_STATS => "dir_stats",
        jrec::APFS_TYPE_SNAP_NAME => "snap_name",
        jrec::APFS_TYPE_SIBLING_MAP => "sibling_map",
        jrec::APFS_TYPE_FILE_INFO => "file_info",
        _ => "unknown",
    }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}
