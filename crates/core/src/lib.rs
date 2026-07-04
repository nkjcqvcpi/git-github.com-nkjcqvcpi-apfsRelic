//! apfsRelic core — a read-only, non-`sudo` APFS image reader library.
//!
//! This crate is the dependency-free engine shared by the CLI
//! (`apfsrelic-cli`) and the desktop GUI (`apfsrelic-gui`). It is organized in
//! layers (see `AGENTS.md`):
//!  * [`device`] — the read-only [`device::BlockDevice`] abstraction and backends.
//!  * [`apfs`] — on-disk parsers and the container/volume engine.
//!  * [`json`] — the stable JSON envelope; [`error`] — the structured error model.
//!  * [`source`] — turn a container path + selectors into an opened image.

pub mod apfs;
pub mod device;
pub mod error;
pub mod json;
pub mod source;
