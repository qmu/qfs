//! Declare-time well-formedness for a `CREATE TYPE` **refinement predicate** (blueprint §5.4).
//!
//! A named type may carry an optional `WHERE <pred>` — a **row-local, pure, total, boolean**
//! predicate. It is NOT proof-carrying refinement: it is contract-checked like a `CHECK`
//! constraint. This module owns the DECLARE-time half — the check that runs when a `CREATE TYPE`
//! is stored, so a malformed refinement fails at CREATE and never at first write. (The enforcement
//! half — per-row MEMBERSHIP at the write/`OF` boundary — is [`crate::membership`].)
//!
//! The stored body is the JSON OBJECT the `CREATE TYPE` desugar emits:
//! `{"columns":[{name,type,…}],"where":<Expr|null>}`. The validator rehydrates the declared columns
//! into a [`Schema`] (rejecting an `unknown`-typed column — a declaration is a contract, so a
//! late-bound column type is not a legal *declaration*), then, if a `where` predicate is present,
//! checks it is row-local (no aggregate / table-valued / context built-in, no cross-relation /
//! subquery reference), every column it names is declared, and its result type is boolean.
//!
//! Mirrors [`crate::ddl::transform`]: DATA in, a structured secret-free error out, no I/O.

use std::collections::HashSet;

use qfs_parser::ast::{Expr, FnRef};
use qfs_types::{base_column_type, declared_type_path, Column, ColumnType, DeclaredColumn, Schema};

use crate::stdlib::{context_builtin_names, BuiltinEval, StdlibRegistry};
use crate::typeck::{check_expr, TyEnv};

/// Why a declared type (its columns and optional refinement predicate) is malformed — a
/// structured, secret-free error surfaced at DECLARE time (blueprint §5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeDefError {
    /// The stored type body JSON was not the expected `{"columns":[…],"where":…}` object.
    MalformedBody {
        /// A secret-free reason.
        detail: String,
    },
    /// A declared column named a type the type system does not know.
    UnknownColumnType {
        /// The offending column name.
        column: String,
        /// The unrecognised type string.
        ty: String,
    },
    /// A declared type referenced from column position was not present in the `/type` catalog.
    UnknownDeclaredType {
        /// The offending column name.
        column: String,
        /// The unresolved declared-type path.
        ty: String,
    },
    /// A declared type tried to use itself, directly or through another type.
    RecursiveDeclaredType {
        /// The recursive declared-type path.
        ty: String,
    },
    /// A declared type with more than one column was used in a single-column type position.
    MultiColumnDeclaredType {
        /// The offending column name.
        column: String,
        /// The declared-type path.
        ty: String,
    },
    /// A declaration tried to define a type name that is reserved for the base column vocabulary.
    TypeNameShadowsBase {
        /// The rejected `/type/...` name.
        name: String,
    },
    /// A declared column was typed `unknown`. A declaration is a contract, so a late-bound
    /// (`unknown`) column type is not a legal declaration.
    UnknownTypedColumn {
        /// The offending column name.
        column: String,
    },
    /// The refinement predicate referenced a name that is not a declared column (and not a
    /// lambda parameter in scope).
    UnknownColumnRef {
        /// The unresolved reference.
        name: String,
    },
    /// The refinement predicate called a built-in that is not row-local pure — an aggregate, a
    /// table-valued/source built-in, or a context built-in (`NOW`/`CURRENT_DATE`/`LAST_RUN`/`env`).
    ImpureRefinement {
        /// The offending built-in name.
        name: String,
        /// The rejected category (`"aggregate"` / `"table-valued"` / `"context"`).
        category: &'static str,
    },
    /// The refinement predicate did not type-check to a boolean result.
    NonBooleanRefinement {
        /// The type the predicate produced instead of `bool`.
        found: String,
    },
    /// The refinement predicate failed the static type checker (an unknown function, a bad
    /// argument type, a comparison mismatch). Carries the checker's stable reason code.
    RefinementTypeError {
        /// The [`crate::EvalError::code`] of the underlying failure.
        code: &'static str,
    },
}

impl core::fmt::Display for TypeDefError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedBody { detail } => write!(f, "malformed CREATE TYPE body: {detail}"),
            Self::UnknownColumnType { column, ty } => {
                write!(f, "column `{column}` declares unknown type `{ty}`")
            }
            Self::UnknownDeclaredType { column, ty } => {
                write!(f, "column `{column}` declares unknown type `{ty}`")
            }
            Self::RecursiveDeclaredType { ty } => {
                write!(f, "declared type `{ty}` is recursive")
            }
            Self::MultiColumnDeclaredType { column, ty } => write!(
                f,
                "column `{column}` declares `{ty}`, but a declared type used as a column type must \
                 have exactly one structural column"
            ),
            Self::TypeNameShadowsBase { name } => write!(
                f,
                "declared type `{name}` shadows a base column type token"
            ),
            Self::UnknownTypedColumn { column } => write!(
                f,
                "column `{column}` is typed `unknown`; a declared type is a contract and cannot \
                 leave a column late-bound"
            ),
            Self::UnknownColumnRef { name } => write!(
                f,
                "the refinement predicate references `{name}`, which is not a declared column"
            ),
            Self::ImpureRefinement { name, category } => write!(
                f,
                "the refinement predicate calls `{name}` (a {category} built-in); a refinement must \
                 be row-local and pure"
            ),
            Self::NonBooleanRefinement { found } => write!(
                f,
                "the refinement predicate must be boolean, but it is `{found}`"
            ),
            Self::RefinementTypeError { code } => {
                write!(f, "the refinement predicate does not type-check ({code})")
            }
        }
    }
}

impl std::error::Error for TypeDefError {}

/// The decoded body of a `CREATE TYPE`: the declared columns + the optional refinement `Expr`.
#[derive(serde::Deserialize)]
struct TypeBody {
    #[serde(default)]
    columns: Vec<DeclaredColumn>,
    #[serde(default)]
    r#where: Option<Expr>,
}

/// A refinement inherited from a declared type used in column position (`email email`). The
/// `column` is the containing type/table column; `schema` and `predicate` are the referenced
/// single-column type's own membership check.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnRefinement {
    pub column: String,
    pub ty: String,
    pub schema: Schema,
    pub predicate: Expr,
}

/// A declared type body after resolving every named column type to its structural base.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTypeDef {
    pub columns: Vec<DeclaredColumn>,
    pub schema: Schema,
    pub refinement: Option<Expr>,
    pub column_refinements: Vec<ColumnRefinement>,
}

/// Validate a stored `CREATE TYPE` body at DECLARE time (blueprint §5.4). Rehydrates the declared
/// columns into a [`Schema`] (rejecting an `unknown`-typed column), and — when a refinement is
/// present — checks it is row-local (no aggregate / table-valued / context built-in, no
/// cross-relation reference), names only declared columns, and yields a boolean.
///
/// # Errors
/// [`TypeDefError`] for a malformed body, an unknown / `unknown`-typed column, an impure or
/// non-boolean refinement, an unknown column reference, or a static type error.
pub fn validate_type_def(body_json: &str, stdlib: &StdlibRegistry) -> Result<(), TypeDefError> {
    validate_type_def_with_catalog(body_json, stdlib, |_| None)
}

/// Validate a stored `CREATE TYPE` body with access to the declared-type catalog. The lookup takes
/// an absolute `/type/...` path and returns that type's stored body JSON when it exists.
///
/// # Errors
/// [`TypeDefError`] for malformed JSON, unknown / recursive named types, an `unknown` declaration,
/// an impure/non-boolean refinement, or a static type error.
pub fn validate_type_def_with_catalog<F>(
    body_json: &str,
    stdlib: &StdlibRegistry,
    lookup: F,
) -> Result<(), TypeDefError>
where
    F: Fn(&str) -> Option<String>,
{
    let resolved = resolve_type_def(body_json, lookup)?;

    // No refinement ⇒ the column contract alone is the type; nothing further to check.
    let Some(pred) = &resolved.refinement else {
        return Ok(());
    };

    validate_refinement(pred, &resolved.schema, stdlib)
}

/// Resolve a type body into a structural schema plus the per-column refinements inherited from any
/// declared type names used in column position. Catalog lookup is injected so this crate stays pure.
///
/// # Errors
/// [`TypeDefError`] for malformed JSON, unknown / recursive named types, a named multi-column type
/// in column position, or an `unknown` declaration.
pub fn resolve_type_def<F>(body_json: &str, lookup: F) -> Result<ResolvedTypeDef, TypeDefError>
where
    F: Fn(&str) -> Option<String>,
{
    resolve_type_def_inner(body_json, &lookup, &mut Vec::new())
}

/// Whether `/type/<name>` would shadow the base column-token namespace.
#[must_use]
pub fn type_name_shadows_base(path: &str) -> bool {
    qfs_types::type_name_shadows_base(path)
}

fn resolve_type_def_inner<F>(
    body_json: &str,
    lookup: &F,
    stack: &mut Vec<String>,
) -> Result<ResolvedTypeDef, TypeDefError>
where
    F: Fn(&str) -> Option<String>,
{
    let body: TypeBody =
        serde_json::from_str(body_json).map_err(|e| TypeDefError::MalformedBody {
            detail: e.to_string(),
        })?;

    // Rehydrate the declared columns into a Schema, rejecting a late-bound (`unknown`) declaration.
    let mut columns = Vec::with_capacity(body.columns.len());
    let mut column_refinements = Vec::new();
    for c in &body.columns {
        let resolved = resolve_column_type(c, lookup, stack)?;
        column_refinements.extend(resolved.refinements);
        columns.push(Column::new(c.name.clone(), resolved.ty, c.nullable));
    }
    let schema = Schema::new(columns);

    Ok(ResolvedTypeDef {
        columns: body.columns,
        schema,
        refinement: body.r#where,
        column_refinements,
    })
}

struct ResolvedColumnType {
    ty: ColumnType,
    refinements: Vec<ColumnRefinement>,
}

fn resolve_column_type<F>(
    c: &DeclaredColumn,
    lookup: &F,
    stack: &mut Vec<String>,
) -> Result<ResolvedColumnType, TypeDefError>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(ty) = base_column_type(&c.ty) {
        if matches!(ty, ColumnType::Unknown) {
            return Err(TypeDefError::UnknownTypedColumn {
                column: c.name.clone(),
            });
        }
        return Ok(ResolvedColumnType {
            ty,
            refinements: Vec::new(),
        });
    }

    let Some(path) = declared_type_path(&c.ty) else {
        return Err(TypeDefError::UnknownColumnType {
            column: c.name.clone(),
            ty: c.ty.clone(),
        });
    };
    if stack.iter().any(|p| p == &path) {
        return Err(TypeDefError::RecursiveDeclaredType { ty: path });
    }
    let body_json = lookup(&path).ok_or_else(|| TypeDefError::UnknownDeclaredType {
        column: c.name.clone(),
        ty: path.clone(),
    })?;
    stack.push(path.clone());
    let resolved = resolve_type_def_inner(&body_json, lookup, stack);
    stack.pop();
    let resolved = resolved?;
    let [base_col] = resolved.schema.columns.as_slice() else {
        return Err(TypeDefError::MultiColumnDeclaredType {
            column: c.name.clone(),
            ty: path,
        });
    };
    let base_col = base_col.clone();
    let mut refinements = Vec::new();
    refinements.extend(
        resolved
            .column_refinements
            .into_iter()
            .map(|r| ColumnRefinement {
                column: c.name.clone(),
                ty: r.ty,
                schema: r.schema,
                predicate: r.predicate,
            }),
    );
    if let Some(predicate) = resolved.refinement {
        refinements.push(ColumnRefinement {
            column: c.name.clone(),
            ty: path,
            schema: Schema::new(vec![base_col.clone()]),
            predicate,
        });
    }
    Ok(ResolvedColumnType {
        ty: base_col.ty.clone(),
        refinements,
    })
}

fn validate_refinement(
    pred: &Expr,
    schema: &Schema,
    stdlib: &StdlibRegistry,
) -> Result<(), TypeDefError> {
    // Structural walk: reject non-row-local categories + column references that are not declared.
    // Lambda parameters (introduced by `map`/`filter`/`reduce`) are the only non-column names a
    // row-local predicate may bind, so they are tracked in scope as the walk descends.
    let mut bound = HashSet::new();
    walk_predicate(pred, schema, stdlib, &mut bound)?;

    // Static type check: the predicate must yield a boolean (a late-bound `unknown` result is
    // tolerated — an unresolvable-but-pure sub-expression stays late-bound rather than false-fails).
    let ty = check_expr(pred, &TyEnv::new(), schema, Some(stdlib))
        .map_err(|e| TypeDefError::RefinementTypeError { code: e.code() })?;
    match ty.as_prim() {
        Some(ColumnType::Bool | ColumnType::Unknown) => Ok(()),
        Some(other) => Err(TypeDefError::NonBooleanRefinement {
            found: other.type_token().to_string(),
        }),
        None => Err(TypeDefError::NonBooleanRefinement {
            found: "function".to_string(),
        }),
    }
}

/// Walk a predicate `Expr`, rejecting the non-row-local built-in categories and any column
/// reference that is not a declared column (or a lambda parameter currently in scope). The Expr
/// grammar has no subquery / cross-relation node, so a refinement structurally cannot embed one —
/// `Expr::Path` is row-local struct navigation over a declared column, not a relation reference.
fn walk_predicate(
    expr: &Expr,
    schema: &Schema,
    stdlib: &StdlibRegistry,
    bound: &mut HashSet<String>,
) -> Result<(), TypeDefError> {
    match expr {
        Expr::Lit(_) => Ok(()),
        // A bare identifier is a lambda parameter in scope, a literal word (`true`/`false`/`null`),
        // or a declared column. Anything else is an undeclared reference (a contract violation).
        Expr::Col(name) => {
            if bound.contains(name) {
                return Ok(());
            }
            match name.to_ascii_lowercase().as_str() {
                "true" | "false" | "null" => Ok(()),
                _ if schema.column(name).is_some() => Ok(()),
                _ => Err(TypeDefError::UnknownColumnRef { name: name.clone() }),
            }
        }
        // Struct navigation `a.b.c`: the HEAD must resolve to a declared column / bound parameter;
        // the trailing field walk is row-local.
        Expr::Path(segs) => match segs.first() {
            Some(head)
                if bound.contains(head)
                    || schema.column(head).is_some()
                    || matches!(
                        head.to_ascii_lowercase().as_str(),
                        "true" | "false" | "null"
                    ) =>
            {
                Ok(())
            }
            Some(head) => Err(TypeDefError::UnknownColumnRef { name: head.clone() }),
            None => Ok(()),
        },
        Expr::Fn(fnref) => walk_call(fnref, schema, stdlib, bound),
        Expr::Lambda { params, body } => {
            // The parameters bind within the body only; walk under an extended scope, then restore.
            let added: Vec<String> = params
                .iter()
                .filter(|p| bound.insert(p.name.clone()))
                .map(|p| p.name.clone())
                .collect();
            let result = walk_predicate(body, schema, stdlib, bound);
            for name in added {
                bound.remove(&name);
            }
            result
        }
        Expr::Binary { lhs, rhs, .. } => {
            walk_predicate(lhs, schema, stdlib, bound)?;
            walk_predicate(rhs, schema, stdlib, bound)
        }
        Expr::Unary { expr, .. } => walk_predicate(expr, schema, stdlib, bound),
        Expr::In { expr, set } | Expr::AnyOp { expr, set, .. } => {
            walk_predicate(expr, schema, stdlib, bound)?;
            for member in set {
                walk_predicate(member, schema, stdlib, bound)?;
            }
            Ok(())
        }
        Expr::Between { expr, low, high } => {
            walk_predicate(expr, schema, stdlib, bound)?;
            walk_predicate(low, schema, stdlib, bound)?;
            walk_predicate(high, schema, stdlib, bound)
        }
        Expr::Like { expr, pattern } => {
            walk_predicate(expr, schema, stdlib, bound)?;
            walk_predicate(pattern, schema, stdlib, bound)
        }
        Expr::Array(elems) => {
            for e in elems {
                walk_predicate(e, schema, stdlib, bound)?;
            }
            Ok(())
        }
        Expr::Struct(fields) => {
            for (_, e) in fields {
                walk_predicate(e, schema, stdlib, bound)?;
            }
            Ok(())
        }
    }
}

/// Reject a call to a non-row-local built-in (aggregate / table-valued / context), then recurse
/// into the arguments. An unknown / prelude-alias name is left for [`check_expr`] to adjudicate.
fn walk_call(
    fnref: &FnRef,
    schema: &Schema,
    stdlib: &StdlibRegistry,
    bound: &mut HashSet<String>,
) -> Result<(), TypeDefError> {
    let name = fnref.name.as_str();
    let canonical = name.to_ascii_lowercase();
    if context_builtin_names().contains(&canonical.as_str()) {
        return Err(TypeDefError::ImpureRefinement {
            name: name.to_string(),
            category: "context",
        });
    }
    if let Some(builtin) = stdlib.builtin(name) {
        if !builtin.is_row_local_pure() {
            let category = match builtin.eval {
                BuiltinEval::Aggregate(_) => "aggregate",
                BuiltinEval::TableValued(_) => "table-valued",
                BuiltinEval::Scalar(_) | BuiltinEval::HigherOrder(_) => "impure",
            };
            return Err(TypeDefError::ImpureRefinement {
                name: name.to_string(),
                category,
            });
        }
    }
    for arg in &fnref.args {
        walk_predicate(arg, schema, stdlib, bound)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdlib() -> StdlibRegistry {
        StdlibRegistry::with_core()
    }

    /// Build a §5.4 body object from a column list and an optional `WHERE` clause by parsing a real
    /// `CREATE TYPE` and lifting the stored `body` cell — the exact JSON the validator sees at write.
    fn body_of(create_type: &str) -> String {
        let stmt = qfs_parser::parse_statement(create_type).expect("parses");
        let qfs_parser::Statement::Effect(effect) = stmt else {
            panic!("expected an INSERT effect");
        };
        let qfs_parser::EffectBody::Values(values) = &effect.body else {
            panic!("expected a VALUES body");
        };
        let cols = values.columns.as_ref().expect("explicit columns");
        let idx = cols.iter().position(|c| c == "body").expect("body column");
        let qfs_parser::Expr::Lit(qfs_parser::Literal::Str(body)) = &values.rows[0][idx] else {
            panic!("expected a string body cell");
        };
        body.clone()
    }

    #[test]
    fn a_well_formed_refinement_declares_ok() {
        let body = body_of("CREATE TYPE email (value text) WHERE value LIKE '%@%'");
        assert_eq!(validate_type_def(&body, &stdlib()), Ok(()));
    }

    #[test]
    fn a_type_without_a_refinement_declares_ok() {
        let body = body_of("CREATE TYPE point (x int, y int)");
        assert_eq!(validate_type_def(&body, &stdlib()), Ok(()));
    }

    #[test]
    fn only_canonical_base_types_resolve_through_the_shared_surface() {
        let body = body_of(
            "CREATE TYPE canonical (\
             id int, \
             name text, \
             payload bytes, \
             document json)",
        );
        let resolved = resolve_type_def(&body, |_| None).expect("canonical types resolve");
        let types: Vec<ColumnType> = resolved
            .schema
            .columns
            .iter()
            .map(|c| c.ty.clone())
            .collect();
        assert_eq!(
            types,
            vec![
                ColumnType::Int,
                ColumnType::Text,
                ColumnType::Bytes,
                ColumnType::Json,
            ]
        );
    }

    #[test]
    fn retired_base_type_aliases_are_not_silently_canonicalized() {
        let body = body_of("CREATE TYPE legacy (value string)");
        assert_eq!(
            validate_type_def_with_catalog(&body, &stdlib(), |_| None),
            Err(TypeDefError::UnknownDeclaredType {
                column: "value".to_string(),
                ty: "/type/string".to_string()
            })
        );
    }

    #[test]
    fn a_row_local_scalar_builtin_is_allowed_case_insensitively() {
        let body = body_of("CREATE TYPE email (value text) WHERE lower(value) LIKE '%@%'");
        assert_eq!(validate_type_def(&body, &stdlib()), Ok(()));
    }

    #[test]
    fn an_unknown_column_reference_is_rejected() {
        let body = body_of("CREATE TYPE email (value text) WHERE addr LIKE '%@%'");
        assert_eq!(
            validate_type_def(&body, &stdlib()),
            Err(TypeDefError::UnknownColumnRef {
                name: "addr".to_string()
            })
        );
    }

    #[test]
    fn a_non_boolean_refinement_is_rejected() {
        // A bare text column is not a boolean predicate.
        let body = body_of("CREATE TYPE email (value text) WHERE value");
        assert!(matches!(
            validate_type_def(&body, &stdlib()),
            Err(TypeDefError::NonBooleanRefinement { .. })
        ));
    }

    #[test]
    fn an_aggregate_builtin_is_rejected() {
        let body = body_of("CREATE TYPE t (n int) WHERE COUNT(n) > 0");
        assert_eq!(
            validate_type_def(&body, &stdlib()),
            Err(TypeDefError::ImpureRefinement {
                name: "COUNT".to_string(),
                category: "aggregate",
            })
        );
    }

    #[test]
    fn a_context_builtin_is_rejected() {
        let body = body_of("CREATE TYPE t (v text) WHERE v == env('HOME')");
        assert_eq!(
            validate_type_def(&body, &stdlib()),
            Err(TypeDefError::ImpureRefinement {
                name: "env".to_string(),
                category: "context",
            })
        );
    }

    #[test]
    fn a_now_context_builtin_is_rejected() {
        let body = body_of("CREATE TYPE t (at timestamp) WHERE at < NOW()");
        assert_eq!(
            validate_type_def(&body, &stdlib()),
            Err(TypeDefError::ImpureRefinement {
                name: "NOW".to_string(),
                category: "context",
            })
        );
    }

    #[test]
    fn an_unknown_typed_column_is_rejected_in_declaration_position() {
        let body = body_of("CREATE TYPE t (v unknown)");
        assert_eq!(
            validate_type_def(&body, &stdlib()),
            Err(TypeDefError::UnknownTypedColumn {
                column: "v".to_string()
            })
        );
    }

    #[test]
    fn a_column_typed_by_declared_type_resolves_to_its_base_and_refinement() {
        let email = body_of("CREATE TYPE email (value text) WHERE value LIKE '%@%'");
        let customer = body_of("CREATE TYPE customer (email email)");
        let lookup = |path: &str| (path == "/type/email").then(|| email.clone());

        assert_eq!(
            validate_type_def_with_catalog(&customer, &stdlib(), lookup),
            Ok(())
        );

        let resolved = resolve_type_def(&customer, |path| {
            (path == "/type/email").then(|| email.clone())
        })
        .expect("declared type resolves");
        assert_eq!(resolved.schema.columns[0].name, "email");
        assert_eq!(resolved.schema.columns[0].ty, ColumnType::Text);
        assert_eq!(resolved.column_refinements.len(), 1);
        assert_eq!(resolved.column_refinements[0].column, "email");
    }

    #[test]
    fn an_unknown_declared_type_name_is_rejected() {
        let body = body_of("CREATE TYPE customer (email email)");
        assert_eq!(
            validate_type_def_with_catalog(&body, &stdlib(), |_| None),
            Err(TypeDefError::UnknownDeclaredType {
                column: "email".to_string(),
                ty: "/type/email".to_string()
            })
        );
    }

    #[test]
    fn a_declared_type_name_cannot_shadow_a_base_token() {
        assert!(type_name_shadows_base("/type/text"));
        assert!(!type_name_shadows_base("/type/customer"));
        assert!(!type_name_shadows_base("/type/chatwork/message"));
    }
}
