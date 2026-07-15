//! Builtin lexing + desugaring (ticket t28, hard part (c)).
//!
//! The shell's filesystem verbs (`ls cd pwd cat cp mv rm`) are **CLI-layer sugar, not grammar
//! keywords** (blueprint §3 governance). A line is a builtin iff its **first token** is in the builtin
//! set; everything else parses as a raw closed-core qfs statement. This module owns that
//! distinction and the lowering of each effectful/read builtin into one or more closed-core qfs
//! **source statements** — which the shell then routes through the SAME parse → plan → preview /
//! scan pipeline the one-shot path uses. Desugaring to *source text* (not a hand-built AST) is
//! deliberate: it guarantees a builtin produces byte-for-byte the same `Statement` (hence the
//! same `Plan`) as the equivalent statement typed at the prompt, which is the t28 acceptance.
//!
//! `cd`/`pwd` carry **no plan** — they are pure cwd state changes the [`session`](crate::shell::session)
//! layer performs directly (with a driver capability check for `cd`), so they are modelled here
//! only as their [`Builtin`] tag, never desugared to a statement.

use qfs_core::{Archetype, NodeCategory};

use crate::error::{ErrorKind, ExecError};
use crate::shell::path::{resolve, VfsPath};

/// The describe facts the desugar reads about ONE path — the driver's own words, resolved by the
/// session (which holds the registry) and passed in so this module stays pure and unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeFacts {
    /// The node's entry kind — what shape its rows are. Decides `ls`'s projection and `cp`'s verb.
    pub archetype: Archetype,
    /// Which of §5.5's two categories the node's rows are (data vs definitions).
    pub category: NodeCategory,
}

impl NodeFacts {
    /// Build the facts for a node.
    #[must_use]
    pub fn new(archetype: Archetype, category: NodeCategory) -> Self {
        Self {
            archetype,
            category,
        }
    }

    /// Whether this node is a definition catalog (`/type`, `/transform`).
    #[must_use]
    pub fn is_definition(self) -> bool {
        self.category == NodeCategory::Definition
    }
}

/// What the session resolved about the paths a builtin names. `None` for a path that is unmounted or
/// undescribable — every rule below then falls back to the SAFE default (the bare read for `ls`, the
/// shipped `UPSERT` for `cp`), never a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Facts {
    /// The first path operand: `ls [path]`, `cat <path>`, `cp <src>`, `mv <src>`.
    pub src: Option<NodeFacts>,
    /// The destination: `cp <src> <dst>`, `mv <src> <dst>`.
    pub dst: Option<NodeFacts>,
}

impl Facts {
    /// The facts for a single-operand builtin (`ls`).
    #[must_use]
    pub fn of_src(src: Option<NodeFacts>) -> Self {
        Self { src, dst: None }
    }
}

/// The closed builtin set. These names are reserved **only at the line head**; the same word
/// anywhere else (or as a raw statement keyword) is untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// `ls [path]` — list a namespace (desugars to a `SELECT` over the listing relation).
    Ls,
    /// `cd <path>` — change the cwd (pure state change; capability-gated by the session).
    Cd,
    /// `pwd` — print the cwd (pure).
    Pwd,
    /// `cat <path>` — read a blob/relation (desugars to a bare `FROM <path>`).
    Cat,
    /// `describe [path]` — report a node's contract (pure; no plan). Like `cd`/`pwd` it carries no
    /// statement form: the session folds the driver's introspective half directly.
    Describe,
    /// `cp <src> <dst>` — copy (desugars to `UPSERT INTO <dst> FROM <src>`).
    Cp,
    /// `mv <src> <dst>` — move (desugars to copy→delete: `UPSERT … FROM …` then `REMOVE <src>`).
    Mv,
    /// `rm <path>…` — remove (desugars each arg to `REMOVE <path>`).
    Rm,
}

impl Builtin {
    /// Recognise a line-head token as a builtin, else `None` (the line is a raw qfs statement).
    /// Case-sensitive lowercase: the shell verbs are lowercase sugar; the qfs grammar keywords
    /// are conventionally uppercase, so `LS`/`RM` typed at the prompt parse as qfs, never sugar.
    #[must_use]
    pub fn from_head(token: &str) -> Option<Self> {
        match token {
            "ls" => Some(Builtin::Ls),
            "cd" => Some(Builtin::Cd),
            "pwd" => Some(Builtin::Pwd),
            "cat" => Some(Builtin::Cat),
            "describe" => Some(Builtin::Describe),
            "cp" => Some(Builtin::Cp),
            "mv" => Some(Builtin::Mv),
            "rm" => Some(Builtin::Rm),
            _ => None,
        }
    }

    /// The builtin name (for completion + help).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Ls => "ls",
            Builtin::Cd => "cd",
            Builtin::Pwd => "pwd",
            Builtin::Cat => "cat",
            Builtin::Describe => "describe",
            Builtin::Cp => "cp",
            Builtin::Mv => "mv",
            Builtin::Rm => "rm",
        }
    }

    /// Whether this builtin produces an **effect** plan (vs a pure read / pure state change).
    /// Drives the PREVIEW/COMMIT gate: only effectful builtins ever need a COMMIT.
    #[must_use]
    pub fn is_effect(self) -> bool {
        matches!(self, Builtin::Cp | Builtin::Mv | Builtin::Rm)
    }

    /// Every builtin name, in display order (for the completer).
    #[must_use]
    pub fn all_names() -> &'static [&'static str] {
        &["ls", "cd", "pwd", "cat", "describe", "cp", "mv", "rm"]
    }
}

/// The classification of a typed line: a recognised builtin (with its already-split args) or a
/// raw qfs statement to parse verbatim.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A builtin invocation: the verb + its whitespace-split argument tokens.
    Builtin {
        /// The recognised builtin.
        verb: Builtin,
        /// The argument tokens (already whitespace-split; quoting is not yet supported).
        args: Vec<String>,
    },
    /// A raw qfs statement (anything whose head is not a builtin).
    Raw(String),
    /// An empty line (whitespace only) — a no-op.
    Empty,
}

/// Classify a typed `line` into a [`Line`] (hard part (c)): split the head token, check it
/// against the builtin set, else treat the whole line as a raw qfs statement.
#[must_use]
pub fn classify(line: &str) -> Line {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Line::Empty;
    }
    let mut tokens = trimmed.split_whitespace();
    let head = tokens.next().unwrap_or("");
    match Builtin::from_head(head) {
        Some(verb) => Line::Builtin {
            verb,
            args: tokens.map(str::to_string).collect(),
        },
        None => Line::Raw(trimmed.to_string()),
    }
}

/// What a builtin lowers to: a sequence of closed-core qfs **source statements** to run in
/// order, plus the rendering intent so the shell knows whether the result is a listing (read)
/// or an effect preview/commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Desugared {
    /// The qfs source statements to run, in order. A single read for `ls`/`cat`; one effect for
    /// `cp`/`rm <one>`; two for cross-mount `mv` (copy then delete); N for `rm a b c`.
    pub statements: Vec<String>,
    /// Whether these are effect statements (need the PREVIEW/COMMIT gate) or a pure read.
    pub is_effect: bool,
}

/// Desugar an effectful/read builtin into closed-core qfs source statements, resolving every
/// path argument against `cwd` first (so a relative `notes/x` becomes the absolute `/local/...`
/// the grammar requires). `cd`/`pwd`/`describe` are pure — the first two are cwd state changes and
/// the third folds the driver's introspective half — so the session handles them directly and they
/// are rejected here with a usage error (they have no statement form).
///
/// # Errors
/// [`ExecError`] (kind `usage`) on a wrong argument count or a `cd`/`pwd`/`describe` passed here.
pub fn desugar(
    verb: Builtin,
    args: &[String],
    cwd: &VfsPath,
    facts: Facts,
) -> Result<Desugared, ExecError> {
    let ls_archetype = facts.src.map(|f| f.archetype);
    match verb {
        Builtin::Cd | Builtin::Pwd | Builtin::Describe => Err(ExecError::usage(format!(
            "`{}` is pure with no statement form",
            verb.name()
        ))),
        Builtin::Ls => {
            // `ls` lists the cwd by default, else the (resolved) argument. The enumeration is
            // **entry-kind-typed** (blueprint §5.1): a BLOB NAMESPACE enumerates file rows (the
            // stable `name/size/is_dir/modified` projection); every OTHER entry kind's rows ARE its
            // enumeration, so `ls` is the bare read (a relational table's rows, a definition
            // catalog's defs = SHOW TYPES/TRANSFORMS, an append log's tail, an object graph's
            // entities). The `ls_archetype` is the resolved target's describe archetype, supplied by
            // the session (which holds the registry); `None` (unmounted/undescribable) safely
            // defaults to the bare read. Hardcoding the blob projection here was the §5.1 defect —
            // it made `ls /mail/inbox` / `ls /transform` fail with `unknown column`.
            let target = match args.first() {
                Some(p) => resolve(p, cwd)?,
                None => cwd.clone(),
            };
            let abs = target.render();
            let stmt = match ls_archetype {
                Some(Archetype::BlobNamespace) => {
                    format!("{abs} |> SELECT name, size, is_dir, modified")
                }
                _ => abs,
            };
            Ok(Desugared {
                statements: vec![stmt],
                is_effect: false,
            })
        }
        Builtin::Cat => {
            let p = arg(args, 0, "cat <path>")?;
            let target = resolve(p, cwd)?;
            Ok(Desugared {
                statements: vec![target.render()],
                is_effect: false,
            })
        }
        Builtin::Cp => {
            let (src_raw, dst_raw) = two_args(args, "cp <src> <dst>")?;
            let src = resolve(src_raw, cwd)?;
            let dst = resolve(dst_raw, cwd)?;
            // §5.5's category line, caught at the shell where both operands are known: copying DATA
            // rows into a definition catalog is a CATEGORY error — the two categories never pool.
            // (The engine enforces the same rule at plan time for a raw `INSERT INTO /transform
            // <data>`; this arm is the shell's earlier, better-worded half.)
            if let (Some(s), Some(d)) = (facts.src, facts.dst) {
                if d.is_definition() && !s.is_definition() {
                    return Err(category_error(&src, &dst));
                }
                if d.is_definition() && s.is_definition() {
                    return Err(cp_definition_clone_error(&src, &dst));
                }
            }
            Ok(Desugared {
                statements: vec![copy_stmt(&src, &dst, facts.dst)],
                is_effect: true,
            })
        }
        Builtin::Mv => {
            let (src_raw, dst_raw) = two_args(args, "mv <src> <dst>")?;
            let src = resolve(src_raw, cwd)?;
            let dst = resolve(dst_raw, cwd)?;
            mv_statements(&src, &dst, facts)
        }
        Builtin::Rm => {
            if args.is_empty() {
                return Err(ExecError::usage("rm <path>… requires at least one path"));
            }
            // Each arg is its own `REMOVE <target>` (a target may be a glob the driver expands to
            // a set). The shell previews the union of affected counts; COMMIT applies all.
            let statements = args
                .iter()
                .map(|a| resolve(a, cwd).map(|p| remove_stmt(&p)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Desugared {
                statements,
                is_effect: true,
            })
        }
    }
}

/// The copy desugar shared by `cp` and `mv`'s blob leg (decision R, t73: the source operand leads,
/// no `FROM`). **The verb is keyed on the DESTINATION's entry kind** (blueprint §9):
///
/// - **blob namespace → `UPSERT INTO`** — the retry-safe, idempotent write the blob archetype
///   declares (§7): re-running a copy after a partial failure converges rather than erroring on an
///   existing destination, which is exactly the recovery shape `mv`'s copy→verify→delete depends on.
/// - **every other kind → `INSERT INTO`** — a table, an append log, an object graph. An idempotent
///   "send" into an append log is a LIE: `UPSERT` claims a key it has no way to match on, so a
///   retried `cp` into `/mail/drafts` would silently mean "replace", not "append". `INSERT` is the
///   honest verb, and it is what makes the mission's "`cp` ≡ membership-checked `insert into`" true
///   where the destination is an `OF`-typed table (the shipped `materialize_pipeline_source` → args
///   → `check_table_membership` chain then polices every row at commit).
///
/// An UNKNOWN destination (unmounted / undescribable) keeps the shipped `UPSERT`, so the fallback is
/// the historical behaviour rather than a guess.
fn copy_stmt(src: &VfsPath, dst: &VfsPath, dst_facts: Option<NodeFacts>) -> String {
    let verb = match dst_facts.map(|f| f.archetype) {
        Some(Archetype::BlobNamespace) | None => "UPSERT INTO",
        Some(_) => "INSERT INTO",
    };
    format!("{verb} {} {}", dst.render(), src.render())
}

/// Lower `mv` under the **same-entry-kind rule** (blueprint §9). `mv` is the one verb with no
/// uniform meaning across entry kinds, so the discipline is: refuse rather than paper over.
///
/// - **blob → blob**: copy→verify→delete (§7 recovery), as shipped. Two statements, both
///   dry-runnable; the shell previews BOTH affected counts and applies nothing until COMMIT.
/// - **cross-kind**: refused — moving a blob into an append log is not a move.
/// - **append log / table / object graph → same kind**: refused, NAMING the honest spelling. This is
///   the mail trap the slice exists for: `mv /mail/inbox/<msg> /mail/archive` as copy+delete means
///   **send a new message and trash the original** — silently, irreversibly, to a third party. A
///   relabel is `UPDATE`; a row "move" is `UPDATE`, or `REMOVE` + `INSERT`.
/// - **definition → definition**: refused with the honest spelling too (see `mv_definition_error`).
///
/// UNKNOWN facts on either side fall back to the shipped copy+delete rather than refusing a `mv`
/// that used to work.
fn mv_statements(src: &VfsPath, dst: &VfsPath, facts: Facts) -> Result<Desugared, ExecError> {
    let two_legs = |s: &VfsPath, d: &VfsPath| Desugared {
        statements: vec![copy_stmt(s, d, facts.dst), remove_stmt(s)],
        is_effect: true,
    };
    let (Some(s), Some(d)) = (facts.src, facts.dst) else {
        // Nothing described: keep the shipped lowering (an unmounted path fails downstream anyway).
        return Ok(two_legs(src, dst));
    };
    if s.archetype != d.archetype || s.category != d.category {
        return Err(mv_cross_kind_error(src, dst, s, d));
    }
    match (s.archetype, s.category) {
        // The only kind with a real move: a blob rename/relocate.
        (Archetype::BlobNamespace, NodeCategory::Data) => Ok(two_legs(src, dst)),
        (_, NodeCategory::Definition) => Err(mv_definition_error(src, dst)),
        // Any other kind — and any category added later — refuses. `mv` has no uniform meaning, so
        // the safe default for an unmodelled entry kind is to refuse, never to copy+delete.
        (archetype, _) => Err(mv_same_kind_refusal(src, dst, archetype)),
    }
}

/// The §5.5 category error: data rows may not be copied into a definition catalog.
fn category_error(src: &VfsPath, dst: &VfsPath) -> ExecError {
    ExecError::new(
        ErrorKind::Capability,
        "category_error",
        format!(
            "cannot cp `{}` into `{}`: `{}` is a DEFINITION catalog and `{}` holds DATA — the two \
             categories never pool (blueprint §5.5). A definition is declared, not copied in: use \
             `CREATE TYPE` / `CREATE TRANSFORM`.",
            src.render(),
            dst.render(),
            dst.render(),
            src.render(),
        ),
    )
    .with_path(dst.render())
}

/// `cp` between definition-catalog nodes — a clone the catalogs cannot express.
///
/// A definition row CARRIES its own name, so `cp /transform/a /transform/b` would lower to an
/// `INSERT` of a row still named `a` — re-inserting `a`, not cloning it to `b`. Expressing the clone
/// needs a name-rewriting projection the shell has no way to build (and `/type` exposes no write verb
/// at all — a type is installed through `/sys/drivers`). Refuse honestly rather than silently write
/// the wrong row.
fn cp_definition_clone_error(src: &VfsPath, dst: &VfsPath) -> ExecError {
    ExecError::new(
        ErrorKind::Capability,
        "cp_unsupported_kind",
        format!(
            "cannot cp `{}` to `{}`: a definition carries its own name, so copying its row would \
             re-insert `{}` rather than clone it. Declare the new definition explicitly \
             (`CREATE …` under the new name) — that is the one spelling that names it.",
            src.render(),
            dst.render(),
            src.render(),
        ),
    )
    .with_path(dst.render())
}

/// `mv` across entry kinds — not a move at all.
fn mv_cross_kind_error(src: &VfsPath, dst: &VfsPath, s: NodeFacts, d: NodeFacts) -> ExecError {
    ExecError::new(
        ErrorKind::Capability,
        "mv_cross_kind",
        format!(
            "cannot mv `{}` into `{}`: a move is same-kind-only, but the source is a {:?} \
             ({:?}) and the destination a {:?} ({:?}). Copy it (`cp`) and then remove the source \
             explicitly, so both halves are previewed for what they are.",
            src.render(),
            dst.render(),
            s.archetype,
            s.category,
            d.archetype,
            d.category,
        ),
    )
    .with_path(dst.render())
}

/// `mv` within a non-blob data kind — refused, naming the honest spelling. The mail trap.
fn mv_same_kind_refusal(src: &VfsPath, dst: &VfsPath, archetype: Archetype) -> ExecError {
    let honest = match archetype {
        Archetype::AppendLog => {
            "an append log has no move: copy+delete here would SEND a new entry and delete the \
             original (on /mail that means mailing a third party and trashing your copy). To \
             re-file, change the labels: `UPDATE <path> SET labels = …`"
        }
        Archetype::RelationalTable => {
            "a row is a value, not a location: use `UPDATE <path> SET … WHERE …` to change it in \
             place, or `REMOVE` + `INSERT` if it really must move"
        }
        _ => {
            "this entry kind has no move: use the explicit write verbs (`UPDATE`, or `REMOVE` + \
             `INSERT`) so the effect is previewed for what it is"
        }
    };
    ExecError::new(
        ErrorKind::Capability,
        "mv_unsupported_kind",
        format!(
            "cannot mv `{}` to `{}`: {honest}.",
            src.render(),
            dst.render(),
        ),
    )
    .with_path(src.render())
}

/// `mv` between definition-catalog nodes — a rename the catalogs cannot express as one write.
fn mv_definition_error(src: &VfsPath, dst: &VfsPath) -> ExecError {
    ExecError::new(
        ErrorKind::Capability,
        "mv_unsupported_kind",
        format!(
            "cannot mv `{}` to `{}`: a definition is renamed by re-declaring it under the new name \
             and removing the old (`CREATE …` then `REMOVE {}`) — the catalogs expose no in-place \
             rename, and a silent copy+delete would leave references to the old name dangling.",
            src.render(),
            dst.render(),
            src.render(),
        ),
    )
    .with_path(src.render())
}

/// `REMOVE <target>` — the delete desugar shared by `rm` and `mv`'s second leg.
fn remove_stmt(target: &VfsPath) -> String {
    format!("REMOVE {}", target.render())
}

/// Fetch the `n`-th argument or a usage error naming the expected form.
fn arg<'a>(args: &'a [String], n: usize, usage: &str) -> Result<&'a String, ExecError> {
    args.get(n)
        .ok_or_else(|| ExecError::usage(format!("usage: {usage}")))
}

/// Require exactly two arguments (`<src> <dst>`).
fn two_args<'a>(args: &'a [String], usage: &str) -> Result<(&'a String, &'a String), ExecError> {
    if args.len() != 2 {
        return Err(ExecError::usage(format!("usage: {usage}")));
    }
    Ok((&args[0], &args[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> VfsPath {
        VfsPath::new("local", vec!["docs".into()])
    }

    #[test]
    fn line_head_only_recognises_builtins() {
        assert_eq!(
            classify("ls notes"),
            Line::Builtin {
                verb: Builtin::Ls,
                args: vec!["notes".into()]
            }
        );
        // A qfs keyword at the head is NOT a builtin — it parses as a raw statement.
        assert_eq!(
            classify("/local |> LIMIT 1"),
            Line::Raw("/local |> LIMIT 1".into())
        );
        // Uppercase `LS` is not the lowercase sugar.
        assert!(matches!(classify("LS x"), Line::Raw(_)));
        assert_eq!(classify("   "), Line::Empty);
    }

    /// Data-category facts for an entry kind (the overwhelming common case).
    fn data(archetype: Archetype) -> Option<NodeFacts> {
        Some(NodeFacts::new(archetype, NodeCategory::Data))
    }

    /// Definition-catalog facts (`/type`, `/transform`).
    fn def() -> Option<NodeFacts> {
        Some(NodeFacts::new(
            Archetype::RelationalTable,
            NodeCategory::Definition,
        ))
    }

    /// `cp`/`mv` facts for a src→dst pair.
    fn pair(src: Option<NodeFacts>, dst: Option<NodeFacts>) -> Facts {
        Facts { src, dst }
    }

    #[test]
    fn ls_over_a_blob_namespace_keeps_the_file_projection() {
        let d = desugar(
            Builtin::Ls,
            &[],
            &cwd(),
            Facts::of_src(data(Archetype::BlobNamespace)),
        )
        .unwrap();
        assert_eq!(
            d.statements,
            vec!["/local/docs |> SELECT name, size, is_dir, modified"]
        );
        assert!(!d.is_effect);
    }

    #[test]
    fn ls_over_a_non_blob_kind_is_the_bare_read() {
        // §5.1: a relational table / definition catalog / append log / object graph enumerates its
        // own rows, so `ls` is the bare read — no blob projection (which would fail `unknown column`).
        for arch in [
            Archetype::RelationalTable,
            Archetype::AppendLog,
            Archetype::ObjectGraphWorkflow,
        ] {
            let d = desugar(
                Builtin::Ls,
                &["/mail/inbox".into()],
                &cwd(),
                Facts::of_src(data(arch)),
            )
            .unwrap();
            assert_eq!(d.statements, vec!["/mail/inbox"], "{arch:?}");
            assert!(!d.is_effect);
        }
        // An unmounted/undescribable target also defaults to the bare read.
        let d = desugar(Builtin::Ls, &["/x/y".into()], &cwd(), Facts::default()).unwrap();
        assert_eq!(d.statements, vec!["/x/y"]);
    }

    #[test]
    fn ls_with_relative_arg_resolves_against_cwd() {
        let d = desugar(
            Builtin::Ls,
            &["sub".into()],
            &cwd(),
            Facts::of_src(data(Archetype::BlobNamespace)),
        )
        .unwrap();
        assert_eq!(
            d.statements[0],
            "/local/docs/sub |> SELECT name, size, is_dir, modified"
        );
    }

    #[test]
    fn cat_desugars_to_bare_from() {
        let d = desugar(Builtin::Cat, &["a.md".into()], &cwd(), Facts::default()).unwrap();
        assert_eq!(d.statements, vec!["/local/docs/a.md"]);
        assert!(!d.is_effect);
    }

    #[test]
    fn cp_into_a_blob_is_upsert() {
        // §9: blob → UPSERT — the idempotent, retry-safe write the blob archetype declares, and the
        // recovery shape `mv`'s copy→verify→delete depends on.
        let blob = data(Archetype::BlobNamespace);
        let d = desugar(
            Builtin::Cp,
            &["a.md".into(), "b.md".into()],
            &cwd(),
            pair(blob, blob),
        )
        .unwrap();
        assert_eq!(
            d.statements,
            vec!["UPSERT INTO /local/docs/b.md /local/docs/a.md"]
        );
        assert!(d.is_effect);
    }

    #[test]
    fn cp_into_a_non_blob_destination_is_insert() {
        // §9: the verb is keyed on the DESTINATION's entry kind. An idempotent "send" into an append
        // log is a lie — UPSERT claims a key it cannot match, so a retried `cp` into /mail/drafts
        // would mean "replace", not "append". This is also what makes "cp ≡ membership-checked
        // insert into" literally true for an OF-typed table destination.
        for arch in [
            Archetype::RelationalTable,
            Archetype::AppendLog,
            Archetype::ObjectGraphWorkflow,
        ] {
            let d = desugar(
                Builtin::Cp,
                &["report.md".into(), "/mail/drafts/report".into()],
                &cwd(),
                pair(data(Archetype::BlobNamespace), data(arch)),
            )
            .unwrap();
            assert_eq!(
                d.statements,
                vec!["INSERT INTO /mail/drafts/report /local/docs/report.md"],
                "{arch:?}"
            );
        }
    }

    #[test]
    fn cp_cross_mount_keeps_each_driver_and_falls_back_to_upsert_when_undescribable() {
        // src under cwd's driver, dst under another driver, resolved independently — no `cd`. With
        // NO facts (unmounted/undescribable) the shipped UPSERT stands: the fallback is the
        // historical behaviour, never a guess.
        let d = desugar(
            Builtin::Cp,
            &["report.md".into(), "/mail/drafts/report".into()],
            &cwd(),
            Facts::default(),
        )
        .unwrap();
        assert_eq!(
            d.statements,
            vec!["UPSERT INTO /mail/drafts/report /local/docs/report.md"]
        );
    }

    #[test]
    fn cp_of_data_into_a_definition_catalog_is_a_category_error() {
        // §5.5: paths are data, names are definitions — the two categories never pool.
        let e = desugar(
            Builtin::Cp,
            &["a.md".into(), "/transform".into()],
            &cwd(),
            pair(data(Archetype::BlobNamespace), def()),
        )
        .unwrap_err();
        assert_eq!(e.code, "category_error");
        assert!(e.message.contains("DEFINITION"), "{}", e.message);
    }

    #[test]
    fn cp_between_definition_catalogs_refuses_rather_than_writing_a_misnamed_row() {
        // A definition row carries its OWN name, so copying it would re-insert the source's name
        // rather than clone it under the destination's. Refuse honestly (owner ruling: a cross-
        // catalog def clone is a pure refusal).
        let e = desugar(
            Builtin::Cp,
            &["/transform/a".into(), "/transform/b".into()],
            &cwd(),
            pair(def(), def()),
        )
        .unwrap_err();
        assert_eq!(e.code, "cp_unsupported_kind");
        assert!(e.message.contains("CREATE"), "{}", e.message);
    }

    #[test]
    fn mv_blob_to_blob_is_copy_then_delete() {
        // The one entry kind with a real move.
        let blob = data(Archetype::BlobNamespace);
        let d = desugar(
            Builtin::Mv,
            &["a.md".into(), "b.md".into()],
            &cwd(),
            pair(blob, blob),
        )
        .unwrap();
        assert_eq!(
            d.statements,
            vec![
                "UPSERT INTO /local/docs/b.md /local/docs/a.md",
                "REMOVE /local/docs/a.md",
            ]
        );
        assert!(d.is_effect);
    }

    #[test]
    fn mv_on_an_append_log_refuses_and_names_the_honest_spelling() {
        // THE trap this slice exists for: `mv` on a mail path as copy+delete = SEND a new message
        // to a third party and trash the original. Refuse, and name `UPDATE labels`.
        let log = data(Archetype::AppendLog);
        let e = desugar(
            Builtin::Mv,
            &["/mail/inbox/m1".into(), "/mail/archive".into()],
            &cwd(),
            pair(log, log),
        )
        .unwrap_err();
        assert_eq!(e.code, "mv_unsupported_kind");
        assert!(
            e.message.contains("SEND"),
            "must name the trap: {}",
            e.message
        );
        assert!(
            e.message.contains("labels"),
            "must name the honest spelling: {}",
            e.message
        );
    }

    #[test]
    fn mv_on_a_table_row_refuses_and_names_update() {
        let table = data(Archetype::RelationalTable);
        let e = desugar(
            Builtin::Mv,
            &["/sql/db/t/1".into(), "/sql/db/t2".into()],
            &cwd(),
            pair(table, table),
        )
        .unwrap_err();
        assert_eq!(e.code, "mv_unsupported_kind");
        assert!(e.message.contains("UPDATE"), "{}", e.message);
    }

    #[test]
    fn mv_across_entry_kinds_refuses() {
        // A blob into an append log is not a move at all.
        let e = desugar(
            Builtin::Mv,
            &["a.md".into(), "/mail/drafts".into()],
            &cwd(),
            pair(data(Archetype::BlobNamespace), data(Archetype::AppendLog)),
        )
        .unwrap_err();
        assert_eq!(e.code, "mv_cross_kind");
    }

    #[test]
    fn mv_between_definitions_refuses_naming_redeclare() {
        // The catalogs expose no in-place rename, and a silent copy+delete would leave references to
        // the old name dangling.
        let e = desugar(
            Builtin::Mv,
            &["/transform/a".into(), "/transform/b".into()],
            &cwd(),
            pair(def(), def()),
        )
        .unwrap_err();
        assert_eq!(e.code, "mv_unsupported_kind");
        assert!(e.message.contains("re-declaring"), "{}", e.message);
    }

    #[test]
    fn mv_with_no_facts_keeps_the_shipped_copy_then_delete() {
        // An undescribable path must not start refusing a `mv` that used to work.
        let d = desugar(
            Builtin::Mv,
            &["a.md".into(), "/x/y".into()],
            &cwd(),
            Facts::default(),
        )
        .unwrap();
        assert_eq!(
            d.statements,
            vec![
                "UPSERT INTO /x/y /local/docs/a.md",
                "REMOVE /local/docs/a.md"
            ]
        );
    }

    #[test]
    fn rm_set_is_one_remove_per_arg() {
        let d = desugar(
            Builtin::Rm,
            &["a.md".into(), "sub/b.md".into()],
            &cwd(),
            Facts::default(),
        )
        .unwrap();
        assert_eq!(
            d.statements,
            vec!["REMOVE /local/docs/a.md", "REMOVE /local/docs/sub/b.md"]
        );
        assert!(d.is_effect);
    }

    #[test]
    fn rm_is_kind_independent_and_drops_a_definition() {
        // `rm` needs no per-kind ruling: REMOVE is REMOVE for every entry kind, and on a definition
        // catalog it IS the drop (`rm /transform/<name>`, already irreversible-gated).
        let d = desugar(
            Builtin::Rm,
            &["/transform/classify".into()],
            &cwd(),
            Facts::of_src(def()),
        )
        .unwrap();
        assert_eq!(d.statements, vec!["REMOVE /transform/classify"]);
        assert!(d.is_effect);
    }

    #[test]
    fn rm_requires_an_arg() {
        let e = desugar(Builtin::Rm, &[], &cwd(), Facts::default()).unwrap_err();
        assert_eq!(e.kind.as_str(), "usage");
    }

    #[test]
    fn cd_pwd_describe_have_no_statement_form() {
        assert!(desugar(Builtin::Cd, &["x".into()], &cwd(), Facts::default()).is_err());
        assert!(desugar(Builtin::Pwd, &[], &cwd(), Facts::default()).is_err());
        assert!(desugar(Builtin::Describe, &[], &cwd(), Facts::default()).is_err());
    }

    #[test]
    fn cp_arity_is_enforced() {
        assert!(desugar(Builtin::Cp, &["only-one".into()], &cwd(), Facts::default()).is_err());
    }
}
