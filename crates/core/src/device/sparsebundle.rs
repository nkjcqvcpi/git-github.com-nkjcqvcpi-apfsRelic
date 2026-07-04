//! `SparseBundleDevice` — read a macOS `.sparsebundle` (e.g. a Time Machine
//! backup) directly, without `hdiutil attach`, mounting, or root (rewrite plan
//! Phase 2).
//!
//! A sparsebundle is a directory of fixed-size "band" files that together form a
//! virtual disk. Band *N* holds logical bytes `[N*band_size, (N+1)*band_size)`.
//! Band file names are the **lowercase hexadecimal** of the band index (this
//! matches the C prototype's `%PRIx64` and was confirmed against a real TM
//! bundle whose highest band is `2979`). Absent bands are sparse holes that read
//! as zeros; a band file shorter than `band_size` zero-fills its tail. The
//! bundle is opened and read strictly read-only and never modified.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{check_range, BlockDevice};
use crate::error::{Error, ErrorKind, Result};

/// Read-only sparsebundle backend.
pub struct SparseBundleDevice {
    bands_dir: PathBuf,
    band_size: u64,
    logical_size: u64,
    uuid: Option<String>,
    backingstore_version: Option<u64>,
    /// One-entry-per-band open-file cache (LRU of size 1, like the prototype,
    /// but keyed so reads that bounce between two bands still make progress).
    cache: Mutex<HashMap<u64, File>>,
    description: String,
}

/// Parsed `Info.plist` metadata.
struct InfoPlist {
    band_size: u64,
    size: u64,
    uuid: Option<String>,
    backingstore_version: Option<u64>,
}

/// Summary of band coverage, for `inspect`.
#[derive(Debug, Clone)]
pub struct BandStats {
    pub band_size: u64,
    pub logical_size: u64,
    pub expected_bands: u64,
    pub present_bands: u64,
    /// Indices in `[0, expected_bands)` with no band file (sparse holes). Capped.
    pub missing_bands: Vec<u64>,
    /// Whether `missing_bands` was truncated because there were too many to list.
    pub missing_truncated: bool,
    /// Present band files shorter than `band_size` (excluding the final band).
    pub short_bands: Vec<u64>,
}

const MAX_LISTED_MISSING: usize = 64;

impl SparseBundleDevice {
    /// Returns `true` if `path` looks like a sparsebundle (a directory with an
    /// `Info.plist` and a `bands` subdirectory).
    pub fn looks_like(path: &Path) -> bool {
        path.is_dir() && path.join("Info.plist").is_file() && path.join("bands").is_dir()
    }

    /// Open the sparsebundle at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_dir() {
            return Err(Error::new(
                ErrorKind::UnsupportedFormat,
                format!("`{}` is not a sparsebundle directory", path.display()),
            ));
        }
        let info = parse_info_plist(&path.join("Info.plist"))?;
        if info.band_size == 0 {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "sparsebundle Info.plist has zero band-size",
            ));
        }
        let bands_dir = path.join("bands");
        if !bands_dir.is_dir() {
            return Err(Error::new(
                ErrorKind::UnsupportedFormat,
                "sparsebundle has no `bands` directory",
            ));
        }
        let description = format!(
            "sparsebundle `{}` (band-size {} bytes, size {} bytes)",
            path.display(),
            info.band_size,
            info.size
        );
        Ok(SparseBundleDevice {
            bands_dir,
            band_size: info.band_size,
            logical_size: info.size,
            uuid: info.uuid,
            backingstore_version: info.backingstore_version,
            cache: Mutex::new(HashMap::new()),
            description,
        })
    }

    pub fn band_size(&self) -> u64 {
        self.band_size
    }
    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }
    pub fn backingstore_version(&self) -> Option<u64> {
        self.backingstore_version
    }

    /// Path of the band file for index `band` (lowercase hex name).
    fn band_path(&self, band: u64) -> PathBuf {
        self.bands_dir.join(format!("{band:x}"))
    }

    /// Read up to `buf.len()` bytes from band `band` starting at `within`,
    /// zero-filling for an absent band or a short tail. Always fills `buf`.
    fn read_band(&self, band: u64, within: u64, buf: &mut [u8]) -> Result<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "sparsebundle cache mutex poisoned"))?;

        if !cache.contains_key(&band) {
            // Bound the cache so a wild read pattern can't leak fds.
            if cache.len() >= 8 {
                cache.clear();
            }
            match OpenOptions::new().read(true).open(self.band_path(band)) {
                Ok(f) => {
                    cache.insert(band, f);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Absent band: sparse hole -> zeros.
                    buf.fill(0);
                    return Ok(());
                }
                Err(e) => return Err(Error::from(e)),
            }
        }

        let file = cache
            .get_mut(&band)
            .ok_or_else(|| Error::new(ErrorKind::Internal, "band vanished from cache"))?;
        file.seek(SeekFrom::Start(within)).map_err(Error::from)?;
        // Read what we can; zero-fill any tail beyond the (possibly short) band.
        let mut filled = 0;
        while filled < buf.len() {
            match file.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::from(e)),
            }
        }
        buf[filled..].fill(0);
        Ok(())
    }

    /// Scan the bands directory and summarize coverage (for `inspect`).
    pub fn band_stats(&self) -> Result<BandStats> {
        let expected_bands = self.logical_size.div_ceil(self.band_size);
        let mut present: Vec<u64> = Vec::new();
        let mut short_bands: Vec<u64> = Vec::new();
        let last_band = expected_bands.saturating_sub(1);

        for entry in std::fs::read_dir(&self.bands_dir).map_err(Error::from)? {
            let entry = entry.map_err(Error::from)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Band names are lowercase hex; ignore anything else.
            if let Ok(idx) = u64::from_str_radix(&name, 16) {
                let meta = entry.metadata().map_err(Error::from)?;
                if !meta.is_file() {
                    continue;
                }
                present.push(idx);
                // A short band (other than the final partial band) is reported.
                if meta.len() < self.band_size && idx != last_band {
                    short_bands.push(idx);
                }
            }
        }
        present.sort_unstable();
        short_bands.sort_unstable();

        let present_set: std::collections::HashSet<u64> = present.iter().copied().collect();
        let mut missing_bands = Vec::new();
        let mut missing_truncated = false;
        for i in 0..expected_bands {
            if !present_set.contains(&i) {
                if missing_bands.len() < MAX_LISTED_MISSING {
                    missing_bands.push(i);
                } else {
                    missing_truncated = true;
                    break;
                }
            }
        }

        Ok(BandStats {
            band_size: self.band_size,
            logical_size: self.logical_size,
            expected_bands,
            present_bands: present.len() as u64,
            missing_bands,
            missing_truncated,
            short_bands,
        })
    }
}

impl BlockDevice for SparseBundleDevice {
    fn size(&self) -> u64 {
        self.logical_size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        check_range(offset, buf.len(), self.logical_size)?;
        let mut done = 0usize;
        while done < buf.len() {
            let pos = offset + done as u64;
            let band = pos / self.band_size;
            let within = pos % self.band_size;
            let avail = (self.band_size - within) as usize;
            let chunk = avail.min(buf.len() - done);
            self.read_band(band, within, &mut buf[done..done + chunk])?;
            done += chunk;
        }
        Ok(())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

/// Extract the integer value following `<key>NAME</key>` in a small XML plist.
/// Robust to whitespace; rejects a key whose next value tag is not `<integer>`.
fn plist_int(buf: &str, key: &str) -> Option<u64> {
    let needle = format!("<key>{key}</key>");
    let after = buf.split_once(&needle)?.1;
    let int_open = after.find("<integer>")? + "<integer>".len();
    let rest = &after[int_open..];
    let int_close = rest.find("</integer>")?;
    rest[..int_close].trim().parse::<u64>().ok()
}

/// Extract the string value following `<key>NAME</key>`.
fn plist_string(buf: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let after = buf.split_once(&needle)?.1;
    let open = after.find("<string>")? + "<string>".len();
    let rest = &after[open..];
    let close = rest.find("</string>")?;
    Some(rest[..close].trim().to_string())
}

fn parse_info_plist(path: &Path) -> Result<InfoPlist> {
    let f = File::open(path).map_err(|e| {
        Error::new(
            match e.kind() {
                std::io::ErrorKind::NotFound => ErrorKind::UnsupportedFormat,
                std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
                _ => ErrorKind::Io,
            },
            format!("cannot read `{}`: {e}", path.display()),
        )
    })?;
    let mut buf = String::new();
    // Info.plist is tiny; cap the read so a malicious huge file can't OOM us.
    f.take(1 << 20).read_to_string(&mut buf).map_err(|e| {
        Error::new(
            ErrorKind::Corrupt,
            format!("malformed Info.plist (not UTF-8 text?): {e}"),
        )
    })?;

    let band_size = plist_int(&buf, "band-size").ok_or_else(|| {
        Error::new(
            ErrorKind::Corrupt,
            "Info.plist missing or malformed `band-size`",
        )
    })?;
    // `size` is optional in the spec; without it we can't bound reads, so treat
    // its absence as unsupported rather than guessing.
    let size = plist_int(&buf, "size").ok_or_else(|| {
        Error::new(
            ErrorKind::UnsupportedFeature,
            "Info.plist has no `size`; cannot determine logical size",
        )
    })?;

    Ok(InfoPlist {
        band_size,
        size,
        uuid: plist_string(&buf, "uuid"),
        backingstore_version: plist_int(&buf, "bundle-backingstore-version"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_plist(dir: &Path) {
        let mut f = File::create(dir.join("Info.plist")).unwrap();
        write!(
            f,
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>band-size</key><integer>16</integer>
<key>size</key><integer>40</integer>
<key>uuid</key><string>abc-123</string>
<key>bundle-backingstore-version</key><integer>2</integer>
</dict></plist>"#
        )
        .unwrap();
    }

    #[test]
    fn reads_across_bands_with_holes() {
        let mut base = std::env::temp_dir();
        base.push(format!(
            "apfsrelic-sb-test-{}.sparsebundle",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("bands")).unwrap();
        write_plist(&base);
        // band 0 = bytes [0,16): full of 0xAA. band 1 missing. band 2 partial.
        std::fs::write(base.join("bands/0"), [0xAAu8; 16]).unwrap();
        std::fs::write(base.join("bands/2"), [0xCCu8; 4]).unwrap();

        assert!(SparseBundleDevice::looks_like(&base));
        let dev = SparseBundleDevice::open(&base).unwrap();
        assert_eq!(dev.size(), 40);

        // Cross band 0 -> 1 (hole): 8 bytes of 0xAA then 8 of 0x00.
        let mut buf = [0u8; 16];
        dev.read_at(8, &mut buf).unwrap();
        assert_eq!(&buf[..8], &[0xAA; 8]);
        assert_eq!(&buf[8..], &[0x00; 8]);

        // band 2 partial: first 4 bytes 0xCC, then zero-filled tail.
        let mut buf2 = [0u8; 8];
        dev.read_at(32, &mut buf2).unwrap();
        assert_eq!(&buf2[..4], &[0xCC; 4]);
        assert_eq!(&buf2[4..], &[0x00; 4]);

        let stats = dev.band_stats().unwrap();
        assert_eq!(stats.expected_bands, 3);
        assert_eq!(stats.present_bands, 2);
        assert_eq!(stats.missing_bands, vec![1]);
        // band 2 is the final band, so its short length is not flagged.
        assert!(stats.short_bands.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_size_is_unsupported_not_panic() {
        let mut base = std::env::temp_dir();
        base.push(format!(
            "apfsrelic-sb-nosize-{}.sparsebundle",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("bands")).unwrap();
        let mut f = File::create(base.join("Info.plist")).unwrap();
        write!(
            f,
            "<plist><dict><key>band-size</key><integer>16</integer></dict></plist>"
        )
        .unwrap();
        let result = SparseBundleDevice::open(&base);
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::UnsupportedFeature));
        let _ = std::fs::remove_dir_all(&base);
    }
}
