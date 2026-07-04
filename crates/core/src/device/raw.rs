//! `RawFileDevice` — a plain file (raw APFS container image, `.sparseimage`
//! payload, or a `/dev/rdiskXsY` device node) read read-only.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

use super::{check_range, BlockDevice};
use crate::error::{Error, ErrorKind, Result};

/// A read-only view of a single file as a flat block device.
///
/// The file is opened with read access only (never write/create/truncate), so
/// the input image cannot be modified through this backend. Concurrent
/// `read_at` calls are serialized through a `Mutex` because positioned reads on
/// a shared `File` require exclusive access to the cursor on all platforms.
pub struct RawFileDevice {
    file: Mutex<File>,
    size: u64,
    description: String,
}

impl RawFileDevice {
    /// Open `path` read-only.
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|e| annotate(e, path))?;
        let size = file.metadata().map_err(Error::from)?.len();
        Ok(RawFileDevice {
            file: Mutex::new(file),
            size,
            description: format!("raw image `{}` ({} bytes)", path.display(), size),
        })
    }

    /// Open a device node such as `/dev/rdisk4s1`, whose `metadata().len()` is
    /// often reported as 0; the caller passes the real size out-of-band.
    pub fn open_device(path: &Path, size: u64) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|e| annotate(e, path))?;
        Ok(RawFileDevice {
            file: Mutex::new(file),
            size,
            description: format!("device `{}` ({} bytes)", path.display(), size),
        })
    }
}

fn annotate(e: std::io::Error, path: &Path) -> Error {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => ErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        _ => ErrorKind::Io,
    };
    Error::new(kind, format!("cannot open `{}`: {e}", path.display()))
}

impl BlockDevice for RawFileDevice {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        check_range(offset, buf.len(), self.size)?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "device mutex poisoned"))?;
        file.seek(SeekFrom::Start(offset)).map_err(Error::from)?;
        file.read_exact(buf).map_err(Error::from)?;
        Ok(())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_bytes_and_rejects_overrun() {
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("apfsrelic-raw-test-{}.bin", std::process::id()));
        {
            let mut f = File::create(&tmp).unwrap();
            f.write_all(&(0u8..16).collect::<Vec<_>>()).unwrap();
        }
        let dev = RawFileDevice::open(&tmp).unwrap();
        assert_eq!(dev.size(), 16);
        let mut b = [0u8; 4];
        dev.read_at(4, &mut b).unwrap();
        assert_eq!(b, [4, 5, 6, 7]);
        // Overrun must error, not panic.
        assert!(dev.read_at(14, &mut [0u8; 4]).is_err());
        let _ = std::fs::remove_file(&tmp);
    }
}
