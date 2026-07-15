//! The owned transform **definition** (blueprint §15, decision W) and its resolution — the typed
//! record a `CREATE TRANSFORM` declaration stores and a `|> transform <name>` stage resolves. The
//! definition is DATA: a name, the declared INPUT/OUTPUT [`Schema`]s (reusing `qfs_types`, never a
//! parallel schema language), the provider/model/effort selectors, and a secret REFERENCE only
//! (`env:`/`vault:`, resolved lazily at COMMIT by a later ticket — never here, never at DESCRIBE).
//!
//! The cardinality MODE is not a field — it is DERIVED from `input` by the total function
//! [`qfs_types::derive_mode`] ([`TransformDef::mode`]), so a stored flag can never drift from the
//! declared shape. This module owns the definition type + its validation; the plan spine and the
//! executor (the sibling tickets) consume [`TransformDef`] through this one resolution surface.

use serde::Deserialize;

use qfs_types::{derive_mode, Column, ColumnType, ModeError, Schema, TransformMode};

/// A resolved transform definition — the typed form of one `sys_transforms` row (the storage) /
/// one `CREATE TRANSFORM` declaration (the surface). Immutable once built; every field is
/// non-secret (a `secret_ref` is a REFERENCE, never a token).
#[derive(Debug, Clone, PartialEq)]
pub struct TransformDef {
    /// The definition name — the `/transform/<name>` segment and the `|> transform <name>` target.
    pub name: String,
    /// The declared INPUT schema (drives the derived [`TransformMode`]).
    pub input: Schema,
    /// The declared OUTPUT schema (the shape a downstream stage type-checks against).
    pub output: Schema,
    /// The model provider selector (never a token).
    pub provider: String,
    /// The model name/id the provider is asked for.
    pub model: String,
    /// The optional effort/budget hint.
    pub effort: Option<String>,
    /// The optional secret REFERENCE (`env:<VAR>` / `vault:<path>`) — resolved lazily at COMMIT.
    pub secret_ref: Option<String>,
}

/// Why a stored/declared transform definition is invalid — a structured, secret-free error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformDefError {
    /// The declared INPUT schema has no columns (no shape ⇒ no mode).
    EmptyInput,
    /// The declared OUTPUT schema has no columns (nothing for the model to produce).
    EmptyOutput,
    /// A `SECRET` value that is not an `env:`/`vault:` reference (an inline secret is forbidden).
    /// Carries the offending SCHEME only — never the value (which never reaches here anyway).
    InlineSecret,
    /// The stored INPUT/OUTPUT column-descriptor JSON was malformed or named an unknown type.
    MalformedSchema {
        /// Which schema failed (`"input"` / `"output"`).
        field: &'static str,
        /// A secret-free reason.
        detail: String,
    },
}

impl core::fmt::Display for TransformDefError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "a transform INPUT must declare at least one column"),
            Self::EmptyOutput => write!(f, "a transform OUTPUT must declare at least one column"),
            Self::InlineSecret => write!(
                f,
                "a transform SECRET must be an `env:`/`vault:` reference, never an inline value"
            ),
            Self::MalformedSchema { field, detail } => {
                write!(f, "malformed transform {field} schema: {detail}")
            }
        }
    }
}

impl std::error::Error for TransformDefError {}

impl TransformDef {
    /// Build and validate a definition from already-typed schemas. Rejects an empty INPUT or
    /// OUTPUT, and a `secret_ref` that is not an `env:`/`vault:` reference.
    ///
    /// # Errors
    /// [`TransformDefError`] on an empty input/output or an inline (non-reference) secret.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        input: Schema,
        output: Schema,
        provider: impl Into<String>,
        model: impl Into<String>,
        effort: Option<String>,
        secret_ref: Option<String>,
    ) -> Result<Self, TransformDefError> {
        if input.columns.is_empty() {
            return Err(TransformDefError::EmptyInput);
        }
        if output.columns.is_empty() {
            return Err(TransformDefError::EmptyOutput);
        }
        if let Some(s) = &secret_ref {
            if !is_secret_reference(s) {
                return Err(TransformDefError::InlineSecret);
            }
        }
        Ok(Self {
            name: name.into(),
            input,
            output,
            provider: provider.into(),
            model: model.into(),
            effort,
            secret_ref,
        })
    }

    /// Build a definition from a stored `sys_transforms` row, where `input`/`output` are the
    /// column-descriptor JSON the `CREATE TRANSFORM` grammar emitted.
    ///
    /// # Errors
    /// [`TransformDefError`] on malformed schema JSON or the same validation as [`TransformDef::new`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_stored(
        name: impl Into<String>,
        input_json: &str,
        output_json: &str,
        provider: impl Into<String>,
        model: impl Into<String>,
        effort: Option<String>,
        secret_ref: Option<String>,
    ) -> Result<Self, TransformDefError> {
        let input = decode_schema_json(input_json).map_err(|detail| {
            TransformDefError::MalformedSchema {
                field: "input",
                detail,
            }
        })?;
        let output = decode_schema_json(output_json).map_err(|detail| {
            TransformDefError::MalformedSchema {
                field: "output",
                detail,
            }
        })?;
        Self::new(name, input, output, provider, model, effort, secret_ref)
    }

    /// The derived cardinality [`TransformMode`] — the total function of the declared INPUT shape.
    /// `new`/`from_stored` guarantee a non-empty input, so this never errors in practice; the
    /// `Result` keeps the derivation honest rather than panicking.
    ///
    /// # Errors
    /// [`ModeError::EmptyInput`] only if the input was somehow emptied after construction.
    pub fn mode(&self) -> Result<TransformMode, ModeError> {
        derive_mode(&self.input)
    }
}

/// The derived cardinality [`TransformMode`] of a stored INPUT descriptor JSON alone. The mode is a
/// pure function of the INPUT shape, so a mode-only reader (e.g. the `/transform` scan's DERIVED
/// `mode` column) need not decode or validate the OUTPUT. `None` on malformed JSON, an unknown
/// column type, or an empty column list.
#[must_use]
pub fn derived_mode_of_stored_input(input_json: &str) -> Option<TransformMode> {
    decode_schema_json(input_json)
        .ok()
        .and_then(|s| derive_mode(&s).ok())
}

/// Whether `s` is a secret REFERENCE (`env:<VAR>` / `vault:<path>`) rather than an inline value.
/// The scheme list is duplicated in the `CREATE TRANSFORM` parse gate
/// (`qfs-parser`'s `grammar.rs`, which cannot depend on this crate) — keep the two in sync.
#[must_use]
pub fn is_secret_reference(s: &str) -> bool {
    s.starts_with("env:") || s.starts_with("vault:")
}

/// One `{ "name", "type", "nullable" }` column descriptor — the stored INPUT/OUTPUT JSON shape (the
/// CREATE TABLE `columns` convention). `type` is the canonical string `ColumnType::parse` reads.
#[derive(Deserialize)]
struct ColDesc {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default = "default_nullable")]
    nullable: bool,
}

const fn default_nullable() -> bool {
    true
}

/// Decode a column-descriptor JSON array into a typed [`Schema`], rehydrating each column type via
/// [`ColumnType::parse`]. Returns a secret-free reason string on malformed JSON / an unknown type.
fn decode_schema_json(json: &str) -> Result<Schema, String> {
    let cols: Vec<ColDesc> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(cols.len());
    for c in cols {
        let ty =
            ColumnType::parse(&c.ty).ok_or_else(|| format!("unknown column type `{}`", c.ty))?;
        out.push(Column::new(c.name, ty, c.nullable));
    }
    Ok(Schema::new(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_json(name: &str, ty: &str) -> String {
        format!("[{{\"name\":\"{name}\",\"type\":\"{ty}\",\"nullable\":true}}]")
    }

    #[test]
    fn from_stored_row_wise_definition_round_trips() {
        let def = TransformDef::from_stored(
            "classify",
            &scalar_json("body", "text"),
            &scalar_json("label", "text"),
            "claude",
            "claude-sonnet-5",
            Some("medium".into()),
            Some("vault:models/key".into()),
        )
        .unwrap();
        assert_eq!(def.mode().unwrap(), TransformMode::RowWise);
        assert_eq!(def.input.columns.len(), 1);
        assert_eq!(def.output.columns.len(), 1);
    }

    #[test]
    fn extraction_and_relation_wise_modes_derive_from_input() {
        let extraction = TransformDef::from_stored(
            "extract",
            &scalar_json("blob", "bytes"),
            &scalar_json("line", "text"),
            "p",
            "m",
            None,
            None,
        )
        .unwrap();
        assert_eq!(extraction.mode().unwrap(), TransformMode::Extraction);

        let relation = TransformDef::from_stored(
            "rollup",
            "[{\"name\":\"rows\",\"type\":\"array<struct<sku:text>>\",\"nullable\":false}]",
            &scalar_json("total", "int"),
            "p",
            "m",
            None,
            None,
        )
        .unwrap();
        assert_eq!(relation.mode().unwrap(), TransformMode::RelationWise);
    }

    #[test]
    fn empty_input_or_output_is_rejected() {
        assert_eq!(
            TransformDef::from_stored("t", "[]", &scalar_json("x", "text"), "p", "m", None, None),
            Err(TransformDefError::EmptyInput)
        );
        assert_eq!(
            TransformDef::from_stored("t", &scalar_json("x", "text"), "[]", "p", "m", None, None),
            Err(TransformDefError::EmptyOutput)
        );
    }

    #[test]
    fn an_inline_secret_is_rejected_only_a_reference_is_accepted() {
        assert_eq!(
            TransformDef::new(
                "t",
                Schema::new(vec![Column::new("x", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("y", ColumnType::Text, true)]),
                "p",
                "m",
                None,
                Some("sk-abc123".into()),
            ),
            Err(TransformDefError::InlineSecret)
        );
        assert!(is_secret_reference("env:MODEL_KEY"));
        assert!(is_secret_reference("vault:models/key"));
        assert!(!is_secret_reference("sk-abc123"));
    }

    #[test]
    fn malformed_schema_json_is_a_structured_error() {
        let err = TransformDef::from_stored(
            "t",
            "not json",
            &scalar_json("y", "text"),
            "p",
            "m",
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TransformDefError::MalformedSchema { field: "input", .. }
        ));
    }
}
