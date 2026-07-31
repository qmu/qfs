//! The generic `application/x-www-form-urlencoded` wire-body encoder — the §13 declared-map
//! **form-parameter** primitive (`… |> ENCODE form VALUES (row)`, ticket 20260727214856).
//!
//! Most plain-REST APIs of the pre-JSON generation (Chatwork, Slack's Web API, and every service
//! whose docs say "POST parameters") accept parameters ONLY as a form body and answer a JSON body
//! with 400. Without this encoder such an API is readable but not writable through a declaration,
//! which is a direct counter-example to the blueprint §13 claim that a plain-REST API is expressible
//! as declarative config.
//!
//! The convention, deliberately service-agnostic and deliberately narrow — the evaluated map body (a
//! [`Value::Struct`]) maps onto form parameters:
//!   * every struct field becomes one `key=value` pair, **in field order** (declaration order is the
//!     wire order, so a golden request stays byte-stable);
//!   * every scalar field ([`Value::Text`], `Int`, `Float`, `Bool`, `Timestamp`) renders through its
//!     display form and is percent-encoded;
//!   * `Null` fields are **skipped** — an absent optional parameter, not an empty one (the same
//!     reading [`crate::multipart`] gives a null part);
//!   * a [`Value::Bytes`] field is **refused**: a form body has no binary parameter encoding, and
//!     silently lossy-stringifying bytes onto the wire is exactly the guess the predicate-honesty law
//!     forbids. An upload belongs in `ENCODE multipart`, and the refusal says so;
//!   * a nested [`Value::Struct`] or [`Value::Array`] is **refused** for the same reason — this
//!     encoder deliberately implements NO bracket/index flattening convention (`a[b]=`, `a.0=`,
//!     repeated keys), because there is no interoperable one and picking silently would make the
//!     wire shape unpredictable across services.
//!
//! Percent-encoding is the RFC 3986 unreserved set (`A-Z a-z 0-9 - _ . ~`) passed through verbatim,
//! every other byte as uppercase `%XX`. A space is therefore `%20`, never `+` — accepted everywhere
//! `+` is, and free of the `+`/space ambiguity. Multi-byte characters encode per UTF-8 byte.
//!
//! The encode is a pure function of the body: no clock, no randomness, no map iteration order.

use qfs_codec::{Row, RowBatch, Value};
use qfs_types::{Column, ColumnType, Schema};

use crate::effect::{BODY_COL, HEADER_COL_PREFIX};

/// The fixed `Content-Type` a form body carries.
pub const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";

/// One encoded form body: the wire bytes and the `Content-Type` header value.
///
/// The content type is constant (unlike [`crate::multipart::MultipartBody`], whose boundary is
/// content-derived), but it rides on the struct so both encoders present one shape to the applier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormBody {
    /// The `application/x-www-form-urlencoded` payload bytes.
    pub bytes: Vec<u8>,
    /// The full `Content-Type` header value.
    pub content_type: String,
}

/// Encode an evaluated declared-map body into an `application/x-www-form-urlencoded` payload.
///
/// # Errors
/// A human-readable reason when the body is not a struct, carries no parameters, or a field's type
/// has no form-parameter mapping (bytes, arrays, nested structs — see the module docs for why each
/// is a refusal rather than a convention).
pub fn encode_form(body: &Value) -> Result<FormBody, String> {
    let Value::Struct(fields) = body else {
        return Err("ENCODE form needs a struct body ({field: …, …})".to_string());
    };

    let mut pairs: Vec<String> = Vec::new();
    for (name, value) in &fields.entries {
        let rendered = match value {
            Value::Null => continue,
            Value::Text(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Timestamp(t) => t.to_string(),
            Value::Bytes(_) => {
                return Err(format!(
                    "ENCODE form cannot send field `{name}` as bytes — a form body has no binary \
                     parameter encoding; use ENCODE multipart for an upload"
                ))
            }
            other => {
                return Err(format!(
                    "ENCODE form cannot map field `{name}` ({:?}) onto a form parameter — only \
                     scalar fields have a form encoding",
                    other.type_of()
                ))
            }
        };
        pairs.push(format!(
            "{}={}",
            percent_encode(name),
            percent_encode(&rendered)
        ));
    }
    if pairs.is_empty() {
        return Err("ENCODE form produced no parameters (empty struct body)".to_string());
    }

    Ok(FormBody {
        bytes: pairs.join("&").into_bytes(),
        content_type: FORM_CONTENT_TYPE.to_string(),
    })
}

/// Build the single-row args carrying a form-encoded wire body: the payload under [`BODY_COL`] plus
/// the `Content-Type` as an effect header override (the applier layers it after the config
/// defaults, so a declaration does not have to restate it). The form twin of
/// [`crate::multipart::http_multipart_args`].
///
/// # Errors
/// The [`encode_form`] reason when the body cannot be encoded.
pub fn http_form_args(body: &Value) -> Result<RowBatch, String> {
    let encoded = encode_form(body)?;
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

/// Percent-encode one form key or value: the RFC 3986 unreserved set passes through verbatim, every
/// other byte becomes uppercase `%XX`. Dependency-free and wasm-safe (this crate links no
/// `url`/`percent-encoding` crate), mirroring the same routine the Slack client carries.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(hex_upper(b >> 4));
            out.push(hex_upper(b & 0x0F));
        }
    }
    out
}

/// One uppercase hex digit for a nibble.
const fn hex_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
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

    fn encoded_text(entries: Vec<(&str, Value)>) -> String {
        let encoded = encode_form(&struct_body(entries)).expect("encodes");
        String::from_utf8(encoded.bytes).expect("form bodies are ASCII after encoding")
    }

    /// Field order is declaration order, and the pairs join with `&` — the byte-stable golden shape.
    #[test]
    fn encodes_scalars_as_ordered_key_value_pairs() {
        let body = encoded_text(vec![
            ("body", Value::Text("hello".into())),
            ("count", Value::Int(3)),
            ("flag", Value::Bool(true)),
        ]);
        assert_eq!(body, "body=hello&count=3&flag=true");
    }

    /// The reserved characters a naive encoder would let through unescaped — `&`, `=`, `+`, `%` and
    /// space — each become `%XX`, so a value can never forge a second parameter.
    #[test]
    fn percent_encodes_reserved_characters_so_a_value_cannot_forge_a_parameter() {
        let body = encoded_text(vec![("body", Value::Text("a&b=c+d e%f".into()))]);
        assert_eq!(body, "body=a%26b%3Dc%2Bd%20e%25f");
        assert!(
            !body.contains('+'),
            "a space is %20, never + — no +/space ambiguity: {body}"
        );
    }

    /// Multi-byte text encodes per UTF-8 byte, uppercase hex.
    #[test]
    fn percent_encodes_multibyte_text_per_utf8_byte() {
        let body = encoded_text(vec![("body", Value::Text("こんにちは".into()))]);
        assert_eq!(
            body, "body=%E3%81%93%E3%82%93%E3%81%AB%E3%81%A1%E3%81%AF",
            "each UTF-8 byte becomes one uppercase %XX escape"
        );
    }

    /// A key is encoded with the same rule as a value.
    #[test]
    fn percent_encodes_the_key_too() {
        let body = encoded_text(vec![("odd key", Value::Text("v".into()))]);
        assert_eq!(body, "odd%20key=v");
    }

    /// A null field is an ABSENT parameter, not an empty one — the same reading multipart gives.
    #[test]
    fn null_fields_are_skipped_not_sent_empty() {
        let body = encoded_text(vec![
            ("body", Value::Text("x".into())),
            ("optional", Value::Null),
        ]);
        assert_eq!(body, "body=x");
    }

    /// A bytes field is refused, and the refusal names the encoder that DOES take one — never a
    /// silent lossy stringify onto the wire.
    #[test]
    fn a_bytes_field_is_refused_and_points_at_multipart() {
        let err = encode_form(&struct_body(vec![("file", Value::Bytes(vec![1, 2, 3]))]))
            .expect_err("bytes have no form encoding");
        assert!(err.contains("ENCODE multipart"), "{err}");
    }

    /// A nested struct or array is refused rather than flattened by a guessed convention.
    #[test]
    fn nested_shapes_are_refused_rather_than_flattened() {
        assert!(encode_form(&struct_body(vec![("bad", Value::Array(vec![]))])).is_err());
        let nested = struct_body(vec![("bad", struct_body(vec![("inner", Value::Int(1))]))]);
        assert!(encode_form(&nested).is_err());
    }

    /// A non-struct body and an all-null (so empty) body are both structured refusals.
    #[test]
    fn a_non_struct_or_empty_body_refuses() {
        assert!(encode_form(&Value::Text("not a struct".into())).is_err());
        assert!(encode_form(&struct_body(vec![("only", Value::Null)])).is_err());
    }

    /// The encode is deterministic — same body, same bytes.
    #[test]
    fn the_encode_is_deterministic() {
        let body = struct_body(vec![
            ("body", Value::Text("x".into())),
            ("n", Value::Int(1)),
        ]);
        assert_eq!(encode_form(&body).unwrap(), encode_form(&body).unwrap());
    }

    /// The args twin carries the payload under `__http_body` plus the form `Content-Type` override
    /// the applier layers onto the POST — the header the Chatwork 400 was missing.
    #[test]
    fn http_form_args_carries_body_and_content_type_header() {
        let args = http_form_args(&struct_body(vec![("body", Value::Text("hi".into()))]))
            .expect("args encode");
        assert_eq!(args.schema.columns[0].name, BODY_COL);
        assert_eq!(
            args.schema.columns[1].name,
            format!("{HEADER_COL_PREFIX}Content-Type")
        );
        let Value::Bytes(payload) = &args.rows[0].values[0] else {
            panic!("the body rides as bytes");
        };
        assert_eq!(payload, b"body=hi");
        let Value::Text(ct) = &args.rows[0].values[1] else {
            panic!("content type is text");
        };
        assert_eq!(ct, FORM_CONTENT_TYPE);
    }
}
