//! `volumes` — list the volumes in a container (rewrite plan Phase 9).

use crate::cli::Options;
use apfsrelic_core::error::Result;
use apfsrelic_core::json::{Envelope, Json};

pub fn run(opts: &Options) -> Result<i32> {
    let opened = super::open(opts)?;
    let slots = opened.container.list_volumes()?;

    let mut entries = Vec::new();
    for slot in &slots {
        let a = &slot.apsb;
        entries.push(
            Json::obj()
                .set("index", slot.index)
                .set("name", a.volname.as_str())
                .set("role", a.role_name())
                .set("uuid", a.vol_uuid.as_str())
                .set("case_insensitive", a.is_case_insensitive())
                .set("encrypted", a.is_encrypted())
                .set("sealed", a.is_sealed())
                .set("num_files", a.num_files)
                .set("num_directories", a.num_directories)
                .set("num_snapshots", a.num_snapshots)
                .set("features", Json::hex(a.features))
                .set("incompatible_features", Json::hex(a.incompatible_features))
                .set("root_tree_oid", Json::hex(a.root_tree_oid)),
        );
    }

    if opts.json {
        let env = Envelope::new("volumes")
            .image(super::image_json(&opened))
            .checkpoint_xid(opened.container.checkpoint_xid)
            .result(Json::obj().set("volumes", Json::Array(entries)))
            .warnings(opened.container.warnings.clone());
        super::print_json(env.build());
    } else {
        println!("{} volume(s):", slots.len());
        for slot in &slots {
            let a = &slot.apsb;
            println!(
                "  {}: {:<28} role={:<8} files={:<8} snapshots={}{}",
                slot.index,
                a.volname,
                a.role_name(),
                a.num_files,
                a.num_snapshots,
                if a.is_encrypted() { " [encrypted]" } else { "" }
            );
        }
    }
    Ok(0)
}
