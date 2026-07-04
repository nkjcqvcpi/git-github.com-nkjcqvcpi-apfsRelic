//! `snapshots` — list a volume's snapshots (rewrite plan Phase 13).

use crate::cli::Options;
use apfsrelic_core::apfs::snapshot;
use apfsrelic_core::apfs::time;
use apfsrelic_core::apfs::vol::Volume;
use apfsrelic_core::error::Result;
use apfsrelic_core::json::{Envelope, Json};

pub fn run(opts: &Options) -> Result<i32> {
    let opened = super::open(opts)?;
    let index = super::resolve_volume_index(&opened, opts)?;
    let vol = Volume::open(&opened.container, index)?;
    let bt = vol.btree();
    let snaps = snapshot::list_snapshots(&vol, &bt)?;

    let mut warnings = opened.container.warnings.clone();
    warnings.extend(vol.warnings.clone());
    warnings.extend(bt.take_warnings());

    let mut entries = Vec::new();
    for s in &snaps {
        let mut e = Json::obj()
            .set("name", s.name.as_str())
            .set("xid", s.xid)
            .set("sblock_oid", Json::hex(s.sblock_oid));
        if let Some(t) = time::iso8601(s.create_time) {
            e.insert("create_time", t);
        }
        if let Some(t) = time::iso8601(s.change_time) {
            e.insert("change_time", t);
        }
        e.insert("create_time_raw", s.create_time);
        entries.push(e);
    }

    if opts.json {
        let env = Envelope::new("snapshots")
            .image(super::image_json(&opened))
            .volume(super::volume_json(&vol, index))
            .checkpoint_xid(opened.container.checkpoint_xid)
            .result(Json::obj().set("snapshots", Json::Array(entries)))
            .warnings(warnings);
        super::print_json(env.build());
    } else {
        println!("{} snapshot(s) on volume {}:", snaps.len(), index);
        for s in &snaps {
            let when = time::iso8601(s.create_time).unwrap_or_else(|| "-".into());
            println!("  xid {:#x}  {}  {}", s.xid, when, s.name);
        }
    }
    Ok(0)
}
