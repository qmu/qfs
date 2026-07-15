//! The static type model: [`ColumnType`], [`Column`], [`Schema`], [`Provenance`],
//! and the pure schema algebra (`project`, `resolve_path`, `expand`). `unify` lives
//! in its own module ([`crate::unify`]) because the widening matrix is large enough
//! to warrant one concept per module (blueprint §11 coding standards).

use serde::{Deserialize, Serialize};

use crate::error::TypeError;

/// An identifier name — a column name or a path segment. Owned text; resolution of
/// names against a registry is a later semantic concern (blueprint §3), never grammar.
pub type Name = String;

/// A driver identity used only for [`Provenance`]. An **owned** newtype (never a
/// vendor handle, never a token) so the type model stays a true leaf and carries no
/// credentials (blueprint §8). Defined here rather than imported from `qfs-driver` so
/// `qfs-types` remains the lowest crate in the spine.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DriverId(pub String);

impl DriverId {
    /// Construct a driver id from owned text.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The driver id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a column came from (blueprint §6/§8). Carries the originating [`DriverId`] and the
/// backend's source column name — **never** a secret or capability. Used for audit and
/// for `unify` provenance when columns from divergent sources merge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The driver that produced this column, if known.
    pub driver: Option<DriverId>,
    /// The backend's original column name, if it differed from [`Column::name`].
    pub source_col: Option<String>,
}

impl Provenance {
    /// An empty provenance (no driver, no source column) — for synthetic columns
    /// (e.g. `VALUES`, `EXTEND`).
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// The type of a column (blueprint §4). A sum type over scalars, nested `Struct`/`Array`,
/// an open `Json` escape for deeply-irregular data, and `Unknown` for
/// inferred-but-unresolved columns from sparse heterogeneous sources.
///
/// Exhaustively matched everywhere (no catch-all) so a new variant forces every
/// consumer to consider it (blueprint §11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ColumnType {
    /// A boolean.
    Bool,
    /// A 64-bit signed integer.
    Int,
    /// A 64-bit float.
    Float,
    /// An arbitrary-precision decimal (carried as text at runtime).
    Decimal,
    /// UTF-8 text.
    Text,
    /// Opaque bytes (e.g. a mail attachment blob).
    Bytes,
    /// A timestamp (epoch-based at runtime).
    Timestamp,
    /// A calendar date.
    Date,
    /// A UUID.
    Uuid,
    /// A nested record; path access `a.b.c` walks this (blueprint §4 no flattening).
    Struct(Schema),
    /// A homogeneous collection; the `EXPAND` target (blueprint §4).
    Array(Box<ColumnType>),
    /// Deeply-irregular JSON kept as a single column (blueprint §4); queryable late-bound.
    Json,
    /// Inferred but unresolved — a sparse column from a heterogeneous source, or a
    /// path navigated into a `Json` column. Still queryable; not a hard error.
    Unknown,
}

impl ColumnType {
    /// The canonical **§5 type token** — the lowercase name the result envelope's `schema`
    /// carries (ticket 20260703150300, blueprint §14). One source of truth for the agent-facing
    /// type name: the same lowercase tokens the `CREATE TABLE` / `CREATE TYPE` literal grammar
    /// and `typeck` accept (`text`/`int`/`timestamp`/…). Nested types collapse to their family
    /// token (`struct`/`array`/`json`); an unresolved column is honestly `"unknown"`.
    #[must_use]
    pub fn type_token(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Decimal => "decimal",
            Self::Text => "text",
            Self::Bytes => "bytes",
            Self::Timestamp => "timestamp",
            Self::Date => "date",
            Self::Uuid => "uuid",
            Self::Struct(_) => "struct",
            Self::Array(_) => "array",
            Self::Json => "json",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a canonical type string — the inverse of a describe/definition round-trip — into a
    /// [`ColumnType`]. The grammar is the §5 lowercase tokens (`text`/`int`/`bytes`/…) plus two
    /// recursive forms: `array<ELEM>` and `struct<name:TYPE,name:TYPE>` (`struct<>` is the empty
    /// record). Used to rehydrate a stored transform-definition INPUT/OUTPUT schema (blueprint §15);
    /// the encoder is the `CREATE TRANSFORM` grammar, so the two agree on this one format.
    ///
    /// Returns `None` for an unrecognised token or a malformed nested form — the caller decides
    /// whether that is a structured error (a stored definition should always round-trip).
    #[must_use]
    pub fn parse(s: &str) -> Option<ColumnType> {
        let s = s.trim();
        match s {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "decimal" => Some(Self::Decimal),
            "text" => Some(Self::Text),
            "bytes" => Some(Self::Bytes),
            "timestamp" => Some(Self::Timestamp),
            "date" => Some(Self::Date),
            "uuid" => Some(Self::Uuid),
            "json" => Some(Self::Json),
            "unknown" => Some(Self::Unknown),
            _ => {
                if let Some(inner) = s.strip_prefix("array<").and_then(|r| r.strip_suffix('>')) {
                    return Some(Self::Array(Box::new(Self::parse(inner)?)));
                }
                if let Some(fields) = s.strip_prefix("struct<").and_then(|r| r.strip_suffix('>')) {
                    return Some(Self::Struct(Schema::new(parse_struct_fields(fields)?)));
                }
                None
            }
        }
    }

    /// Whether this is a scalar (non-nested, non-`Json`, non-`Unknown`) type.
    #[must_use]
    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            Self::Bool
                | Self::Int
                | Self::Float
                | Self::Decimal
                | Self::Text
                | Self::Bytes
                | Self::Timestamp
                | Self::Date
                | Self::Uuid
        )
    }
}

/// One named, typed column of a [`Schema`] (blueprint §4/§6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    /// The column name.
    pub name: Name,
    /// The column type.
    pub ty: ColumnType,
    /// Whether the column admits `Null` (orthogonal to `ty`, blueprint §4).
    pub nullable: bool,
    /// Where the column came from (driver id + source name; no secrets).
    pub provenance: Provenance,
}

impl Column {
    /// Construct a column with empty provenance.
    #[must_use]
    pub fn new(name: impl Into<Name>, ty: ColumnType, nullable: bool) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable,
            provenance: Provenance::none(),
        }
    }

    /// Builder: attach provenance.
    #[must_use]
    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = provenance;
        self
    }
}

/// An ordered, named, typed set of columns — the static description of a relation
/// (blueprint §4/§6). [`Row`](crate::Row) values are positional and aligned to
/// [`Schema::columns`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    /// The columns, in order.
    pub columns: Vec<Column>,
}

impl Schema {
    /// An empty schema (no columns).
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Construct a schema from an explicit column list.
    #[must_use]
    pub fn new(columns: Vec<Column>) -> Self {
        Self { columns }
    }

    /// Look up a column by name (the first match; names are conventionally unique).
    #[must_use]
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// The column names, in order (used in [`TypeError::UnknownColumn`] context).
    #[must_use]
    pub fn column_names(&self) -> Vec<Name> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }

    /// Project to a subset of columns, preserving the requested order (blueprint §4
    /// `SELECT`). Errors with [`TypeError::UnknownColumn`] (carrying the available
    /// names) on the first name that is not present.
    ///
    /// # Errors
    /// [`TypeError::UnknownColumn`] if any requested name is absent.
    pub fn project(&self, names: &[Name]) -> Result<Schema, TypeError> {
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let col = self.column(name).ok_or_else(|| TypeError::UnknownColumn {
                name: name.clone(),
                available: self.column_names(),
            })?;
            out.push(col.clone());
        }
        Ok(Schema::new(out))
    }

    /// Concatenate two schemas for a `JOIN`, applying a **column-collision policy**
    /// (O-t07-2): the left schema's columns come first, then the right's; a right
    /// column whose name already exists on the left is **disambiguated** by prefixing
    /// its provenance driver id (`<driver>.<name>`), or a positional `r.<name>` suffix
    /// when no driver provenance is available, so the joined schema never silently
    /// shadows a left column behind a right one of the same name.
    ///
    /// This is the structural counterpart to the t07 evaluator's raw column concat
    /// (which dropped the collision question): a federated `JOIN` of two sources that
    /// both expose `id`/`name` produces a schema where both sides remain addressable.
    /// Nullability and types are preserved per side (a `JOIN` does not widen).
    #[must_use]
    pub fn join(&self, rhs: &Schema) -> Schema {
        let mut out: Vec<Column> = self.columns.clone();
        let left_names: std::collections::BTreeSet<&str> =
            self.columns.iter().map(|c| c.name.as_str()).collect();
        for c in &rhs.columns {
            if left_names.contains(c.name.as_str()) {
                let qualifier = c
                    .provenance
                    .driver
                    .as_ref()
                    .map(|d| d.as_str().to_string())
                    .unwrap_or_else(|| "r".to_string());
                let mut renamed = c.clone();
                renamed.name = format!("{qualifier}.{}", c.name);
                out.push(renamed);
            } else {
                out.push(c.clone());
            }
        }
        Schema::new(out)
    }

    /// Resolve a dotted path `a.b.c` to its nested type, walking `Struct` columns
    /// **without flattening** (blueprint §4 path access). A path that descends into a
    /// `Json` column yields [`ColumnType::Unknown`] (late-bound, still queryable), not
    /// a hard error.
    ///
    /// # Errors
    /// - [`TypeError::UnknownColumn`] if the head segment is absent.
    /// - [`TypeError::NotAStruct`] if an intermediate segment is not a `Struct`/`Json`.
    pub fn resolve_path(&self, path: &[Name]) -> Result<ColumnType, TypeError> {
        let Some((head, rest)) = path.split_first() else {
            // An empty path resolves to the whole relation as an anonymous struct.
            return Ok(ColumnType::Struct(self.clone()));
        };
        let col = self.column(head).ok_or_else(|| TypeError::UnknownColumn {
            name: head.clone(),
            available: self.column_names(),
        })?;
        resolve_in_type(&col.ty, head, rest)
    }

    /// Explode a nested collection column into rows (blueprint §4 `EXPAND`, "same operator
    /// for mail attachments and JSON arrays"). The named field must be a collection:
    /// - `Array(T)` → the field column is replaced by a column of element type `T`
    ///   (when `T` is a `Struct`, its fields are flattened into the row columns).
    /// - `Struct(s)` → the field column is replaced by `s`'s columns flattened in.
    ///
    /// Other columns are preserved in place. Expanding a scalar / `Json` / `Unknown`
    /// column is rejected.
    ///
    /// # Errors
    /// - [`TypeError::UnknownColumn`] if `field` is absent.
    /// - [`TypeError::NotExpandable`] if `field` is not an `Array`/`Struct`.
    pub fn expand(&self, field: &Name) -> Result<Schema, TypeError> {
        let idx = self
            .columns
            .iter()
            .position(|c| &c.name == field)
            .ok_or_else(|| TypeError::UnknownColumn {
                name: field.clone(),
                available: self.column_names(),
            })?;

        let target = &self.columns[idx];
        let replacement: Vec<Column> = match &target.ty {
            ColumnType::Array(elem) => match elem.as_ref() {
                // Array of structs: flatten the element struct's columns in place.
                ColumnType::Struct(inner) => inner.columns.clone(),
                // Array of scalars/other: one column of the element type, same name,
                // no longer nullable-by-collection (an exploded element is present).
                other => vec![Column::new(field.clone(), other.clone(), false)
                    .with_provenance(target.provenance.clone())],
            },
            // Struct field: flatten its columns in place (de-nesting one level).
            ColumnType::Struct(inner) => inner.columns.clone(),
            other => {
                return Err(TypeError::NotExpandable {
                    field: field.clone(),
                    ty: other.clone(),
                })
            }
        };

        let mut out = Vec::with_capacity(self.columns.len() + replacement.len());
        out.extend_from_slice(&self.columns[..idx]);
        out.extend(replacement);
        out.extend_from_slice(&self.columns[idx + 1..]);
        Ok(Schema::new(out))
    }
}

/// Parse the body of a `struct<…>` type (the text between the angle brackets) into its columns.
/// Fields are `name:TYPE`, comma-separated at the TOP level (a comma inside a nested `<…>` does not
/// split), so `struct<sku:text,rows:array<struct<x:int>>>` parses correctly. `struct<>` (empty
/// body) is the empty record. Returns `None` on a malformed field. All fields are nullable
/// (a definition schema declares shape, not null-strictness, at this layer).
fn parse_struct_fields(body: &str) -> Option<Vec<Column>> {
    let body = body.trim();
    if body.is_empty() {
        return Some(Vec::new());
    }
    let mut cols = Vec::new();
    for field in split_top_level_commas(body) {
        let (name, ty) = field.split_once(':')?;
        cols.push(Column::new(
            name.trim(),
            ColumnType::parse(ty.trim())?,
            true,
        ));
    }
    Some(cols)
}

/// Split `s` on top-level commas — a comma nested inside `<…>` is NOT a separator. Used to break a
/// `struct<…>` field list without a full tokenizer.
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '<' => {
                depth += 1;
                cur.push(ch);
            }
            '>' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Walk the remaining path segments into a (possibly nested) type. Shared by
/// [`Schema::resolve_path`].
fn resolve_in_type(ty: &ColumnType, seg: &str, rest: &[Name]) -> Result<ColumnType, TypeError> {
    match rest.split_first() {
        None => Ok(ty.clone()),
        Some((next, tail)) => match ty {
            ColumnType::Struct(inner) => {
                let col = inner.column(next).ok_or_else(|| TypeError::UnknownColumn {
                    name: next.clone(),
                    available: inner.column_names(),
                })?;
                resolve_in_type(&col.ty, next, tail)
            }
            // Descending into deeply-irregular JSON is late-bound: every further
            // segment resolves to Unknown rather than failing (blueprint §4).
            ColumnType::Json | ColumnType::Unknown => Ok(ColumnType::Unknown),
            other => Err(TypeError::NotAStruct {
                segment: seg.to_string(),
                ty: other.clone(),
            }),
        },
    }
}

/// A relation node that carries a resolved [`Schema`] (blueprint §6). Surface only; impls
/// (effect-plan nodes, driver relations) land in E2/E4.
pub trait Typed {
    /// The output schema of this relation node.
    fn schema(&self) -> &Schema;
}

/// The single effectful seam of the type model: a backend that can describe the
/// schema at a logical path (blueprint §6 "driver declares schema; powers `DESCRIBE`").
///
/// **Surface only** — real impls live in E4 drivers. The path is a logical segment
/// list ([`Name`]s) rather than the driver `Path` type so `qfs-types` stays a leaf
/// (no dependency on `qfs-driver`); E4 adapts the driver `Path` into segments at the
/// boundary.
pub trait SchemaSource {
    /// Describe the schema rooted at `path` (a logical segment list).
    ///
    /// # Errors
    /// [`TypeError`] if the path does not resolve to a describable node.
    fn describe(&self, path: &[Name]) -> Result<Schema, TypeError>;
}
