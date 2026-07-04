//! Feature-flag compatibility checking (rewrite plan Phase 10).
//!
//! APFS objects carry three feature words: optional features, read-only
//! compatible features, and incompatible features. For a *read-only* tool the
//! critical rule is: an **unknown incompatible** feature means the on-disk
//! format may differ in ways we don't understand, so we must refuse rather than
//! risk silently misinterpreting data. Unknown read-only-compatible / optional
//! features are safe to ignore for browsing. This module produces a structured
//! report plus a hard error when an unknown incompatible bit is set.

use super::nx::{NxSuperblock, NX_SUPPORTED_INCOMPAT_MASK};
use super::volume::{
    ApfsSuperblock, APFS_INCOMPAT_DATALESS_SNAPS, APFS_INCOMPAT_SEALED_VOLUME,
    APFS_SUPPORTED_INCOMPAT_MASK,
};
use crate::error::{unsupported, Result};

/// How well a feature is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Fully understood and interpreted.
    Supported,
    /// Recognized but only safe for metadata browsing (not full fidelity).
    BrowseOnly,
    /// Recognized and incompatible — recovery/browse may be wrong; refuse.
    UnsupportedIncompatible,
    /// Unknown incompatible bit — refuse.
    UnknownIncompatible,
}

/// One classified feature bit.
#[derive(Debug, Clone)]
pub struct FeatureNote {
    pub name: String,
    pub support: Support,
}

/// The result of classifying a container's and volume's features.
#[derive(Debug, Clone, Default)]
pub struct FeatureReport {
    pub notes: Vec<FeatureNote>,
    pub warnings: Vec<String>,
}

impl FeatureReport {
    fn note(&mut self, name: &str, support: Support) {
        self.notes.push(FeatureNote {
            name: name.to_string(),
            support,
        });
    }
}

/// Classify a container superblock's incompatible features. Returns an error if
/// an unknown incompatible bit is set (fail-safe).
pub fn check_container(nx: &NxSuperblock) -> Result<FeatureReport> {
    let mut report = FeatureReport::default();

    if nx.incompatible_features & super::nx::NX_INCOMPAT_FUSION != 0 {
        // Fusion (tiered) containers are a documented non-goal.
        report.note("nx_incompat_fusion", Support::UnsupportedIncompatible);
        report
            .warnings
            .push("Fusion container detected; tier-2 data is not resolved.".into());
    }

    let unknown = nx.incompatible_features & !NX_SUPPORTED_INCOMPAT_MASK;
    if unknown != 0 {
        return Err(unsupported(format!(
            "container has unknown incompatible feature bits {unknown:#x}; refusing to read"
        )));
    }
    Ok(report)
}

/// Classify a volume superblock's incompatible features.
pub fn check_volume(apsb: &ApfsSuperblock) -> Result<FeatureReport> {
    let mut report = FeatureReport::default();

    if apsb.is_case_insensitive() {
        report.note("incompat_case_insensitive", Support::Supported);
    }
    if apsb.is_normalization_insensitive() {
        report.note("incompat_normalization_insensitive", Support::Supported);
    }
    if apsb.incompatible_features & APFS_INCOMPAT_DATALESS_SNAPS != 0 {
        report.note("incompat_dataless_snaps", Support::BrowseOnly);
        report
            .warnings
            .push("Volume has dataless snapshots; some file data may be absent.".into());
    }
    if apsb.incompatible_features & APFS_INCOMPAT_SEALED_VOLUME != 0 {
        report.note("incompat_sealed_volume", Support::BrowseOnly);
        report.warnings.push(
            "Sealed volume: seal/integrity validation is not performed (browse only).".into(),
        );
    }
    if apsb.is_encrypted() {
        report.note("volume_encrypted", Support::BrowseOnly);
        report
            .warnings
            .push("Volume is encrypted; file data cannot be decrypted (use --raw-extents).".into());
    }

    let unknown = apsb.incompatible_features & !APFS_SUPPORTED_INCOMPAT_MASK;
    if unknown != 0 {
        return Err(unsupported(format!(
            "volume `{}` has unknown incompatible feature bits {unknown:#x}; refusing to read",
            apsb.volname
        )));
    }
    Ok(report)
}
