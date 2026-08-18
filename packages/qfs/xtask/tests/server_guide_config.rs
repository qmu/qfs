//! Verified-true ratchet over the server guide's config examples (ticket 20260816161146).
//!
//! `docs/server.md` teaches `qfs serve <config.qfs>` by showing a config. On 2026-08-16 that config,
//! copied verbatim into a file, did not boot: its statements carried no trailing `;`, so the
//! splitter read the whole fence as ONE statement and the parser rejected it. Every reader hit it,
//! and nothing in the tree noticed — the page is generated, and `gen-docs --check` only proves the
//! rendered file matches the generator, never that what the generator renders is true.
//!
//! So this test reads the *committed* page the way a reader does and holds every ` ```qfs ` block
//! to the first half of the real boot path:
//!
//! 1. It **splits** as a `.qfs` document ([`qfs_core::ddl::document::split_document`], the exact
//!    splitter [`qfs_server::Runtime::boot`] runs).
//! 2. Every statement it splits into **parses** on the shipped grammar.
//!
//! This is the sibling of `cookbook_skills.rs` and takes its shape deliberately: the extractor, the
//! statement floor, and the "report every failure, not the first" loop are the same, because the
//! failure mode is the same one — a documented line an agent or a human would paste and be told is
//! wrong.
//!
//! # What this does NOT cover
//!
//! - **Lowering and commit.** A statement that parses can still fail to become a binding — the `do`
//!   form of `create endpoint` in the same ticket parsed and died at lower time with
//!   `CREATE ENDPOINT requires ON '<method> /route'`. Reaching that would mean linking `qfs-server`
//!   here; the shape column it lives in is prose in a markdown table, which no fence-scoped ratchet
//!   sees either way. `crates/cmd/tests/e2e_serve.rs` boots real configs end to end.
//! - **Non-`qfs` fences.** A ` ```sh ` block's `qfs …` invocations are shell, not config.
//! - **The other generated pages.** `docs/language.md` and `docs/drivers.md` are held by
//!   `qfs::docs`'s own goldens plus the cookbook ratchet; widening this one to them is
//!   ticket 20260816161146's open Consideration, not its scope.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qfs_core::ddl::document::split_document;
use qfs_parser::parse_statement;
use std::fs;
use std::path::PathBuf;

/// Floor on extracted statements — guards the extractor against silently breaking (a fence
/// info-string change, a renamed page) and reporting "0 configs, all pass".
const MIN_STATEMENTS: usize = 5;

/// The repo-root `docs/server.md` (this test crate is `packages/qfs/xtask`).
fn server_guide() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("docs/server.md")
}

/// Extract the body of every ` ```qfs ` fenced block, each returned whole — a config file is a
/// document, not a list of lines, so the block is handed to the splitter exactly as a reader would
/// paste it into a file.
fn extract_config_blocks(md: &str) -> Vec<String> {
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
            if !buf.trim().is_empty() {
                out.push(buf.clone());
            }
            continue;
        }
        buf.push_str(line);
        buf.push('\n');
    }
    out
}

#[test]
fn every_server_guide_config_parses() {
    let page = server_guide();
    let md =
        fs::read_to_string(&page).unwrap_or_else(|e| panic!("reading {}: {e}", page.display()));
    let blocks = extract_config_blocks(&md);
    assert!(
        !blocks.is_empty(),
        "no ```qfs config block found in {} — the extractor or the page changed shape",
        page.display()
    );

    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (b, block) in blocks.iter().enumerate() {
        let stmts = match split_document(block) {
            Ok(stmts) => stmts,
            Err(e) => {
                failures.push(format!(
                    "config block {b}: does not tokenize at line {}: {} [{}]",
                    e.line, e.message, e.code
                ));
                continue;
            }
        };
        for (line, src) in stmts {
            total += 1;
            if let Err(e) = parse_statement(&src) {
                failures.push(format!(
                    "config block {b}, line {line}: {} [{}]\n    {src}",
                    e.message,
                    e.code.as_str()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} documented server config statement(s) do not parse — a reader copying the guide gets \
         an error, so fix the GENERATOR (`crates/qfs/src/docs.rs`) and re-run `cargo run -p xtask \
         -- gen-docs`:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        total >= MIN_STATEMENTS,
        "only {total} config statements extracted from {} (floor {MIN_STATEMENTS}) — the \
         extractor is probably broken, not the page",
        page.display()
    );
}

/// The ratchet must FAIL on the exact defect it was written for: two `;`-less bindings on two
/// lines are one statement, and that statement does not parse. Without this, a future extractor
/// change could make `every_server_guide_config_parses` vacuous and nothing would say so.
#[test]
fn a_semicolonless_config_is_rejected() {
    let md = "```qfs\n\
              create policy readmail ALLOW select ON mail\n\
              create endpoint recent on 'GET /recent' policy readmail as /mail/inbox\n\
              ```\n";
    let blocks = extract_config_blocks(md);
    assert_eq!(blocks.len(), 1, "extractor should see one block");
    let stmts = split_document(&blocks[0]).expect("it tokenizes; it just does not parse");
    assert_eq!(
        stmts.len(),
        1,
        "no `;` means the two lines are ONE statement — that is the defect"
    );
    assert!(
        parse_statement(&stmts[0].1).is_err(),
        "the merged statement must be rejected, or this ratchet proves nothing"
    );
}
