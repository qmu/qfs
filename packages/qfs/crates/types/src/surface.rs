//! Shared surface helpers for the §5 type language.
//!
//! The parser and DDL validators both need to distinguish base column-type tokens from declared
//! type names, and both store declared columns in the same JSON shape. Keeping those rules here
//! lets the type model remain the single lower-level authority without pulling parser/core types
//! into the leaf crate.

use serde::{Deserialize, Serialize};

use crate::ColumnType;

/// One declared column descriptor as serialized by `CREATE TABLE` / `CREATE TYPE` sugar and
/// decoded by declaration validators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default = "default_nullable")]
    pub nullable: bool,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub unique: bool,
}

fn default_nullable() -> bool {
    true
}

/// Return the canonical lowercase base-column token for a surface spelling.
///
/// The §5 surface has one spelling per base type. Retired aliases such as `string`, `varchar`,
/// `integer`, and `jsonb` deliberately return `None` so declaration validators can treat them as
/// unknown declared-type names instead of silently normalizing another dialect.
#[must_use]
pub fn canonical_base_column_type(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bool" => Some("bool"),
        "int" => Some("int"),
        "float" => Some("float"),
        "decimal" => Some("decimal"),
        "text" => Some("text"),
        "bytes" => Some("bytes"),
        "timestamp" => Some("timestamp"),
        "date" => Some("date"),
        "uuid" => Some("uuid"),
        "json" => Some("json"),
        "unknown" => Some("unknown"),
        _ => None,
    }
}

/// Parse a surface base-column spelling into the canonical [`ColumnType`].
#[must_use]
pub fn base_column_type(raw: &str) -> Option<ColumnType> {
    match canonical_base_column_type(raw)? {
        "bool" => Some(ColumnType::Bool),
        "int" => Some(ColumnType::Int),
        "float" => Some(ColumnType::Float),
        "decimal" => Some(ColumnType::Decimal),
        "text" => Some(ColumnType::Text),
        "bytes" => Some(ColumnType::Bytes),
        "timestamp" => Some(ColumnType::Timestamp),
        "date" => Some(ColumnType::Date),
        "uuid" => Some(ColumnType::Uuid),
        "json" => Some(ColumnType::Json),
        "unknown" => Some(ColumnType::Unknown),
        _ => None,
    }
}

/// Convert a declared type reference to its stored `/type/...` catalog path.
///
/// A bare or qualified name (`email`, `chatwork/message`) is a definition reference and
/// canonicalizes under `/type`. Existing stored paths under `/type/...` are accepted for validator
/// compatibility with rows already materialized in the catalog. Other leading-slash paths are data
/// paths and are rejected by returning `None`.
#[must_use]
pub fn declared_type_path(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("/type/") {
        return Some(raw.to_string());
    }
    if raw.starts_with('/') {
        return None;
    }
    Some(format!("/type/{raw}"))
}

/// Whether a stored `/type/...` path shadows a base column-token name.
#[must_use]
pub fn type_name_shadows_base(path: &str) -> bool {
    path.strip_prefix("/type/")
        .filter(|name| !name.contains('/'))
        .and_then(canonical_base_column_type)
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_type_spellings_canonicalize_only_the_canonical_vocabulary() {
        assert_eq!(canonical_base_column_type("TEXT"), Some("text"));
        assert_eq!(canonical_base_column_type("string"), None);
        assert_eq!(canonical_base_column_type("varchar"), None);
        assert_eq!(canonical_base_column_type("integer"), None);
        assert_eq!(canonical_base_column_type("jsonb"), None);
        assert_eq!(canonical_base_column_type("frobnitz"), None);
    }

    #[test]
    fn base_type_spellings_parse_to_column_types() {
        assert_eq!(base_column_type("bool"), Some(ColumnType::Bool));
        assert_eq!(base_column_type("int"), Some(ColumnType::Int));
        assert_eq!(base_column_type("bytes"), Some(ColumnType::Bytes));
        assert_eq!(base_column_type("unknown"), Some(ColumnType::Unknown));
        assert_eq!(base_column_type("customer"), None);
    }

    #[test]
    fn declared_type_references_canonicalize_to_catalog_paths() {
        assert_eq!(declared_type_path("email").as_deref(), Some("/type/email"));
        assert_eq!(
            declared_type_path("chatwork/message").as_deref(),
            Some("/type/chatwork/message")
        );
        assert_eq!(
            declared_type_path("/type/email").as_deref(),
            Some("/type/email")
        );
        assert_eq!(declared_type_path("/sql/shop/customers"), None);
        assert_eq!(declared_type_path(""), None);
    }

    #[test]
    fn base_type_shadow_detection_uses_the_same_surface_vocabulary() {
        assert!(type_name_shadows_base("/type/text"));
        assert!(!type_name_shadows_base("/type/string"));
        assert!(!type_name_shadows_base("/type/chatwork/message"));
        assert!(!type_name_shadows_base("/sql/text"));
    }
}
