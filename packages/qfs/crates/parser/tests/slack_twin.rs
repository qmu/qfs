//! §13 self-hosting ratchet — the first conversion. `slack.qfs` (the script twin of the compiled
//! Slack driver) parses statement-for-statement and desugars to `/sys/drivers` installs. Hermetic:
//! parse + desugar only, no network, no credentials. The declared-vs-compiled read/post ROW
//! comparison lives in the qfs crate's evaluator tests; the recorded parity gaps are on the ticket.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qfs_parser::{parse_statement, Statement};

/// Split a `.qfs` script into statements: drop `--` comment lines, then group runs of non-blank
/// lines (a statement's clauses span multiple indented lines) separated by blank lines.
fn statements(src: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for line in src.lines() {
        let l = line.trim_end();
        if l.trim_start().starts_with("--") {
            continue;
        }
        if l.trim().is_empty() {
            if !cur.is_empty() {
                stmts.push(cur.join("\n"));
                cur.clear();
            }
        } else {
            cur.push(l.to_string());
        }
    }
    if !cur.is_empty() {
        stmts.push(cur.join("\n"));
    }
    stmts
}

#[test]
fn slack_qfs_parses_and_installs_to_sys_drivers() {
    let src = include_str!("fixtures/slack.qfs");
    let stmts = statements(src);
    assert_eq!(stmts.len(), 4, "driver + type + view + map: {stmts:?}");
    for s in &stmts {
        let stmt = parse_statement(s)
            .unwrap_or_else(|e| panic!("slack.qfs statement failed to parse:\n{s}\n{e}"));
        // Every §13 declaration desugars to an `INSERT INTO /sys/drivers` effect (no new Statement
        // variant): the ratchet installs the twin exactly like any other script.
        match stmt {
            Statement::Effect(e) => {
                let path: String = e
                    .target
                    .segments
                    .iter()
                    .map(|seg| format!("/{}", seg.name))
                    .collect();
                assert_eq!(path, "/sys/drivers", "desugars to /sys/drivers:\n{s}");
            }
            other => panic!("expected the §13 desugar (an Effect), got {other:?} for:\n{s}"),
        }
    }
}
