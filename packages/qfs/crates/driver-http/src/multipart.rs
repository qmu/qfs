//! The generic `multipart/form-data` wire-body encoder — the §13 declared-map **upload**
//! primitive (`… |> ENCODE multipart VALUES (row)`, ticket 20260711121526).
//!
//! Deliberately service-agnostic: the evaluated map body (a [`Value::Struct`]) maps onto form
//! parts by one small convention, so a declared driver expresses any single-file upload without
//! compiled code:
//!   * every struct field becomes one form part, in field order;
//!   * a [`Value::Bytes`] field becomes the FILE part (`filename=` from the sibling `filename`
//!     text field, else the field name); every scalar field is a plain text part;
//!   * the `filename` field itself names the file part and is NOT emitted as a part of its own;
//!   * `Null` fields are skipped (an absent optional part, not an empty one).
//!
//! The boundary is derived deterministically from the part contents (an FNV-1a fingerprint,
//! lengthened until it collides with no part), so the encode is a pure function of the body —
//! goldens stay byte-stable, no clock/randomness enters the wire layer.

use qfs_codec::{Row, RowBatch, Value};
use qfs_types::{Column, ColumnType, Schema};

use crate::effect::{BODY_COL, HEADER_COL_PREFIX};

/// One encoded multipart body: the wire bytes and the `Content-Type` header value carrying the
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartBody {
    /// The `multipart/form-data` payload bytes.
    pub bytes: Vec<u8>,
    /// The full `Content-Type` header value (`multipart/form-data; boundary=…`).
    pub content_type: String,
}

/// Encode an evaluated declared-map body into a `multipart/form-data` payload.
///
/// # Errors
/// A human-readable reason when the body is not a struct, carries no parts, or a field's type
/// has no part mapping (arrays / nested structs).
pub fn encode_multipart(body: &Value) -> Result<MultipartBody, String> {
    let Value::Struct(fields) = body else {
        return Err("ENCODE multipart needs a struct body ({field: …, …})".to_string());
    };
    // The optional `filename` convention field names the file part (and is not itself a part).
    let filename = match fields.get("filename") {
        Some(Value::Text(name)) if !name.is_empty() => Some(name.clone()),
        _ => None,
    };

    // Render each part's (headers, content) pair first — the boundary derives from the contents.
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, value) in &fields.entries {
        if name == "filename" && filename.is_some() {
            continue;
        }
        let part = match value {
            Value::Null => continue,
            Value::Bytes(bytes) => {
                let fname = filename.clone().unwrap_or_else(|| name.clone());
                (
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{fname}\"\r\n\
                         Content-Type: application/octet-stream\r\n"
                    ),
                    bytes.clone(),
                )
            }
            Value::Text(s) => (
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n"),
                s.clone().into_bytes(),
            ),
            Value::Int(i) => (
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n"),
                i.to_string().into_bytes(),
            ),
            Value::Float(f) => (
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n"),
                f.to_string().into_bytes(),
            ),
            Value::Bool(b) => (
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n"),
                b.to_string().into_bytes(),
            ),
            Value::Timestamp(t) => (
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n"),
                t.to_string().into_bytes(),
            ),
            other => {
                return Err(format!(
                    "ENCODE multipart cannot map field `{name}` ({:?}) onto a form part",
                    other.type_of()
                ))
            }
        };
        parts.push(part);
    }
    if parts.is_empty() {
        return Err("ENCODE multipart produced no parts (empty struct body)".to_string());
    }

    let boundary = derive_boundary(&parts);
    let mut bytes = Vec::new();
    for (headers, content) in &parts {
        bytes.extend_from_slice(format!("--{boundary}\r\n{headers}\r\n").as_bytes());
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(MultipartBody {
        bytes,
        content_type: format!("multipart/form-data; boundary={boundary}"),
    })
}

/// Build the single-row args carrying a multipart-encoded wire body: the payload under
/// [`BODY_COL`] plus the boundary-bearing `Content-Type` as an effect header override (the
/// applier layers it after the config defaults). The multipart twin of
/// [`crate::http_body_args`].
///
/// # Errors
/// The [`encode_multipart`] reason when the body cannot be encoded.
pub fn http_multipart_args(body: &Value) -> Result<RowBatch, String> {
    let encoded = encode_multipart(body)?;
    Ok(RowBatch::new(
        Schema::new(vec![
            Column::new(BODY_COL, ColumnType::Bytes, false),
            Column::new(
                format!("{HEADER_COL_PREFIX}Content-Type"),
                ColumnType::Text,
                false,
            ),
        ]),
        vec![Row::new(vec![
            Value::Bytes(encoded.bytes),
            Value::Text(encoded.content_type),
        ])],
    ))
}

/// A deterministic boundary no part contains: an FNV-1a fingerprint of every part's headers +
/// content, re-hashed (fixed-point style) while any part contains the candidate — so the encode
/// stays a pure function of the body with the RFC 2046 "boundary must not occur in the data"
/// guarantee intact.
fn derive_boundary(parts: &[(String, Vec<u8>)]) -> String {
    fn feed(mut hash: u64, bytes: &[u8]) -> u64 {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (headers, content) in parts {
        hash = feed(hash, headers.as_bytes());
        hash = feed(hash, content);
    }
    loop {
        let candidate = format!("qfs-multipart-{hash:016x}");
        let collides = parts.iter().any(|(headers, content)| {
            headers
                .as_bytes()
                .windows(candidate.len())
                .any(|w| w == candidate.as_bytes())
                || content
                    .windows(candidate.len())
                    .any(|w| w == candidate.as_bytes())
        });
        if !collides {
            return candidate;
        }
        hash = feed(hash, candidate.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use qfs_types::Fields;

    use super::*;

    fn struct_body(entries: Vec<(&str, Value)>) -> Value {
        Value::Struct(Fields::new(
            entries
                .into_iter()
                .map(|(n, v)| (n.to_string(), v))
                .collect(),
        ))
    }

    /// The generic convention end to end: a bytes field becomes the file part named by the
    /// sibling `filename` (which is itself NOT a part), scalars become text parts, and the
    /// payload closes with the terminal boundary.
    #[test]
    fn encodes_a_file_part_with_the_filename_convention_and_text_parts() {
        let body = struct_body(vec![
            ("file", Value::Bytes(b"PDFBYTES".to_vec())),
            ("filename", Value::Text("report.pdf".into())),
            ("message", Value::Text("here you go".into())),
        ]);
        let encoded = encode_multipart(&body).expect("encodes");
        let text = String::from_utf8_lossy(&encoded.bytes).to_string();
        assert!(
            text.contains("Content-Disposition: form-data; name=\"file\"; filename=\"report.pdf\""),
            "the bytes field is the file part, named by the filename convention: {text}"
        );
        assert!(text.contains("Content-Type: application/octet-stream"));
        assert!(text.contains("PDFBYTES"));
        assert!(
            text.contains("Content-Disposition: form-data; name=\"message\"\r\n\r\nhere you go"),
            "scalars are plain text parts: {text}"
        );
        assert!(
            !text.contains("name=\"filename\""),
            "the filename convention field is not a part of its own: {text}"
        );
        let boundary = encoded
            .content_type
            .strip_prefix("multipart/form-data; boundary=")
            .expect("content type carries the boundary");
        assert!(
            text.ends_with(&format!("--{boundary}--\r\n")),
            "terminal boundary"
        );
    }

    /// Without a `filename` field, the file part is named by its own field name; a bare bytes
    /// field alone is a valid single-part upload.
    #[test]
    fn a_bytes_field_without_filename_uses_the_field_name() {
        let body = struct_body(vec![("file", Value::Bytes(b"DATA".to_vec()))]);
        let encoded = encode_multipart(&body).expect("encodes");
        let text = String::from_utf8_lossy(&encoded.bytes).to_string();
        assert!(text.contains("name=\"file\"; filename=\"file\""));
    }

    /// The encode is deterministic (a pure function of the body): same body, same bytes, same
    /// boundary — the golden-stability contract.
    #[test]
    fn the_encode_is_deterministic() {
        let body = struct_body(vec![
            ("file", Value::Bytes(vec![1, 2, 3])),
            ("message", Value::Text("x".into())),
        ]);
        let a = encode_multipart(&body).unwrap();
        let b = encode_multipart(&body).unwrap();
        assert_eq!(a, b);
    }

    /// Nulls are skipped; a non-struct or unmappable field is a structured refusal.
    #[test]
    fn null_fields_skip_and_bad_shapes_refuse() {
        let body = struct_body(vec![("file", Value::Bytes(vec![9])), ("note", Value::Null)]);
        let encoded = encode_multipart(&body).unwrap();
        assert!(!String::from_utf8_lossy(&encoded.bytes).contains("name=\"note\""));

        assert!(encode_multipart(&Value::Text("not a struct".into())).is_err());
        let nested = struct_body(vec![("bad", Value::Array(vec![]))]);
        assert!(encode_multipart(&nested).is_err());
    }

    /// The args twin carries the payload under `__http_body` plus the boundary-bearing
    /// `Content-Type` header override the applier layers onto the POST.
    #[test]
    fn http_multipart_args_carries_body_and_content_type_header() {
        let body = struct_body(vec![("file", Value::Bytes(vec![7]))]);
        let args = http_multipart_args(&body).expect("args");
        assert_eq!(args.schema.columns[0].name, BODY_COL);
        assert_eq!(
            args.schema.columns[1].name,
            format!("{HEADER_COL_PREFIX}Content-Type")
        );
        let Value::Text(ct) = &args.rows[0].values[1] else {
            panic!("content type is text");
        };
        assert!(ct.starts_with("multipart/form-data; boundary="));
    }
}
