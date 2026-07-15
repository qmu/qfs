//! The transform **cardinality mode** — a total function of a transform definition's declared
//! INPUT [`Schema`] (blueprint §15, decision W). A `|> transform <def>` stage calls a model over
//! rows; how the upstream relation is fed to the model is decided ENTIRELY by the shape the
//! definition declares for its input, so the mode can never be an independent, drift-prone flag —
//! it is *derived*, here, once.
//!
//! Three modes, in order of the input shape that selects them (the first match wins):
//! - a single `bytes` column ⇒ [`TransformMode::Extraction`] (one blob in, structured rows out);
//! - a single `array<struct<…>>` column ⇒ [`TransformMode::RelationWise`] (the whole nested
//!   relation is handed to the model at once);
//! - anything else — including a single `text` column, or several columns ⇒
//!   [`TransformMode::RowWise`] (the model is called per upstream row).
//!
//! An empty INPUT declares no shape at all, so it has no mode: [`derive_mode`] returns
//! [`ModeError::EmptyInput`] rather than inventing one (no "ambiguous mode" is representable). The
//! function is pure — no I/O, no secret, no provider — so it lives in the leaf type crate beside
//! [`Schema`]/[`ColumnType`] and is reused unchanged by the plan spine and the executor.

use std::collections::BTreeMap;

use crate::schema::{ColumnType, Schema};

/// A **resolved** transform definition as the pure plan spine consumes it (blueprint §15): the
/// declared INPUT/OUTPUT [`Schema`]s and the derived [`TransformMode`], plus the NON-SECRET
/// provider/model/effort selectors (shown in a PREVIEW's spend-legibility line — a selector names
/// WHICH model runs, never a credential; the secret REFERENCE stays executor-side). The lowering
/// (`qfs-pushdown`) and the schema fold (`qfs-core`) both read this, so it lives in the leaf type
/// crate; the binary builds it from a stored definition and injects a [`TransformDefs`] map at
/// plan time.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTransform {
    /// The declared INPUT schema (drives the mode + the fold's input-column matching).
    pub input: Schema,
    /// The declared OUTPUT schema (the shape the fold exposes to downstream stages).
    pub output: Schema,
    /// The derived cardinality mode.
    pub mode: TransformMode,
    /// The model provider selector (a label, never a credential). Empty when unwired (tests).
    pub provider: String,
    /// The model name/id the provider is asked for. Empty when unwired (tests).
    pub model: String,
    /// The optional effort/budget hint.
    pub effort: Option<String>,
}

impl ResolvedTransform {
    /// Build a resolved definition from its declared INPUT/OUTPUT schemas, deriving the mode.
    /// The provider/model selectors start empty; attach them with [`Self::with_model_meta`].
    ///
    /// # Errors
    /// [`ModeError::EmptyInput`] if `input` declares no columns.
    pub fn new(input: Schema, output: Schema) -> Result<Self, ModeError> {
        let mode = derive_mode(&input)?;
        Ok(Self {
            input,
            output,
            mode,
            provider: String::new(),
            model: String::new(),
            effort: None,
        })
    }

    /// Attach the non-secret provider/model/effort selectors (the PREVIEW spend-legibility
    /// metadata — never a credential).
    #[must_use]
    pub fn with_model_meta(
        mut self,
        provider: impl Into<String>,
        model: impl Into<String>,
        effort: Option<String>,
    ) -> Self {
        self.provider = provider.into();
        self.model = model.into();
        self.effort = effort;
        self
    }
}

/// A name→[`ResolvedTransform`] map — the plan-time resolution surface a `|> transform <name>`
/// stage looks a definition up in. Empty when no definitions are wired (a pure/test path with no
/// System DB), in which case a transform stage lowers/folds to a structured "unresolved" error.
pub type TransformDefs = BTreeMap<String, ResolvedTransform>;

/// How a `|> transform <def>` stage feeds its upstream relation to the model — a **total
/// function** of the definition's declared INPUT shape ([`derive_mode`]). A closed set: a new mode
/// would be a new declared input shape, never a free-floating flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformMode {
    /// Single `bytes` input column: one opaque blob in, structured OUTPUT rows out (e.g. an
    /// attachment → line items). The model is called once per upstream row's blob.
    Extraction,
    /// Single `array<struct<…>>` input column: the whole nested relation is handed to the model
    /// at once (a relation-in, relation-out transform).
    RelationWise,
    /// The default: the model is called once per upstream row, its columns bound to the declared
    /// INPUT. Selected by a single `text` column, or any multi-column input.
    RowWise,
}

impl TransformMode {
    /// The stable lowercase token for this mode (the value the `/transform` describe surface and
    /// the stored row carry — one source of truth, never a re-spelled string).
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Extraction => "extraction",
            Self::RelationWise => "relation-wise",
            Self::RowWise => "row-wise",
        }
    }
}

/// Why a declared INPUT shape has no derivable mode. The only failure is an empty input — every
/// non-empty shape maps to exactly one [`TransformMode`], so the derivation is otherwise total.
///
/// `qfs-types` is a leaf (no `thiserror`, like [`TypeError`](crate::TypeError)): this is its own
/// owned enum with a hand-written [`Display`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModeError {
    /// The declared INPUT schema has no columns, so it declares no shape to feed the model.
    EmptyInput,
}

impl core::fmt::Display for ModeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "a transform INPUT must declare at least one column"),
        }
    }
}

impl std::error::Error for ModeError {}

/// Derive the [`TransformMode`] from a definition's declared INPUT [`Schema`] — the total function
/// at the heart of the transform semantics (blueprint §15). The first matching shape wins:
///
/// 1. empty input ⇒ [`ModeError::EmptyInput`] (no shape ⇒ no mode);
/// 2. single `bytes` column ⇒ [`TransformMode::Extraction`];
/// 3. single `array<struct<…>>` column ⇒ [`TransformMode::RelationWise`];
/// 4. everything else (single `text`, single other scalar, or multi-column) ⇒
///    [`TransformMode::RowWise`].
///
/// # Errors
/// [`ModeError::EmptyInput`] when `input` has no columns.
pub fn derive_mode(input: &Schema) -> Result<TransformMode, ModeError> {
    match input.columns.as_slice() {
        [] => Err(ModeError::EmptyInput),
        [only] => Ok(match &only.ty {
            ColumnType::Bytes => TransformMode::Extraction,
            // A single `array<struct<…>>` is the relation-wise shape. An array of anything else
            // (or of an unresolved element) is NOT relation-wise — it falls through to row-wise.
            ColumnType::Array(elem) if matches!(elem.as_ref(), ColumnType::Struct(_)) => {
                TransformMode::RelationWise
            }
            _ => TransformMode::RowWise,
        }),
        // More than one column always feeds the model per-row (its columns bound to the input).
        _ => Ok(TransformMode::RowWise),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Column;

    fn schema(cols: Vec<Column>) -> Schema {
        Schema::new(cols)
    }

    #[test]
    fn single_bytes_is_extraction() {
        let s = schema(vec![Column::new("blob", ColumnType::Bytes, false)]);
        assert_eq!(derive_mode(&s).unwrap(), TransformMode::Extraction);
    }

    #[test]
    fn single_array_of_struct_is_relation_wise() {
        let elem = ColumnType::Struct(Schema::new(vec![Column::new(
            "sku",
            ColumnType::Text,
            false,
        )]));
        let s = schema(vec![Column::new(
            "rows",
            ColumnType::Array(Box::new(elem)),
            false,
        )]);
        assert_eq!(derive_mode(&s).unwrap(), TransformMode::RelationWise);
    }

    #[test]
    fn single_text_is_row_wise_not_extraction() {
        // The spec's explicit carve-out: a single `text` column is row-wise, NOT a special mode.
        let s = schema(vec![Column::new("body", ColumnType::Text, false)]);
        assert_eq!(derive_mode(&s).unwrap(), TransformMode::RowWise);
    }

    #[test]
    fn array_of_scalar_is_row_wise_not_relation_wise() {
        // Only array<struct> is relation-wise; array<text> falls through to row-wise.
        let s = schema(vec![Column::new(
            "tags",
            ColumnType::Array(Box::new(ColumnType::Text)),
            false,
        )]);
        assert_eq!(derive_mode(&s).unwrap(), TransformMode::RowWise);
    }

    #[test]
    fn multi_column_is_row_wise() {
        let s = schema(vec![
            Column::new("subject", ColumnType::Text, false),
            Column::new("body", ColumnType::Text, false),
        ]);
        assert_eq!(derive_mode(&s).unwrap(), TransformMode::RowWise);
    }

    #[test]
    fn empty_input_has_no_mode() {
        assert_eq!(derive_mode(&Schema::empty()), Err(ModeError::EmptyInput));
    }

    #[test]
    fn column_type_parse_round_trips_the_canonical_grammar() {
        assert_eq!(ColumnType::parse("text"), Some(ColumnType::Text));
        assert_eq!(ColumnType::parse("bytes"), Some(ColumnType::Bytes));
        assert_eq!(
            ColumnType::parse("array<text>"),
            Some(ColumnType::Array(Box::new(ColumnType::Text)))
        );
        // The relation-wise shape rehydrates to Array(Struct(_)) and derives relation-wise.
        let ty = ColumnType::parse("array<struct<sku:text,qty:int>>").unwrap();
        let s = schema(vec![Column::new("rows", ty, false)]);
        assert_eq!(derive_mode(&s).unwrap(), TransformMode::RelationWise);
        // Empty struct and nested commas are handled.
        assert_eq!(
            ColumnType::parse("struct<>"),
            Some(ColumnType::Struct(Schema::empty()))
        );
        assert!(ColumnType::parse("array<struct<a:int,b:array<text>>>").is_some());
        // Unknown tokens fail (the caller turns that into a structured error).
        assert_eq!(ColumnType::parse("string"), None);
        assert_eq!(ColumnType::parse("frobnitz"), None);
        assert_eq!(ColumnType::parse("array<nope>"), None);
    }

    #[test]
    fn mode_tokens_are_stable() {
        assert_eq!(TransformMode::Extraction.token(), "extraction");
        assert_eq!(TransformMode::RelationWise.token(), "relation-wise");
        assert_eq!(TransformMode::RowWise.token(), "row-wise");
    }
}
