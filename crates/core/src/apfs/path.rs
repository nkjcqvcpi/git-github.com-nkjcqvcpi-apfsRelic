//! Path resolution within a volume or snapshot (rewrite plan Phase 12).
//!
//! Resolution walks directory records from the root inode, matching one path
//! component at a time. On case-insensitive volumes a bytewise match is tried
//! first and an ASCII/Unicode case-folded match second, so the lookup is *not*
//! purely bytewise (acceptance criterion). Full Unicode normalization-insensitive
//! matching is approximated by also comparing case-folded forms; this covers the
//! common cases without bundling a normalization table. The legacy Time Machine
//! `"/<snapshot>/.../path"` form needs no special handling here because those
//! components are ordinary nested directories.

use super::btree::BtreeReader;
use super::jrec::{DirRec, ROOT_DIR_INO_NUM};
use super::vol::Volume;
use crate::error::{Error, ErrorKind, Result};

const MAX_PATH_COMPONENTS: usize = 4096;

/// Outcome of resolving a path: the FSOID and its directory-entry type name.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub fsoid: u64,
    pub type_name: String,
}

/// Resolve `path` (absolute, `/`-separated) to an FSOID starting at the volume
/// root. Returns a structured error identifying *why* resolution failed.
pub fn resolve(vol: &Volume, bt: &BtreeReader, path: &str) -> Result<Resolved> {
    if !path.starts_with('/') {
        return Err(Error::new(
            ErrorKind::Usage,
            "path must be absolute (start with `/`)",
        ));
    }

    let mut fsoid = ROOT_DIR_INO_NUM;
    let mut type_name = String::from("dir");
    let case_insensitive =
        vol.apsb.is_case_insensitive() || vol.apsb.is_normalization_insensitive();

    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.len() > MAX_PATH_COMPONENTS {
        return Err(Error::new(ErrorKind::Usage, "path has too many components"));
    }

    for (i, component) in components.iter().enumerate() {
        let entries = vol.list_dir(bt, fsoid)?;
        let matched = match_entry(&entries, component, case_insensitive).ok_or_else(|| {
            Error::new(
                ErrorKind::PathNotFound,
                format!("no entry `{component}` under FSOID {fsoid:#x}"),
            )
        })?;

        let is_last = i + 1 == components.len();
        // An intermediate component must be a directory.
        if !is_last && matched.type_name() != "dir" {
            return Err(Error::new(
                ErrorKind::NotADirectory,
                format!(
                    "`{component}` is a {}, not a directory",
                    matched.type_name()
                ),
            ));
        }
        fsoid = matched.file_id;
        type_name = matched.type_name().to_string();
    }

    Ok(Resolved { fsoid, type_name })
}

/// Find the directory entry matching `name`, preferring an exact byte match and
/// falling back to a case-folded match on case-insensitive volumes.
fn match_entry<'a>(
    entries: &'a [DirRec],
    name: &str,
    case_insensitive: bool,
) -> Option<&'a DirRec> {
    if let Some(e) = entries.iter().find(|e| e.name == name) {
        return Some(e);
    }
    if case_insensitive {
        let folded = casefold(name);
        return entries.iter().find(|e| casefold(&e.name) == folded);
    }
    None
}

/// Lowercase a string for case-insensitive comparison (Unicode-aware via
/// `char::to_lowercase`).
fn casefold(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casefold_is_unicode_aware() {
        assert_eq!(casefold("ABCé"), "abcé");
        assert!(!casefold("İ").is_empty());
    }
}
