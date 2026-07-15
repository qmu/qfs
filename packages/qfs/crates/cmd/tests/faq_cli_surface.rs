//! Anti-drift: every `qfs …` shell command shown in `docs/cookbook/faq.md` must exist in the real
//! clap surface (ticket 20260706163522).
//!
//! The FAQ skill answers most operator questions with SHELL commands (`qfs connect --driver … --
//! account …`, `qfs account add google --app qmu`) because that is how connections are actually made. The
//! `cookbook_skills.rs` recipe ratchet only parse-checks ```` ```qfs ```` *statement* recipes — it
//! never sees shell commands, so the single most important part of the FAQ (the connection-setup
//! answers) had no machine guarantee of staying true when the CLI surface changes. This test closes
//! that gap: it extracts every `qfs <subcommand…> [--flags]` from the FAQ's ```` ```sh ```` fences
//! and asserts each subcommand path and long flag is real, by walking the clap `Command` tree
//! `qfs_cmd::clap_command()` exposes. A renamed or removed flag the FAQ cites turns this red.
//!
//! Home: this lives in `crates/cmd/tests/` (not beside `cookbook_skills.rs` in `crates/test/`) on
//! purpose — `qfs-test` is a PURE, wasm-clean, never-linked-into-the-binary harness (proved by its
//! `dev_only_dep_graph.rs`), so it must not gain a `qfs-cmd`/clap dependency. The CLI surface is
//! owned by `qfs-cmd`, so the check that reflects over it belongs here; it still runs under the
//! same `cargo test --workspace` a developer and CI both use.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use clap::Command;

/// Floor on extracted `qfs` invocations — guards the extractor from silently matching nothing (a
/// fenced info-string change) and reporting "0 commands, all exist".
const MIN_QFS_COMMANDS: usize = 12;

/// The repo-root FAQ article (this crate is `packages/qfs/crates/cmd`).
fn faq_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .join("docs/cookbook/faq.md")
}

/// The body lines of every ```` ```sh ```` fenced block in a markdown article.
fn sh_fence_lines(md: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_sh = false;
    for line in md.lines() {
        let trimmed = line.trim_start();
        if !in_sh {
            if trimmed.starts_with("```sh") {
                in_sh = true;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            in_sh = false;
            continue;
        }
        lines.push(line.to_string());
    }
    lines
}

/// Split one shell line into tokens, honoring single/double quotes so a quoted qfs statement
/// (`"/mail/inbox |> select date"`) is ONE token and its inner `|` never looks like a shell pipe.
/// An unquoted `#` starts a comment (the rest of the line is dropped). Quote characters are
/// removed; their contents stay in the token.
fn tokenize(line: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut has_tok = false;
    for c in line.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                has_tok = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_tok = true;
            }
            '#' if !in_single && !in_double => break, // comment to end of line
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_tok {
                    toks.push(std::mem::take(&mut cur));
                    has_tok = false;
                }
            }
            c => {
                cur.push(c);
                has_tok = true;
            }
        }
    }
    if has_tok {
        toks.push(cur);
    }
    toks
}

/// Whether a token is a shell command separator (ends one command's argv). A quoted qfs statement
/// keeps its `|>` inside a single token, so only a BARE `|` / `||` / `&&` / `;` / redirect splits.
fn is_separator(tok: &str) -> bool {
    matches!(tok, "|" | "||" | "&&" | ";" | ">" | ">>" | "<")
}

/// From a token stream, pull each `qfs …` invocation's argv — the tokens after a `qfs` token, up
/// to the next shell separator (so `cat x | qfs app add google qmu` yields `["app","add","google","qmu"]`).
fn qfs_invocations(tokens: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "qfs" {
            let mut argv = Vec::new();
            i += 1;
            while i < tokens.len() && !is_separator(&tokens[i]) {
                argv.push(tokens[i].clone());
                i += 1;
            }
            out.push(argv);
        } else {
            i += 1;
        }
    }
    out
}

/// The long flag names declared as GLOBAL on the root command (`--json`, `--no-color`) — accepted
/// on any subcommand (clap propagates them at parse time; the un-built tree does not, so we union
/// them in explicitly).
fn global_long_flags(root: &Command) -> BTreeSet<String> {
    root.get_arguments()
        .filter(|a| a.is_global_set())
        .filter_map(|a| a.get_long())
        .map(String::from)
        .collect()
}

/// Verify one `qfs` argv against the clap tree. Resolves the subcommand path (leading non-flag
/// tokens that name child subcommands — the FIRST such token is mandatory, since `qfs` has no
/// top-level positional), then asserts every remaining `--long` flag exists at that leaf or is a
/// global. Positional VALUES (`google`, `/drive`, a quoted statement) and short flags are ignored.
fn verify(root: &Command, globals: &BTreeSet<String>, argv: &[String]) -> Result<(), String> {
    let joined = argv.join(" ");
    let mut cur = root;
    let mut idx = 0;
    let mut depth = 0usize;
    while idx < argv.len() {
        let tok = &argv[idx];
        if tok.starts_with('-') {
            break; // a flag ends the subcommand path
        }
        match cur.find_subcommand(tok.as_str()) {
            Some(sub) => {
                cur = sub;
                idx += 1;
                depth += 1;
            }
            None => {
                if depth == 0 {
                    return Err(format!("`qfs {joined}`: unknown subcommand `{tok}`"));
                }
                break; // a positional value at a deeper level — stop resolving
            }
        }
    }

    let mut allowed: BTreeSet<String> = cur
        .get_arguments()
        .filter_map(|a| a.get_long())
        .map(String::from)
        .collect();
    allowed.extend(globals.iter().cloned());

    for tok in &argv[idx..] {
        if let Some(rest) = tok.strip_prefix("--") {
            if rest.is_empty() {
                continue; // a bare `--` end-of-options marker
            }
            let name = rest.split('=').next().unwrap_or(rest);
            if !allowed.contains(name) {
                return Err(format!(
                    "`qfs {joined}`: unknown flag `--{name}` (not in the clap surface)"
                ));
            }
        }
        // short flags and positional values are not checked (ticket: long flags only, ignore values)
    }
    Ok(())
}

#[test]
fn faq_shell_commands_exist_in_cli() {
    let md = std::fs::read_to_string(faq_path())
        .unwrap_or_else(|e| panic!("reading {}: {e}", faq_path().display()));
    let root = qfs_cmd::clap_command();
    let globals = global_long_flags(&root);

    let mut invocations = Vec::new();
    for line in sh_fence_lines(&md) {
        invocations.extend(qfs_invocations(&tokenize(&line)));
    }

    assert!(
        invocations.len() >= MIN_QFS_COMMANDS,
        "only {} `qfs …` commands extracted from docs/cookbook/faq.md (< {MIN_QFS_COMMANDS}); the \
         sh-fence extractor or the FAQ changed shape",
        invocations.len()
    );

    let mut failures = Vec::new();
    for argv in &invocations {
        if argv.is_empty() {
            continue;
        }
        if let Err(e) = verify(&root, &globals, argv) {
            failures.push(e);
        }
    }
    assert!(
        failures.is_empty(),
        "{} FAQ `qfs …` command(s) cite a subcommand/flag the binary does not expose (the FAQ has \
         drifted from the CLI surface — fix docs/cookbook/faq.md):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- Checker guards: prove the check is not vacuous (it goes red on real drift). ----------------

#[test]
fn accepts_a_real_command_with_real_flags() {
    let root = qfs_cmd::clap_command();
    let globals = global_long_flags(&root);
    let argv: Vec<String> = "connect /drive --driver gdrive --account you@x.com"
        .split_whitespace()
        .map(String::from)
        .collect();
    assert!(verify(&root, &globals, &argv).is_ok());
    // A nested subcommand path resolves (`app add`).
    let nested: Vec<String> = "app add google qmu"
        .split_whitespace()
        .map(String::from)
        .collect();
    assert!(verify(&root, &globals, &nested).is_ok());
    // A global flag is accepted on a subcommand.
    let global: Vec<String> = "describe /drive --json"
        .split_whitespace()
        .map(String::from)
        .collect();
    assert!(verify(&root, &globals, &global).is_ok());
}

#[test]
fn rejects_an_unknown_flag() {
    let root = qfs_cmd::clap_command();
    let globals = global_long_flags(&root);
    // `--acct` does not exist — a rename of `--account` must be caught (the ticket's negative proof).
    let argv: Vec<String> = "connect /drive --driver gdrive --acct you@x.com"
        .split_whitespace()
        .map(String::from)
        .collect();
    assert!(
        verify(&root, &globals, &argv).is_err(),
        "an unknown flag must be rejected"
    );
}

#[test]
fn rejects_an_unknown_subcommand() {
    let root = qfs_cmd::clap_command();
    let globals = global_long_flags(&root);
    let argv: Vec<String> = "bogus --list"
        .split_whitespace()
        .map(String::from)
        .collect();
    assert!(
        verify(&root, &globals, &argv).is_err(),
        "an unknown subcommand must be rejected"
    );
}
