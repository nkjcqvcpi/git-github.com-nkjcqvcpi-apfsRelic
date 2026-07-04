//! Structured error model and process exit codes (rewrite plan Phase 23).
//!
//! Every fallible operation returns [`Result<T>`]. Errors are grouped into a
//! small set of stable classes so that callers (and the JSON layer) can react
//! programmatically and so the process exit code communicates the class of
//! failure. No error path ever panics on malformed image data.

use std::fmt;
use std::io;

/// The machine-readable class of an error. The string form (`code`) is part of
/// the stable JSON contract; do not rename existing variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Bad command-line usage (missing/contradictory options).
    Usage,
    /// The input image path does not exist.
    NotFound,
    /// The input image exists but cannot be opened (permissions).
    PermissionDenied,
    /// Generic underlying I/O failure while reading the image.
    Io,
    /// The container/image format is not one we support.
    UnsupportedFormat,
    /// A specific feature of an otherwise-supported format is unsupported.
    UnsupportedFeature,
    /// The image is structurally corrupt (bad magic, impossible geometry, ...).
    Corrupt,
    /// An APFS object's stored checksum did not validate.
    ChecksumFailed,
    /// An APFS object referenced by id/address could not be found.
    ObjectNotFound,
    /// A filesystem path could not be resolved.
    PathNotFound,
    /// A path component that should have been a directory was not.
    NotADirectory,
    /// Data is encrypted and plaintext access was refused.
    EncryptedUnsupported,
    /// Recovery completed only partially (some bytes/files missing).
    PartialRecovery,
    /// An internal invariant was violated — a bug, not bad input.
    Internal,
}

impl ErrorKind {
    /// Stable lowercase identifier used in JSON `error.code`.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Usage => "usage",
            ErrorKind::NotFound => "not-found",
            ErrorKind::PermissionDenied => "permission-denied",
            ErrorKind::Io => "io",
            ErrorKind::UnsupportedFormat => "unsupported-format",
            ErrorKind::UnsupportedFeature => "unsupported-feature",
            ErrorKind::Corrupt => "corrupt",
            ErrorKind::ChecksumFailed => "checksum-failed",
            ErrorKind::ObjectNotFound => "object-not-found",
            ErrorKind::PathNotFound => "path-not-found",
            ErrorKind::NotADirectory => "not-a-directory",
            ErrorKind::EncryptedUnsupported => "encrypted-unsupported",
            ErrorKind::PartialRecovery => "partial-recovery",
            ErrorKind::Internal => "internal",
        }
    }

    /// BSD-`sysexits.h`-flavoured process exit code for this class. Distinct
    /// codes for usage / IO / unsupported / corrupt / partial recovery so
    /// scripts can branch on `$?`.
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Usage => 64,                // EX_USAGE
            ErrorKind::NotFound => 66,             // EX_NOINPUT
            ErrorKind::PermissionDenied => 77,     // EX_NOPERM
            ErrorKind::Io => 74,                   // EX_IOERR
            ErrorKind::UnsupportedFormat => 65,    // EX_DATAERR-ish (format)
            ErrorKind::UnsupportedFeature => 69,   // EX_UNAVAILABLE
            ErrorKind::Corrupt => 65,              // EX_DATAERR
            ErrorKind::ChecksumFailed => 65,       // EX_DATAERR
            ErrorKind::ObjectNotFound => 65,       // EX_DATAERR
            ErrorKind::PathNotFound => 66,         // EX_NOINPUT-ish
            ErrorKind::NotADirectory => 65,        // EX_DATAERR
            ErrorKind::EncryptedUnsupported => 69, // EX_UNAVAILABLE
            ErrorKind::PartialRecovery => 75,      // EX_TEMPFAIL-ish (partial)
            ErrorKind::Internal => 70,             // EX_SOFTWARE
        }
    }
}

/// A structured error carrying a [`ErrorKind`] class and a human message.
#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    /// Optional extra context (e.g. the OID or block that failed).
    pub context: Option<String>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ctx) = &self.context {
            write!(f, " ({ctx})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        let kind = match e.kind() {
            io::ErrorKind::NotFound => ErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
            _ => ErrorKind::Io,
        };
        Error::new(kind, e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ---- Convenience constructors used throughout the parsers ----

/// `Corrupt` error.
pub fn corrupt(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt, msg)
}

/// `ChecksumFailed` error.
pub fn checksum(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::ChecksumFailed, msg)
}

/// `UnsupportedFeature` error.
pub fn unsupported(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::UnsupportedFeature, msg)
}

/// `ObjectNotFound` error.
pub fn not_found_obj(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::ObjectNotFound, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_exit_codes_for_major_classes() {
        // Usage, IO, unsupported, corrupt, and partial recovery must be
        // distinguishable by exit code (acceptance criterion, Phase 23).
        let codes = [
            ErrorKind::Usage.exit_code(),
            ErrorKind::Io.exit_code(),
            ErrorKind::UnsupportedFeature.exit_code(),
            ErrorKind::Corrupt.exit_code(),
            ErrorKind::PartialRecovery.exit_code(),
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "exit codes must be distinct");
    }

    #[test]
    fn io_error_maps_to_kind() {
        let e: Error = io::Error::new(io::ErrorKind::PermissionDenied, "nope").into();
        assert_eq!(e.kind(), ErrorKind::PermissionDenied);
    }
}
