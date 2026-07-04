//! `inspect` — report image, checkpoint, container, and volume metadata
//! (rewrite plan Phases 2, 6, 22).

use crate::cli::Options;
use apfsrelic_core::apfs::nx::NX_SUPPORTED_INCOMPAT_MASK;
use apfsrelic_core::device::SparseBundleDevice;
use apfsrelic_core::error::Result;
use apfsrelic_core::json::{Envelope, Json};
use apfsrelic_core::source::ImageKind;

pub fn run(opts: &Options) -> Result<i32> {
    let opened = super::open(opts)?;
    let nx = &opened.container.nx;

    // Container summary.
    let container = Json::obj()
        .set("uuid", nx.uuid.as_str())
        .set("block_size", nx.block_size)
        .set("block_count", nx.block_count)
        .set("features", Json::hex(nx.features))
        .set(
            "readonly_compatible_features",
            Json::hex(nx.readonly_compatible_features),
        )
        .set("incompatible_features", Json::hex(nx.incompatible_features))
        .set(
            "supported_incompatible_mask",
            Json::hex(NX_SUPPORTED_INCOMPAT_MASK),
        )
        .set("has_keylocker", nx.has_keylocker())
        .set("max_file_systems", nx.max_file_systems)
        .set("checkpoint_descriptor_blocks", nx.xp_desc_blocks());

    // Volumes summary.
    let slots = opened.container.list_volumes()?;
    let volumes: Vec<Json> = slots
        .iter()
        .map(|s| {
            Json::obj()
                .set("index", s.index)
                .set("name", s.apsb.volname.as_str())
                .set("role", s.apsb.role_name())
                .set("encrypted", s.apsb.is_encrypted())
                .set("num_snapshots", s.apsb.num_snapshots)
        })
        .collect();

    // Sparsebundle band statistics.
    let mut sparsebundle = None;
    if opened.kind == ImageKind::SparseBundle {
        if let Ok(dev) = SparseBundleDevice::open(&super::require_container(opts)?) {
            let stats = dev.band_stats()?;
            let mut sb = Json::obj()
                .set("band_size", stats.band_size)
                .set("logical_size", stats.logical_size)
                .set("expected_bands", stats.expected_bands)
                .set("present_bands", stats.present_bands)
                .set(
                    "missing_band_count",
                    stats.expected_bands - stats.present_bands,
                )
                .set(
                    "missing_bands",
                    Json::Array(stats.missing_bands.iter().map(|&b| Json::UInt(b)).collect()),
                )
                .set("missing_bands_truncated", stats.missing_truncated)
                .set(
                    "short_bands",
                    Json::Array(stats.short_bands.iter().map(|&b| Json::UInt(b)).collect()),
                )
                .set("read_only", true);
            if let Some(u) = dev.uuid() {
                sb.insert("uuid", u);
            }
            if let Some(v) = dev.backingstore_version() {
                sb.insert("backingstore_version", v);
            }
            sparsebundle = Some(sb);
        }
    }

    if opts.json {
        let mut result = Json::obj()
            .set("container", container)
            .set("volumes", Json::Array(volumes));
        if let Some(sb) = sparsebundle {
            result.insert("sparsebundle", sb);
        }
        if opts.partitions {
            if let Some(table) = apfsrelic_core::source::read_partition_table(&*opened.image)? {
                let parts: Vec<Json> = table
                    .entries
                    .iter()
                    .map(|p| {
                        Json::obj()
                            .set("index", p.index)
                            .set("type_guid", p.type_guid.as_str())
                            .set("is_apfs", p.is_apfs)
                            .set("byte_offset", p.byte_offset())
                            .set("byte_len", p.byte_len())
                    })
                    .collect();
                result.insert("partitions", Json::Array(parts));
            }
        }
        let env = Envelope::new("inspect")
            .image(super::image_json(&opened))
            .checkpoint_xid(opened.container.checkpoint_xid)
            .result(result)
            .warnings(opened.container.warnings.clone());
        super::print_json(env.build());
    } else {
        println!("Image: {}", opened.image.description());
        println!("APFS container at offset {} bytes", opened.container_offset);
        println!(
            "Checkpoint XID {:#x} (descriptor index {})",
            opened.container.checkpoint_xid, opened.container.checkpoint_index
        );
        println!(
            "Container UUID {}  block_size {}  blocks {}",
            nx.uuid, nx.block_size, nx.block_count
        );
        for s in &slots {
            println!(
                "  volume {}: {} ({}, {} snapshots){}",
                s.index,
                s.apsb.volname,
                s.apsb.role_name(),
                s.apsb.num_snapshots,
                if s.apsb.is_encrypted() {
                    " [encrypted]"
                } else {
                    ""
                }
            );
        }
        for w in &opened.container.warnings {
            eprintln!("warning: {w}");
        }
    }
    Ok(0)
}
