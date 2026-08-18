//! Verified-true ratchet over the cookbook Agent Skills (ticket: cookbook-articles-as-agent-skills).
//!
//! Each `docs/cookbook/*.md` article is the authored source of a generated Claude Code skill
//! (`plugins/qfs/skills/<name>/SKILL.md`, its body copied VERBATIM by `xtask gen-skills`). So every
//! runnable `qfs` recipe an agent would read out of a skill must actually be true of the shipped
//! binary — a skill can never teach an agent a statement the binary rejects. This test extracts the
//! ` ```qfs ` fenced statements from the source articles and holds them to TWO ratchets:
//!
//! 1. **It parses** on the shipped grammar.
//! 2. **Its columns exist**, checked against the binary's own cred-free describe registry
//!    ([`qfs::describe::compiled_describe_registry`], the same one `gen-docs` renders from).
//!
//! Ratchet 2 exists because ratchet 1 is the weakest possible reading of the promise, and it let a
//! whole article ship false: on 2026-07-25 `docs/cookbook/github.md` documented thirteen PR/issue
//! columns that do not exist in `driver-github`'s DTOs, across eleven recipes. Every one parsed, so
//! the ratchet was green the entire time (ticket 20260725143000).
//!
//! This test lives in `xtask` rather than in `qfs-test` (its previous home) or in the binary crate:
//! ratchet 2 needs the compiled describe registry, `qfs-test` is a wasm-clean pure harness that
//! must not link the binary, and the binary's own dep guard pins it off `qfs-parser` — so the one
//! place that may hold both is the anti-drift tooling crate that already links `qfs` for gen-docs
//! and ships in nothing. Keeping BOTH ratchets in one file keeps one extractor and one floor.
//!
//! This is the narrative-cookbook sibling of `roadmap_cookbook.rs` (which ratchets the broad
//! `query-cookbook.md` catalogue). Testing the ARTICLES rather than the generated skills keeps the
//! check independent of whether `gen-skills` has been run, and the skill body equals the article
//! body by construction.
//!
//! # What this does NOT cover
//!
//! Stated here so an uncovered recipe is a known gap rather than a silent pass (ticket
//! 20260817105331). Ratchet 1 sees every ` ```qfs ` statement in every article; ratchet 2 is the
//! narrower one:
//!
//! - **Sources with no compiled describe surface are skipped** — anything whose schema comes from a
//!   live registration rather than a compiled type: `/sql/<conn>[/<table>]`, `/git/<repo>/…`,
//!   `/slack/<workspace>/…`, `/server/…`, and every declared (`.qfs`) mount (`/chatwork/…`,
//!   `/cloudflare/…`). At the time of writing that is 19 of the 81 query sources in the articles,
//!   leaving 62 typechecked. Skips are PRINTED by path (the `skipped` report in
//!   [`every_cookbook_skill_recipe_names_columns_that_exist`]) and floored by `MIN_TYPECHECKED`, so
//!   "skipped everything" can never read as green.
//! - **Only the checkable PREFIX of a pipeline is checked** — the walk stops at the first stage that
//!   reshapes the relation from somewhere this test does not model
//!   (see [`ends_the_checkable_prefix`]: a codec, join, set op, `CALL`, `TRANSFORM`, `SWITCH`,
//!   `FOLLOW`, `POST`). Columns named after such a stage are unchecked.
//! - **Only queries** — `Statement::Query`. Effects (`UPSERT`/`REMOVE`/`CALL`) and DDL are
//!   parse-checked by ratchet 1 and typechecked by neither.
//! - **`known` only ever grows**, so an alias introduced by `SELECT … AS`/`EXTEND`/`SET`/`AS` is
//!   accepted everywhere afterwards rather than scoped to where it is in fact defined. The check can
//!   under-report; it must never invent a red on a true recipe.
//! - **Shell examples are not recipes.** A ` ```sh ` block's `qfs …` invocations are invisible here;
//!   `crates/cmd/tests/faq_cli_surface.rs` holds those (in `docs/cookbook/faq.md`) against the real
//!   clap surface.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qfs_core::{Path as QfsPath, Schema};
use qfs_parser::{parse_statement, Expr, PipeOp, Pipeline, Projection, Source, Statement};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// Floor on extracted statements — guards the extractor against silently breaking (e.g. a fenced
/// info-string change) and reporting "0 recipes, all pass".
const MIN_STATEMENTS: usize = 45;

/// The repo-root `docs/cookbook` dir (this test crate is `packages/qfs/xtask`).
fn cookbook_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("docs/cookbook")
}

/// Extract the qfs STATEMENTS from a markdown article: the body of every ` ```qfs ` fenced block,
/// split on blank lines (a block may hold more than one statement, e.g. draft-then-send). Prose
/// placeholders like `<msg-id>` live OUTSIDE the fenced blocks, so what is extracted here is real,
/// runnable qfs source.
fn extract_statements(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    let mut buf = String::new();
    for line in md.lines() {
        if !in_block {
            if line.trim_start().starts_with("```qfs") {
                in_block = true;
                buf.clear();
            }
            continue;
        }
        if line.trim_start().starts_with("```") {
            in_block = false;
            for stmt in buf.split("\n\n") {
                let stmt = stmt.trim();
                if !stmt.is_empty() {
                    out.push(stmt.to_string());
                }
            }
            continue;
        }
        buf.push_str(line);
        buf.push('\n');
    }
    out
}

#[test]
fn every_cookbook_skill_recipe_parses() {
    let dir = cookbook_dir();
    let mut articles: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    articles.sort();

    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for article in &articles {
        let md = fs::read_to_string(article)
            .unwrap_or_else(|e| panic!("reading {}: {e}", article.display()));
        for stmt in extract_statements(&md) {
            total += 1;
            if let Err(e) = parse_statement(&stmt) {
                failures.push(format!("{}: {:?} → {e}", article.display(), stmt));
            }
        }
    }

    assert!(
        total >= MIN_STATEMENTS,
        "only {total} qfs statements extracted from the cookbook (< {MIN_STATEMENTS}); the \
         extractor or the articles changed shape"
    );
    assert!(
        failures.is_empty(),
        "{} of {total} cookbook-skill recipes do NOT parse on the shipped grammar (a skill must \
         never teach an agent an invalid statement):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- Ratchet 2: the columns a recipe names must exist on the node it addresses ----

/// Floor on TYPECHECKED statements. Ratchet 2 can only run over paths the compiled describe
/// registry resolves, so a skip is legitimate — but "skipped everything, all pass" must never read
/// as green. This is the `MIN_STATEMENTS` idea applied to the second ratchet.
const MIN_TYPECHECKED: usize = 20;

/// The stages after which the relation's columns are no longer knowable from the node's describe
/// schema alone, so ratchet 2 stops checking rather than guess. A codec reports an undetermined
/// schema by design (blueprint §5.3); a join/set-op/call/transform/switch reshapes it from
/// somewhere this test does not model.
fn ends_the_checkable_prefix(op: &PipeOp) -> bool {
    matches!(
        op,
        PipeOp::Decode(_)
            | PipeOp::Encode(_)
            | PipeOp::Join(_)
            | PipeOp::Union(_)
            | PipeOp::Except(_)
            | PipeOp::Intersect(_)
            | PipeOp::Call(_)
            | PipeOp::Transform(_)
            | PipeOp::Switch(_)
            | PipeOp::Follow(_)
            | PipeOp::Post(_)
    )
}

/// Every column name an expression READS. Only the head of a struct-navigation path is a column
/// (`author.login` reads `author`); literals and nested function args recurse.
fn columns_read(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Col(name) => out.push(name.clone()),
        Expr::Path(segs) => {
            if let Some(head) = segs.first() {
                out.push(head.clone());
            }
        }
        Expr::Fn(f) => f.args.iter().for_each(|a| columns_read(a, out)),
        Expr::Binary { lhs, rhs, .. } => {
            columns_read(lhs, out);
            columns_read(rhs, out);
        }
        Expr::Unary { expr, .. } => columns_read(expr, out),
        Expr::In { expr, set } => {
            columns_read(expr, out);
            set.iter().for_each(|e| columns_read(e, out));
        }
        Expr::Between { expr, low, high } => {
            columns_read(expr, out);
            columns_read(low, out);
            columns_read(high, out);
        }
        Expr::Lit(_) => {}
        // The AST is #[non_exhaustive]-in-spirit: an unmodelled arm reads nothing rather than
        // failing a true recipe. Ratchet 2 must never produce a false red.
        _ => {}
    }
}

/// The node schema the compiled describe registry reports for a source path, or `None` when the
/// path has no compiled describe surface (a `/sql/<conn>/<table>`, a `/git/<repo>`, a declared
/// mount) — those need a live registration, never a compiled type.
fn described_schema(reg: &qfs_core::MountRegistry, source: &Source) -> Option<(String, Schema)> {
    let Source::Path(path_expr) = source else {
        return None;
    };
    let rendered = format!(
        "/{}",
        path_expr
            .segments
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
            .join("/")
    );
    // The FULL path, not `resolve_path`'s mount-stripped remainder: a driver's describe reads the
    // whole address (`/github/{owner}/{repo}/<ns>`), which is what `catalog.rs` — the gen-docs
    // renderer this ratchet must agree with — passes. Handing it the stripped form makes every
    // /github node unresolvable and every /mail node report the message schema, so the ratchet
    // would have skipped exactly the class it exists to catch and mis-flagged two true recipes.
    let (driver, _) = reg.resolve_path(&rendered)?;
    let desc = driver.describe(&QfsPath::new(rendered.clone())).ok()?;
    if desc.schema.columns.is_empty() {
        return None;
    }
    Some((rendered, desc.schema))
}

/// Check one pipeline's checkable prefix. Returns `Ok(())` for a clean or unresolvable recipe, or
/// the offending column names. `known` starts as the node's columns and only ever GROWS (an
/// `EXTEND`/`SET`/`AS` alias introduces a name), so the check can under-report but never invent a
/// failure.
fn unknown_columns(
    reg: &qfs_core::MountRegistry,
    pipeline: &Pipeline,
) -> Option<(String, Vec<String>)> {
    let (path, schema) = described_schema(reg, &pipeline.source)?;
    let mut known: BTreeSet<String> = schema
        .columns
        .iter()
        .map(|c| c.name.as_str().to_string())
        .collect();
    let mut bad: Vec<String> = Vec::new();
    let check = |exprs: Vec<&Expr>, known: &BTreeSet<String>, bad: &mut Vec<String>| {
        for e in exprs {
            let mut read = Vec::new();
            columns_read(e, &mut read);
            for name in read {
                if !known.contains(&name) {
                    bad.push(name);
                }
            }
        }
    };

    for op in &pipeline.ops {
        if ends_the_checkable_prefix(op) {
            break;
        }
        match op {
            PipeOp::Where(e) => check(vec![e], &known, &mut bad),
            PipeOp::GroupBy(es) => check(es.iter().collect(), &known, &mut bad),
            PipeOp::OrderBy(keys) => {
                check(keys.iter().map(|k| &k.expr).collect(), &known, &mut bad)
            }
            PipeOp::Select(projs) | PipeOp::Aggregate(projs) => {
                let exprs: Vec<&Expr> = projs
                    .iter()
                    .filter_map(|p| match p {
                        Projection::Expr { expr, .. } => Some(expr),
                        Projection::Star => None,
                    })
                    .collect();
                check(exprs, &known, &mut bad);
                for p in projs {
                    if let Projection::Expr { alias: Some(a), .. } = p {
                        known.insert(a.clone());
                    }
                }
            }
            PipeOp::Extend(assigns) | PipeOp::Set(assigns) => {
                check(assigns.iter().map(|a| &a.value).collect(), &known, &mut bad);
                for a in assigns {
                    known.insert(a.name.clone());
                }
            }
            PipeOp::Expand(field) => {
                if let Some(head) = field.first() {
                    if !known.contains(head) {
                        bad.push(head.clone());
                    }
                }
            }
            PipeOp::As(alias) => {
                known.insert(alias.clone());
            }
            _ => {}
        }
    }
    Some((path, bad))
}

#[test]
fn every_cookbook_skill_recipe_names_columns_that_exist() {
    // Ticket 20260725143000. Parsing was the weakest reading of "verified true": eleven
    // `docs/cookbook/github.md` recipes named thirteen columns `driver-github` does not carry, and
    // every one parsed. This ratchet resolves each recipe's source path in the binary's OWN
    // cred-free describe registry and asserts the columns it names are really there.
    let reg = qfs::describe::compiled_describe_registry();
    let dir = cookbook_dir();
    let mut articles: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    articles.sort();

    let mut typechecked = 0usize;
    let mut skipped: BTreeSet<String> = BTreeSet::new();
    let mut failures: Vec<String> = Vec::new();

    for article in &articles {
        let md = fs::read_to_string(article)
            .unwrap_or_else(|e| panic!("reading {}: {e}", article.display()));
        for stmt_src in extract_statements(&md) {
            let Ok(stmt) = parse_statement(&stmt_src) else {
                continue; // ratchet 1 owns the parse failure; do not report it twice
            };
            let Statement::Query(pipeline) = &stmt else {
                continue; // effects and DDL are out of this ratchet's scope
            };
            match unknown_columns(&reg, pipeline) {
                None => {
                    skipped.insert(source_label(&pipeline.source));
                }
                Some((path, bad)) => {
                    typechecked += 1;
                    if !bad.is_empty() {
                        failures.push(format!(
                            "{}: {stmt_src:?}\n    names {bad:?} — not columns of {path}",
                            article.display()
                        ));
                    }
                }
            }
        }
    }

    // Skips are reported by path, never silently dropped (Quality Gate item 3).
    println!(
        "typechecked {typechecked} recipe(s); skipped {} source(s) with no compiled describe \
         surface: {:?}",
        skipped.len(),
        skipped
    );

    assert!(
        typechecked >= MIN_TYPECHECKED,
        "only {typechecked} cookbook recipes were typechecked (< {MIN_TYPECHECKED}); the describe \
         registry, the resolver or the articles changed shape and 'skipped everything' is now \
         reading as green. Skipped: {skipped:?}"
    );
    assert!(
        failures.is_empty(),
        "{} cookbook recipe(s) name columns the binary's describe registry does not carry (a \
         skill must never teach an agent a column that does not exist):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A stable label for a skipped source, so the skip report names paths rather than counts.
fn source_label(source: &Source) -> String {
    match source {
        Source::Path(p) => format!(
            "/{}",
            p.segments
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
                .join("/")
        ),
        Source::Values(_) => "<values>".to_string(),
        Source::Subquery(_) => "<subquery>".to_string(),
        Source::Name(n) => format!("<let {n}>"),
    }
}
