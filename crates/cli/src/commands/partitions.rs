//! `partitions` — list GPT partitions of an image (rewrite plan Phase 3).

use crate::cli::Options;
use apfsrelic_core::error::Result;
use apfsrelic_core::json::{Envelope, Json};
use apfsrelic_core::source;

pub fn run(opts: &Options) -> Result<i32> {
    let path = super::require_container(opts)?;
    let (image, kind) = source::open_image(&path)?;
    let table = source::read_partition_table(&*image)?;

    let mut parts = Vec::new();
    if let Some(table) = &table {
        for p in &table.entries {
            parts.push(
                Json::obj()
                    .set("index", p.index)
                    .set("type_guid", p.type_guid.as_str())
                    .set("unique_guid", p.unique_guid.as_str())
                    .set("name", p.name.as_str())
                    .set("first_lba", p.first_lba)
                    .set("last_lba", p.last_lba)
                    .set("byte_offset", p.byte_offset())
                    .set("byte_len", p.byte_len())
                    .set("is_apfs", p.is_apfs),
            );
        }
    }

    if opts.json {
        let img = Json::obj()
            .set(
                "type",
                match kind {
                    source::ImageKind::SparseBundle => "sparsebundle",
                    source::ImageKind::RawFile => "raw",
                },
            )
            .set("description", image.description())
            .set("size", image.size())
            .set("has_gpt", table.is_some());
        let env = Envelope::new("partitions")
            .image(img)
            .result(Json::obj().set("partitions", Json::Array(parts)));
        super::print_json(env.build());
    } else if table.is_none() {
        println!("No GPT found; image is a bare APFS container (offset 0).");
    } else {
        let table = table.unwrap();
        println!("{} partition(s):", table.entries.len());
        for p in &table.entries {
            println!(
                "  {}: {} off={} len={} {}{}",
                p.index,
                p.type_guid,
                p.byte_offset(),
                p.byte_len(),
                if p.name.is_empty() { "" } else { &p.name },
                if p.is_apfs { " [APFS]" } else { "" }
            );
        }
    }
    Ok(0)
}
