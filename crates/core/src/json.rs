//! A tiny, dependency-free JSON value type and serializer, plus the stable
//! top-level envelope (rewrite plan Phase 22).
//!
//! We deliberately avoid `serde`/`serde_json` so that the entire output path is
//! auditable in-tree and so the crate has no runtime dependencies. The model is
//! small but complete: objects preserve insertion order (so golden tests are
//! deterministic), strings are escaped per RFC 8259, and integers serialize as
//! JSON numbers (never quoted) so GUI consumers get real numbers for sizes.

use std::fmt::Write as _;

/// The JSON schema version emitted in every `--json` envelope. Bump on any
/// breaking change to the output shape.
pub const SCHEMA_VERSION: u32 = 1;

/// An owned JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Signed integer (serialized unquoted).
    Int(i64),
    /// Unsigned integer (serialized unquoted). Kept distinct so large `u64`
    /// values above `i64::MAX` round-trip.
    UInt(u64),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    /// Insertion-ordered object.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Build an empty object.
    pub fn obj() -> Json {
        Json::Object(Vec::new())
    }

    /// Append a key/value to an object, returning self for chaining.
    /// No-op if `self` is not an object (should not happen in practice).
    pub fn set(mut self, key: &str, value: impl Into<Json>) -> Json {
        if let Json::Object(entries) = &mut self {
            entries.push((key.to_string(), value.into()));
        }
        self
    }

    /// Mutable insert used when building objects in a loop.
    pub fn insert(&mut self, key: &str, value: impl Into<Json>) {
        if let Json::Object(entries) = self {
            entries.push((key.to_string(), value.into()));
        }
    }

    /// A hex string like `0x1f`, used for object ids / block addresses to match
    /// the legacy output (which prints OIDs as `%#PRIx64`).
    pub fn hex(value: u64) -> Json {
        Json::Str(format!("{value:#x}"))
    }

    /// Serialize compactly (no whitespace) — the default for stdout.
    pub fn to_compact_string(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out);
        out
    }

    /// Serialize with 2-space indentation — used for human-readable dumps and
    /// golden test fixtures.
    pub fn to_pretty_string(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_compact(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Json::UInt(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Float(x) => write_float(out, *x),
            Json::Str(s) => write_json_string(out, s),
            Json::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_compact(out);
                }
                out.push(']');
            }
            Json::Object(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(out, k);
                    out.push(':');
                    v.write_compact(out);
                }
                out.push('}');
            }
        }
    }

    fn write_pretty(&self, out: &mut String, indent: usize) {
        match self {
            Json::Array(items) if !items.is_empty() => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write_pretty(out, indent + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push(']');
            }
            Json::Object(entries) if !entries.is_empty() => {
                out.push_str("{\n");
                for (i, (k, v)) in entries.iter().enumerate() {
                    push_indent(out, indent + 1);
                    write_json_string(out, k);
                    out.push_str(": ");
                    v.write_pretty(out, indent + 1);
                    if i + 1 < entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push('}');
            }
            // Empty containers and scalars: same as compact.
            _ => self.write_compact(out),
        }
    }
}

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_float(out: &mut String, x: f64) {
    if x.is_finite() {
        let _ = write!(out, "{x}");
    } else {
        // JSON has no NaN/Infinity; emit null per common practice.
        out.push_str("null");
    }
}

/// Write `s` as a JSON string literal including surrounding quotes, escaping
/// per RFC 8259. UTF-8 multibyte sequences pass through verbatim so Unicode
/// filenames round-trip.
fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---- `Into<Json>` conveniences ----

impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}
impl From<i64> for Json {
    fn from(n: i64) -> Json {
        Json::Int(n)
    }
}
impl From<i32> for Json {
    fn from(n: i32) -> Json {
        Json::Int(n as i64)
    }
}
impl From<u64> for Json {
    fn from(n: u64) -> Json {
        Json::UInt(n)
    }
}
impl From<u32> for Json {
    fn from(n: u32) -> Json {
        Json::UInt(n as u64)
    }
}
impl From<u16> for Json {
    fn from(n: u16) -> Json {
        Json::UInt(n as u64)
    }
}
impl From<usize> for Json {
    fn from(n: usize) -> Json {
        Json::UInt(n as u64)
    }
}
impl From<f64> for Json {
    fn from(x: f64) -> Json {
        Json::Float(x)
    }
}
impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}
impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}
impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Json {
        Json::Array(v)
    }
}
impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(o: Option<T>) -> Json {
        match o {
            Some(v) => v.into(),
            None => Json::Null,
        }
    }
}

/// Build the stable top-level envelope shared by all `--json` output.
///
/// Shape:
/// ```text
/// { "schema_version", "command", "image"?, "partition"?, "volume"?,
///   "snapshot"?, "checkpoint_xid"?, "result"?, "warnings":[], "error"? }
/// ```
pub struct Envelope {
    command: String,
    image: Option<Json>,
    partition: Option<Json>,
    volume: Option<Json>,
    snapshot: Option<Json>,
    checkpoint_xid: Option<u64>,
    result: Option<Json>,
    warnings: Vec<String>,
    error: Option<Json>,
}

impl Envelope {
    pub fn new(command: &str) -> Self {
        Envelope {
            command: command.to_string(),
            image: None,
            partition: None,
            volume: None,
            snapshot: None,
            checkpoint_xid: None,
            result: None,
            warnings: Vec::new(),
            error: None,
        }
    }

    pub fn image(mut self, image: Json) -> Self {
        self.image = Some(image);
        self
    }
    pub fn partition(mut self, p: Json) -> Self {
        self.partition = Some(p);
        self
    }
    pub fn volume(mut self, v: Json) -> Self {
        self.volume = Some(v);
        self
    }
    pub fn snapshot(mut self, s: Json) -> Self {
        self.snapshot = Some(s);
        self
    }
    pub fn checkpoint_xid(mut self, xid: u64) -> Self {
        self.checkpoint_xid = Some(xid);
        self
    }
    pub fn result(mut self, r: Json) -> Self {
        self.result = Some(r);
        self
    }
    pub fn warning(mut self, w: impl Into<String>) -> Self {
        self.warnings.push(w.into());
        self
    }
    pub fn warnings(mut self, ws: Vec<String>) -> Self {
        self.warnings.extend(ws);
        self
    }
    pub fn error(mut self, code: &str, message: &str) -> Self {
        self.error = Some(Json::obj().set("code", code).set("message", message));
        self
    }

    pub fn build(self) -> Json {
        let mut o = Json::obj()
            .set("schema_version", SCHEMA_VERSION as u64)
            .set("command", self.command.as_str());
        if let Some(image) = self.image {
            o.insert("image", image);
        }
        if let Some(p) = self.partition {
            o.insert("partition", p);
        }
        if let Some(v) = self.volume {
            o.insert("volume", v);
        }
        if let Some(s) = self.snapshot {
            o.insert("snapshot", s);
        }
        if let Some(xid) = self.checkpoint_xid {
            o.insert("checkpoint_xid", Json::UInt(xid));
        }
        if let Some(r) = self.result {
            o.insert("result", r);
        }
        o.insert(
            "warnings",
            Json::Array(self.warnings.into_iter().map(Json::Str).collect()),
        );
        if let Some(e) = self.error {
            o.insert("error", e);
        }
        o
    }

    /// Build and serialize compactly.
    pub fn to_json_string(self) -> String {
        self.build().to_compact_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_strings_and_unicode_roundtrips() {
        let s = Json::Str("a\"b\\c\nd\tÉ走".to_string());
        assert_eq!(s.to_compact_string(), "\"a\\\"b\\\\c\\nd\\tÉ走\"");
    }

    #[test]
    fn integers_are_unquoted_numbers() {
        assert_eq!(
            Json::UInt(2850032923136).to_compact_string(),
            "2850032923136"
        );
        assert_eq!(Json::Int(-7).to_compact_string(), "-7");
    }

    #[test]
    fn objects_preserve_order() {
        let o = Json::obj().set("b", 1u64).set("a", 2u64);
        assert_eq!(o.to_compact_string(), "{\"b\":1,\"a\":2}");
    }

    #[test]
    fn envelope_has_required_fields() {
        let e = Envelope::new("inspect")
            .result(Json::obj())
            .to_json_string();
        assert!(e.contains("\"schema_version\":1"));
        assert!(e.contains("\"command\":\"inspect\""));
        assert!(e.contains("\"warnings\":[]"));
    }

    #[test]
    fn control_chars_escaped_as_u() {
        assert_eq!(
            Json::Str("\u{01}".to_string()).to_compact_string(),
            "\"\\u0001\""
        );
    }
}
