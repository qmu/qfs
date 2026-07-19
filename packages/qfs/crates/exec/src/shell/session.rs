//! The interactive session (ticket t28): the stateful cwd, the line evaluator, and the
//! PREVIEW/COMMIT safety gate. This is the shell's brain — the REPL loop (in `qfs-cmd`) only
//! feeds it lines and renders [`Outcome`]s.
//!
//! ## No new semantics
//! Every line — builtin or raw — funnels through [`Session::eval_line`] into the SAME pipeline
//! the one-shot path uses: a read becomes [`block_on_read`](crate::block_on_read) over the
//! [`ReadDriver`] seam; an effect becomes [`build_plan`](crate::build_plan) →
//! [`plan_preview`](crate::plan_preview) (PREVIEW) or [`apply_via`](crate::apply_via) (COMMIT).
//! The shell adds no execution behaviour; it only desugars sugar to core statements and gates
//! destructive effects behind an explicit, typed `COMMIT`. The binary injects the real world
//! applier; tests can omit it and stay on the in-memory recorder.
//!
//! ## The safety invariant
//! PREVIEW is the default. `cp`/`mv`/`rm` (and any raw effect) print their affected counts and
//! the plan preview, and apply **nothing** until the operator types `COMMIT` (or wraps the line
//! `COMMIT …`). A builtin can never shortcut around this — it lowers to the same `Plan` and the
//! same gate. `cd`/`pwd` are pure cwd state changes (no plan).

use qfs_core::{Engine, MountRegistry};

use crate::dto::{PlanPreview, RowSet};
use crate::error::{ErrorKind, ExecError};
use crate::exec::{apply_via, block_on_read, build_plan, parse, plan_preview};
use crate::read::ReadRegistry;
use crate::shell::desugar::{classify, desugar, Builtin, Facts, Line, NodeFacts};
use crate::shell::path::{resolve, VfsPath};

/// The outcome of evaluating one line — what the REPL renders.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A pure read produced rows (`ls`/`cat`/a raw `SELECT`).
    Listing(RowSet),
    /// One or more effect plans previewed (nothing applied) — the affected counts + plan.
    Preview(Vec<PlanPreview>),
    /// One or more effect plans committed (applied) — the committed summaries.
    Committed(Vec<PlanPreview>),
    /// A `describe` folded a node's contract (pure — no plan, no I/O, no creds).
    Described(Box<qfs_core::DescribeReport>),
    /// A `cd` changed the cwd; carries the new location for the prompt.
    Moved(VfsPath),
    /// A `pwd` — print the cwd.
    Cwd(VfsPath),
    /// Nothing to do (an empty line).
    Empty,
}

/// The interactive session: the tagged cwd plus the engine + read registries every line is
/// evaluated against. Holds no terminal, no history — those belong to the REPL driver, so the
/// session is fully testable by feeding scripted lines and asserting [`Outcome`]s.
pub struct Session<'a> {
    /// The current working location (`{driver, path}`), rendered into the prompt.
    cwd: VfsPath,
    /// The shared engine (mount registry for describe/pushdown/plan; codecs; secrets).
    engine: &'a Engine,
    /// The read-driver registry the read path resolves scans through.
    reads: &'a ReadRegistry,
    /// Optional real-world apply hook. Unit tests leave this absent and use the in-memory recorder;
    /// the binary REPL injects its live interpreter-backed commit path.
    world: Option<&'a crate::WorldApply<'a>>,
}

impl<'a> Session<'a> {
    /// Start a session at `cwd` against the live registries.
    #[must_use]
    pub fn new(cwd: VfsPath, engine: &'a Engine, reads: &'a ReadRegistry) -> Self {
        Self {
            cwd,
            engine,
            reads,
            world: None,
        }
    }

    /// Attach the real world-apply hook for COMMIT. Preview remains pure; only confirmed commits
    /// route through this hook.
    #[must_use]
    pub fn with_world_apply(mut self, world: &'a crate::WorldApply<'a>) -> Self {
        self.world = Some(world);
        self
    }

    /// The current working location (for the prompt / `pwd`).
    #[must_use]
    pub fn cwd(&self) -> &VfsPath {
        &self.cwd
    }

    /// The prompt string `{driver}:{path}$ ` reflecting the cwd (blueprint §9).
    #[must_use]
    pub fn prompt(&self) -> String {
        let path = if self.cwd.is_root() {
            "/".to_string()
        } else {
            format!("/{}", self.cwd.segments().join("/"))
        };
        format!("{}:{}$ ", self.cwd.driver(), path)
    }

    /// Evaluate one typed line. Dispatches builtin vs raw, applies the PREVIEW/COMMIT gate, and
    /// returns the [`Outcome`] to render. `commit` is the session's apply switch (the REPL sets
    /// it when the operator typed a bare `COMMIT` confirmation or a `COMMIT …`-wrapped line).
    ///
    /// # Errors
    /// [`ExecError`] with the mapped kind on any parse / capability / commit failure.
    pub fn eval_line(&mut self, line: &str, commit: bool) -> Result<Outcome, ExecError> {
        match classify(line) {
            Line::Empty => Ok(Outcome::Empty),
            Line::Builtin { verb, args } => self.eval_builtin(verb, &args, commit),
            Line::Raw(stmt) => self.eval_statements(&[stmt], commit),
        }
    }

    /// Evaluate a recognised builtin. `cd`/`pwd` are pure state changes; the rest desugar to
    /// closed-core statements and route through [`Self::eval_statements`].
    fn eval_builtin(
        &mut self,
        verb: Builtin,
        args: &[String],
        commit: bool,
    ) -> Result<Outcome, ExecError> {
        match verb {
            Builtin::Pwd => Ok(Outcome::Cwd(self.cwd.clone())),
            Builtin::Cd => {
                let raw = args.first().map_or("~", String::as_str);
                let target = resolve(raw, &self.cwd)?;
                self.validate_namespace(&target)?;
                self.cwd = target.clone();
                Ok(Outcome::Moved(target))
            }
            Builtin::Describe => self.eval_describe(args),
            _ => {
                let d = desugar(verb, args, &self.cwd, self.facts_for(verb, args))?;
                self.eval_statements(&d.statements, commit)
            }
        }
    }

    /// Evaluate `describe [path]` — the in-session read of a node's contract. Pure: it folds only
    /// the driver's introspective half into a [`qfs_core::DescribeReport`] (the same fold the
    /// one-shot `qfs describe` performs), so it builds no plan, opens no socket, and resolves no
    /// credential. The path defaults to the cwd and is resolved against it, so `describe` answers
    /// for where you already are — the one thing the one-shot form (absolute-path-only, no cwd)
    /// cannot do.
    ///
    /// # Errors
    /// [`ExecError`] (kind `capability`) if the path is unmounted or the driver cannot describe it.
    fn eval_describe(&self, args: &[String]) -> Result<Outcome, ExecError> {
        use qfs_core::{DescribeReport, Path};
        let target = match args.first() {
            Some(p) => resolve(p, &self.cwd)?,
            None => self.cwd.clone(),
        };
        let abs = target.render();
        // 番地の`@選択`: a trailing selection segment names a ROW node — describe the BASE,
        // then refine into the row view (the same fold the one-shot form performs).
        let (base, selection) = qfs_core::split_selection(&abs);
        let (driver, _) = self.engine.mounts.resolve_path(base).ok_or_else(|| {
            ExecError::new(
                ErrorKind::Capability,
                "unknown_mount",
                format!("cannot describe `{abs}`: no driver is mounted there"),
            )
            .with_path(abs.clone())
        })?;
        let report = DescribeReport::from_driver(driver.as_ref(), &Path::new(base))
            .map_err(|e| ExecError::from_qfs(&e))?;
        let report = match selection {
            Some(raw) => report.for_selected_row(&abs, raw).map_err(|e| {
                ExecError::new(ErrorKind::Usage, e.code(), e.to_string()).with_path(abs.clone())
            })?,
            None => report,
        };
        Ok(Outcome::Described(Box::new(report)))
    }

    /// Resolve the describe facts the desugar needs for `verb`'s path operands — the session holds
    /// the registry, so it is the layer that can ask the driver, and the desugar stays pure.
    ///
    /// - `ls [path]` — the target's entry kind, so the desugar picks the blob projection vs the bare
    ///   read (blueprint §5.1). Defaults to the cwd.
    /// - `cp`/`mv <src> <dst>` — BOTH operands, so the desugar can key `cp`'s verb on the
    ///   destination's kind and apply `mv`'s same-kind rule (§9).
    /// - `cat`/`rm` — nothing; their lowering is kind-independent.
    ///
    /// An unmounted or undescribable path yields `None`, and every rule falls back to the shipped
    /// behaviour rather than guessing.
    fn facts_for(&self, verb: Builtin, args: &[String]) -> Facts {
        match verb {
            Builtin::Ls => Facts::of_src(match args.first() {
                Some(p) => self.node_facts(p),
                None => self.facts_of(&self.cwd),
            }),
            Builtin::Cp | Builtin::Mv => Facts {
                src: args.first().and_then(|p| self.node_facts(p)),
                dst: args.get(1).and_then(|p| self.node_facts(p)),
            },
            _ => Facts::default(),
        }
    }

    /// The describe facts of a raw path argument resolved against the cwd.
    fn node_facts(&self, raw: &str) -> Option<NodeFacts> {
        self.facts_of(&resolve(raw, &self.cwd).ok()?)
    }

    /// The describe facts of an already-resolved path, or `None` when unmounted/undescribable.
    fn facts_of(&self, target: &VfsPath) -> Option<NodeFacts> {
        use qfs_core::Path;
        let abs = target.render();
        let (driver, _) = self.engine.mounts.resolve_path(&abs)?;
        let desc = driver.describe(&Path::new(abs)).ok()?;
        Some(NodeFacts::new(desc.archetype, desc.category))
    }

    /// Run a batch of qfs source statements (one for most builtins/raw lines; several for `mv`
    /// and `rm a b c`). A read batch returns a [`Outcome::Listing`]; an effect batch is
    /// previewed (default) or committed, with the affected counts surfaced.
    fn eval_statements(&self, statements: &[String], commit: bool) -> Result<Outcome, ExecError> {
        // Determine read-vs-effect from the FIRST statement; a builtin batch is homogeneous by
        // construction (a read `ls`/`cat`, or an effect `cp`/`mv`/`rm`).
        let first = statements
            .first()
            .ok_or_else(|| ExecError::usage("empty statement batch"))?;
        let stmt = parse(first)?;
        let (inner, line_commit) = unwrap_commit(&stmt);

        if matches!(inner, qfs_parser::Statement::Query(_)) {
            // A read: a single listing (builtins never batch reads).
            let rows = block_on_read(
                inner,
                &self.engine.mounts,
                self.reads,
                &qfs_core::RequestContext::anonymous(),
            )?;
            return Ok(Outcome::Listing(rows));
        }

        // An effect batch. Build every leg's plan first (so a parse/capability error aborts the
        // whole batch before any apply), then preview or commit uniformly.
        let do_commit = commit || line_commit;
        let mut planned = Vec::with_capacity(statements.len());
        for s in statements {
            let parsed = parse(s)?;
            let (eff, c) = unwrap_commit(&parsed);
            // A `COMMIT`-wrapped leg promotes the whole batch to commit (typed intent).
            let _ = c;
            planned.push((s.clone(), build_plan(eff, self.engine)?));
        }

        if do_commit {
            let mut out = Vec::with_capacity(planned.len());
            for (source, plan) in &planned {
                let parsed = parse(source)?;
                let (stmt, _) = unwrap_commit(&parsed);
                let mut to_apply = plan.clone();
                // The interactive shell injects no transform executor yet — a `|> transform`
                // source fails closed in the engine (`transform_no_executor`); the one-shot
                // `qfs run … --commit --commit-irreversible` is the §15 execution surface.
                crate::materialize_pipeline_source(
                    stmt,
                    &mut to_apply,
                    self.engine,
                    self.reads,
                    None,
                )?;
                out.push(apply_via(&to_apply, self.world)?);
            }
            Ok(Outcome::Committed(out))
        } else {
            Ok(Outcome::Preview(
                planned.iter().map(|(_, plan)| plan_preview(plan)).collect(),
            ))
        }
    }

    /// Validate that `target` is a node a `cd` may enter (hard part (a) gate): the path must route
    /// to a mounted driver that describes the node as **navigable** — its children are locations,
    /// not rows. This is the pure, structural capability check (blueprint §6/§9). A driver that
    /// describes the node as a row-bearing leaf yields a structured capability error rather than a
    /// half-applied `cd`.
    ///
    /// # Errors
    /// [`ExecError`] (kind `capability`) if the path is unmounted or not navigable.
    fn validate_namespace(&self, target: &VfsPath) -> Result<(), ExecError> {
        namespace_check(&self.engine.mounts, target)
    }
}

/// Unwrap a (possibly nested) `PREVIEW`/`COMMIT` wrapper, returning the inner statement and
/// whether a `COMMIT` keyword was present. Mirrors the one-shot path's `unwrap_plan`.
fn unwrap_commit(stmt: &qfs_parser::Statement) -> (&qfs_parser::Statement, bool) {
    use qfs_parser::{PlanWrap, Statement};
    let mut cur = stmt;
    let mut commit = false;
    while let Statement::Plan(PlanWrap {
        commit: c, inner, ..
    }) = cur
    {
        commit = commit || *c;
        cur = inner;
    }
    (cur, commit)
}

/// The pure **enumerable-children** gate for `cd` (extracted so it is unit-testable without a
/// `Session`): resolve the target's driver in `mounts` and assert the node it describes is
/// navigable — that its children are LOCATIONS, not rows.
///
/// The predicate is the driver's own describe-contract fact ([`qfs_core::NodeDesc::navigable`]),
/// never a shell-side heuristic: only the driver knows, per path, whether a node is an interior.
/// Keying on the archetype instead was the shipped defect — it admitted exactly
/// `BlobNamespace | ObjectGraphWorkflow`, so `cd /sql/<conn>`, `cd /transform`, `cd /type` and
/// `cd /mail` were all refused as `not_a_namespace` even though their `ls` is meaningful, because a
/// navigable catalog interior and a leaf table report the SAME `RelationalTable` archetype.
/// Refusing a `cd` into a row-set is the part that stays: rows are values, not locations.
///
/// # Errors
/// [`ExecError`] (kind `capability`) if the path is unmounted or describes a non-navigable node.
pub fn namespace_check(mounts: &MountRegistry, target: &VfsPath) -> Result<(), ExecError> {
    use qfs_core::Path;
    let abs = target.render();
    let Some((driver, _)) = mounts.resolve_path(&abs) else {
        return Err(ExecError::new(
            ErrorKind::Capability,
            "unknown_mount",
            format!("cannot cd into `{abs}`: no driver is mounted there"),
        )
        .with_path(abs));
    };
    let desc = driver
        .describe(&Path::new(abs.clone()))
        .map_err(|e| ExecError::from_qfs(&e))?;
    if desc.navigable {
        return Ok(());
    }
    let archetype = desc.archetype;
    Err(ExecError::new(
        ErrorKind::Capability,
        "not_a_namespace",
        format!(
            "cannot cd into `{abs}`: it is a {archetype:?} leaf whose children are rows, not \
             locations you can enter — read it instead (`ls {abs}`)"
        ),
    )
    .with_path(abs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use qfs_core::{
        Archetype, Capabilities, CfsError, Column, ColumnType, Driver, NodeDesc, Path,
        PushdownProfile, Schema,
    };

    // A minimal namespace driver mounted at `/local`, and a leaf-table driver at `/mail`, so the
    // cd namespace gate is exercised without any real filesystem. `navigable` overrides the
    // archetype's default, modelling a driver that states the fact per node (a catalog interior
    // carrying a row-shaped archetype, e.g. `/transform`).
    struct NsDriver {
        mount: String,
        archetype: Archetype,
        navigable: Option<bool>,
    }
    impl NsDriver {
        fn new(mount: &str, archetype: Archetype) -> Self {
            Self {
                mount: mount.into(),
                archetype,
                navigable: None,
            }
        }
        fn navigable(mut self, navigable: bool) -> Self {
            self.navigable = Some(navigable);
            self
        }
    }
    impl Driver for NsDriver {
        fn mount(&self) -> &str {
            &self.mount
        }
        fn describe(&self, _p: &Path) -> Result<NodeDesc, CfsError> {
            let desc = NodeDesc::new(
                self.archetype,
                Schema::new(vec![Column::new("name", ColumnType::Text, false)]),
            );
            Ok(match self.navigable {
                Some(n) => desc.navigable(n),
                None => desc,
            })
        }
        fn capabilities(&self, _p: &Path) -> Capabilities {
            Capabilities::none()
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn qfs_core::PlanApplier {
            unreachable!("namespace gate never applies")
        }
    }

    fn engine() -> Engine {
        let mut e = Engine::new();
        e.mounts
            .register(Arc::new(NsDriver::new("/local", Archetype::BlobNamespace)))
            .unwrap();
        e.mounts
            .register(Arc::new(NsDriver::new("/mail", Archetype::RelationalTable)))
            .unwrap();
        // A catalog INTERIOR that carries a row-shaped archetype — the `/transform`/`/type`/
        // `/sql/<conn>` shape the archetype-pair gate could not distinguish from `/mail` above.
        e.mounts
            .register(Arc::new(
                NsDriver::new("/transform", Archetype::RelationalTable).navigable(true),
            ))
            .unwrap();
        // An append log whose children are locations — the gmail label-tree shape.
        e.mounts
            .register(Arc::new(
                NsDriver::new("/labels", Archetype::AppendLog).navigable(true),
            ))
            .unwrap();
        e
    }

    #[test]
    fn prompt_renders_driver_and_path() {
        let e = engine();
        let reads = ReadRegistry::new();
        let s = Session::new(VfsPath::new("local", vec!["docs".into()]), &e, &reads);
        assert_eq!(s.prompt(), "local:/docs$ ");
        let root = Session::new(VfsPath::root("local"), &e, &reads);
        assert_eq!(root.prompt(), "local:/$ ");
    }

    #[test]
    fn pwd_returns_cwd_without_a_plan() {
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        assert_eq!(
            s.eval_line("pwd", false).unwrap(),
            Outcome::Cwd(VfsPath::root("local"))
        );
    }

    #[test]
    fn cd_into_namespace_moves_cwd() {
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        let out = s.eval_line("cd docs", false).unwrap();
        assert_eq!(
            out,
            Outcome::Moved(VfsPath::new("local", vec!["docs".into()]))
        );
        assert_eq!(s.cwd().render(), "/local/docs");
    }

    #[test]
    fn cd_into_a_row_leaf_is_capability_error() {
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        // `/mail` here is a row-bearing RelationalTable leaf: rows are values, not locations, so cd
        // is rejected and the cwd is unchanged. This is the half of the old gate that must SURVIVE
        // the enumerable-children change.
        let err = s.eval_line("cd /mail", false).unwrap_err();
        assert_eq!(err.kind.as_str(), "capability");
        assert_eq!(err.code, "not_a_namespace");
        assert_eq!(s.cwd().driver(), "local");
    }

    #[test]
    fn cd_into_a_catalog_interior_succeeds_despite_its_row_shaped_archetype() {
        // The slice-2 defect, at the gate level: `/transform` describes as `RelationalTable` — the
        // SAME archetype as the `/mail` row leaf above — so the old archetype-pair gate refused it
        // as `not_a_namespace` even though `ls /transform` is meaningful. The driver's per-node
        // `navigable` fact is what separates the two; the archetype provably cannot.
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        let out = s.eval_line("cd /transform", false).unwrap();
        assert_eq!(out, Outcome::Moved(VfsPath::root("transform")));
        assert_eq!(s.cwd().render(), "/transform");

        // Same archetype, opposite verdict — the assertion that pins WHY the field exists.
        use qfs_core::Path;
        let desc = |p: &str| {
            let (d, _) = e.mounts.resolve_path(p).unwrap();
            d.describe(&Path::new(p)).unwrap()
        };
        assert_eq!(desc("/transform").archetype, desc("/mail").archetype);
        assert!(desc("/transform").navigable);
        assert!(!desc("/mail").navigable);
    }

    #[test]
    fn cd_into_an_append_log_whose_children_are_locations_succeeds() {
        // The gmail label-tree shape: mail rows ARE an append log (the archetype is correct and
        // `ls` depends on it), but `/mail`'s children are LABELS — locations. Navigability is an
        // orthogonal fact, so an AppendLog can be enterable.
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        let out = s.eval_line("cd /labels", false).unwrap();
        assert_eq!(out, Outcome::Moved(VfsPath::root("labels")));
    }

    #[test]
    fn cd_into_unmounted_is_capability_error() {
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        let err = s.eval_line("cd /nope", false).unwrap_err();
        assert_eq!(err.kind.as_str(), "capability");
    }

    #[test]
    fn ls_over_a_non_blob_mount_is_the_bare_read_not_the_blob_projection() {
        // §5.1 regression: `ls` over a RelationalTable mount (`/mail` here) must lower to the bare
        // read (its rows ARE the enumeration), NOT the blob `name/size/is_dir/modified` projection
        // that fails `unknown column` on a non-blob schema. The session resolves the archetype and
        // the desugar picks the bare read.
        let e = engine();
        let reads = ReadRegistry::new();
        let s = Session::new(VfsPath::root("local"), &e, &reads);
        let facts = s.facts_for(Builtin::Ls, &["/mail".into()]);
        assert_eq!(
            facts.src.map(|f| f.archetype),
            Some(Archetype::RelationalTable),
        );
        let d = desugar(Builtin::Ls, &["/mail".into()], &s.cwd, facts).unwrap();
        assert_eq!(d.statements, vec!["/mail"]);
        // A blob mount (`/local`) keeps the projection — and `ls` with no arg describes the cwd.
        assert_eq!(
            s.facts_for(Builtin::Ls, &[]).src.map(|f| f.archetype),
            Some(Archetype::BlobNamespace),
        );
    }

    #[test]
    fn the_session_resolves_both_operands_for_cp_and_mv() {
        // The desugar keys `cp`'s verb on the DESTINATION and `mv`'s rule on BOTH — so the session
        // must describe both operands, not just the first.
        let e = engine();
        let reads = ReadRegistry::new();
        let s = Session::new(VfsPath::root("local"), &e, &reads);
        let facts = s.facts_for(Builtin::Cp, &["/local/a.md".into(), "/mail".into()]);
        assert_eq!(
            facts.src.map(|f| f.archetype),
            Some(Archetype::BlobNamespace)
        );
        assert_eq!(
            facts.dst.map(|f| f.archetype),
            Some(Archetype::RelationalTable)
        );
        // `cat`/`rm` lowering is kind-independent — nothing is resolved for them.
        assert_eq!(
            s.facts_for(Builtin::Rm, &["/local/a.md".into()]),
            Facts::default()
        );
    }

    #[test]
    fn empty_line_is_noop() {
        let e = engine();
        let reads = ReadRegistry::new();
        let mut s = Session::new(VfsPath::root("local"), &e, &reads);
        assert_eq!(s.eval_line("   ", false).unwrap(), Outcome::Empty);
    }
}
