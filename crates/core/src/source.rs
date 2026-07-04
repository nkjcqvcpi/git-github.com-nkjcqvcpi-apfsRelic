//! Image opening: turn a `--container` path (+ partition/offset selectors) into
//! an opened [`Container`], choosing the backend and APFS container window.
//!
//! Backend selection is by inspection: a directory that looks like a
//! `.sparsebundle` uses the sparsebundle backend; anything else is a raw file or
//! device. The APFS container window is then located via `--offset`, GPT
//! auto-detection, or an explicit `--partition` (rewrite plan Phases 1-3).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::apfs::container::Container;
use crate::device::partition::{resolve_container_window, PartitionSelector, PartitionTable};
use crate::device::{BlockDevice, RawFileDevice, SparseBundleDevice};
use crate::error::Result;

/// What kind of backend an image path resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    SparseBundle,
    RawFile,
}

/// Options controlling how an image is opened.
pub struct OpenOptions {
    pub container_path: PathBuf,
    pub partition: PartitionSelector,
    pub offset: Option<u64>,
    pub max_xid: u64,
}

/// A fully-opened image: the raw backing device, the APFS container window, and
/// the opened container at a selected checkpoint.
pub struct OpenedImage {
    /// The raw backing device (whole sparsebundle/file).
    pub image: Arc<dyn BlockDevice>,
    pub kind: ImageKind,
    /// Byte offset of the APFS container within the image.
    pub container_offset: u64,
    /// 1-based GPT partition index, if one was selected.
    pub partition_index: Option<u32>,
    pub container: Container,
}

/// Open just the backing device (no container parsing) — used by `inspect
/// --partitions` and sparsebundle stat reporting.
pub fn open_image(path: &Path) -> Result<(Arc<dyn BlockDevice>, ImageKind)> {
    if SparseBundleDevice::looks_like(path) {
        let dev = SparseBundleDevice::open(path)?;
        Ok((Arc::new(dev), ImageKind::SparseBundle))
    } else {
        let dev = RawFileDevice::open(path)?;
        Ok((Arc::new(dev), ImageKind::RawFile))
    }
}

/// Open the partition table of an image, if any (for `partitions`/`inspect`).
pub fn read_partition_table(image: &dyn BlockDevice) -> Result<Option<PartitionTable>> {
    PartitionTable::parse(image)
}

/// Open the container described by `opts`.
pub fn open(opts: &OpenOptions) -> Result<OpenedImage> {
    let (image, kind) = open_image(&opts.container_path)?;

    let (offset, len, partition_index) =
        resolve_container_window(&*image, opts.partition, opts.offset)?;

    // Avoid an extra indirection when the container spans the whole image.
    let container_dev: Arc<dyn BlockDevice> = if offset == 0 && len == image.size() {
        Arc::clone(&image)
    } else {
        Arc::new(crate::device::OffsetDevice::new(
            Arc::clone(&image),
            offset,
            len,
        ))
    };

    let container = Container::open(container_dev, opts.max_xid)?;

    Ok(OpenedImage {
        image,
        kind,
        container_offset: offset,
        partition_index,
        container,
    })
}
