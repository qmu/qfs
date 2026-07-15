//! **Command-execution governance lock** (ticket 20260711121536).
//!
//! Turns "qfs has no command-execution risk" from an audit claim into a *mechanically enforced
//! invariant* — the same move `transform_one_seam_lock` made for model calls. This test scans the
//! workspace's production source (every `crates/*/src` and `xtask/src` `.rs` file, MINUS test
//! harnesses) for process-spawn sites (`Command::new(...)`) and asserts the set of
//! `(file, spawned-program)` pairs matches an EXACT allowlist.
//!
//! A future driver, transform output, declared-driver evaluation, or codec cannot quietly grow an
//! exec path from query text or fetched data: the moment a new `Command::new` appears (or an
//! existing site changes the program it spawns, or multiplies), this test fails with a message
//! demanding a deliberate allowlist edit — which forces the blueprint's exec-risk section
//! (`§ Command-execution assurance`) to be revisited in the same PR.
//!
//! ## Why an allowlist of `(file, program)` and not just a count
//! Every allowlisted spawn today runs a FIXED program (`git`, or the platform desktop `OPENER`
//! constant) with argv built from FIXED literals + validated tokens (hex oids, `refs/heads/`-
//! prefixed ref names — see `driver-git`'s `qualify_ref` / `Oid::parse` and the hygiene tests
//! there). NONE takes a program name or an unsanitized argument derived from query text or fetched
//! bytes, and there is no `sh -c` / shell-string interpolation anywhere (asserted below). The
//! allowlist pins that exact shape; a diff to it is the review signal.

// Test code: assertions and setup may panic/expect/unwrap freely.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The workspace root (`packages/qfs`), derived from this integration test's crate manifest dir
/// (`packages/qfs/crates/cmd`) — no PATH or CWD assumptions.
fn workspace_root() -> PathBuf {
    let cmd_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    cmd_manifest
        .parent() // crates/
        .and_then(Path::parent) // packages/qfs
        .expect("cmd crate sits two levels under the workspace root")
        .to_path_buf()
}

/// Recursively collect `.rs` files under `dir` whose path contains a `/src/` segment, EXCLUDING
/// test harnesses: any file named `tests.rs` and anything under a `/tests/` directory (integration
/// tests spawn `cargo`/`git` legitimately — they are not the production attack surface this lock
/// guards).
fn production_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip build artifacts and integration-test dirs outright.
            if name == "target" || name == "tests" {
                continue;
            }
            production_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_under_src = path.components().any(|c| c.as_os_str() == "src");
            if is_under_src && name != "tests.rs" {
                out.push(path);
            }
        }
    }
}

/// Remove `#[cfg(test)] mod <name> { ... }` blocks from `src` by brace matching, so an in-file unit
/// test module that legitimately spawns `git`/`cargo` in a fixture does not read as a production
/// spawn site. Brace-counting is sufficient for this codebase (test-module headers carry no
/// unbalanced braces in strings); a production spawn never lives inside such a module.
fn strip_cfg_test_modules(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut kept = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("#[cfg(test)]") {
            // Look ahead for the module header (allow attributes/blank lines between).
            let mut j = i + 1;
            while j < lines.len() && !lines[j].contains("mod ") && lines[j].trim().is_empty() {
                j += 1;
            }
            if j < lines.len() && lines[j].contains("mod ") && lines[j].contains('{') {
                // Enter the module; skip until the brace opened here closes.
                let mut depth: i32 = 0;
                let mut k = j;
                loop {
                    let l = lines[k];
                    depth += l.matches('{').count() as i32;
                    depth -= l.matches('}').count() as i32;
                    k += 1;
                    if depth <= 0 || k >= lines.len() {
                        break;
                    }
                }
                i = k;
                continue;
            }
        }
        kept.push_str(line);
        kept.push('\n');
        i += 1;
    }
    kept
}

/// Every `Command::new(<token>)` occurrence: `(program-token, ())`. The token is the raw text
/// between the parens (`"git"`, `OPENER`, …).
fn spawn_tokens(src: &str) -> Vec<String> {
    let needle = "Command::new(";
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = src[from..].find(needle) {
        let start = from + rel + needle.len();
        // Read to the matching close paren (tokens here are single, paren-free).
        let mut end = start;
        while end < bytes.len() && bytes[end] != b')' {
            end += 1;
        }
        out.push(src[start..end].trim().to_string());
        from = end;
    }
    out
}

#[test]
fn every_process_spawn_site_is_on_the_allowlist() {
    let root = workspace_root();
    let mut files = Vec::new();
    production_rs_files(&root.join("crates"), &mut files);
    production_rs_files(&root.join("xtask"), &mut files);
    files.sort();

    // The EXACT set of production spawn sites, keyed by workspace-relative file path, valued by the
    // multiset of programs spawned there. Adding/removing/renaming a spawn site requires a
    // deliberate edit here AND a matching edit to the blueprint's exec-risk section.
    //
    //   * driver-git/applier.rs — the git-CLI COMMIT applier (`git hash-object -w --stdin`, atomic
    //     `git update-ref` CAS, `rev-parse`); argv is fixed literals + hex oids + `refs/heads/`-
    //     qualified ref names (see the hygiene tests in that crate). Piped stdio, never a shell.
    //   * qfs/src/git.rs — the `/git` read facet's repo introspection (`git -C <path> cat-file /
    //     show-ref`); oids are `Oid::parse`-validated hex, the path is operator config, not query
    //     text.
    //   * qfs/src/migration_guard.rs — release-tag / shipped-migration introspection (`git`),
    //     developer-tooling only, no query-derived argv.
    //   * qfs/src/tty.rs — the desktop opener (`OPENER` = the platform `open`/`xdg-open` constant),
    //     Stdio::null, launched with a single operator-facing URL/path, never fetched data.
    let mut expected: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    expected.insert(
        "crates/driver-git/src/applier.rs",
        vec!["\"git\"", "\"git\""],
    );
    expected.insert("crates/qfs/src/git.rs", vec!["\"git\"", "\"git\""]);
    expected.insert(
        "crates/qfs/src/migration_guard.rs",
        vec!["\"git\"", "\"git\""],
    );
    expected.insert("crates/qfs/src/tty.rs", vec!["OPENER"]);
    //   * xtask/src/main.rs — BUILD-ONLY release tooling (`publish = false`, a TERMINAL_LEAVES
    //     crate never shipped in any artifact, per the dep-direction guard): `cmd` spawns the
    //     platform checksum tool (`sha256sum` / `shasum -a 256`) over release tarballs, and
    //     `program` is the generic packaging runner (tar / git archive) invoked from xtask's own
    //     literals. It never runs inside the shipped binary and never sees query text or fetched
    //     bytes — outside the runtime attack surface, allowlisted for completeness of the scan.
    expected.insert("xtask/src/main.rs", vec!["cmd", "program"]);

    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in &files {
        let src = std::fs::read_to_string(file).unwrap();
        let src = strip_cfg_test_modules(&src);
        let tokens = spawn_tokens(&src);
        if tokens.is_empty() {
            continue;
        }
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        found.entry(rel).or_default().extend(tokens);
    }
    for v in found.values_mut() {
        v.sort();
    }

    let expected_norm: BTreeMap<String, Vec<String>> = expected
        .iter()
        .map(|(k, v)| {
            let mut vv: Vec<String> = v.iter().map(|s| s.to_string()).collect();
            vv.sort();
            (k.to_string(), vv)
        })
        .collect();

    assert_eq!(
        found, expected_norm,
        "\n\nCOMMAND-EXECUTION LOCK TRIPPED.\n\
         The set of `Command::new(...)` spawn sites in production source no longer matches the \
         allowlist in this test.\n\
         If you added, removed, or changed a process-spawn site, this is a DELIBERATE security \
         decision:\n\
         1. Confirm the new site takes a FIXED program and NO argument derived from query text or \
         fetched bytes (no `sh -c`, no shell-join, no program name from data).\n\
         2. Add argument-hygiene coverage for it (see driver-git's ref/oid hygiene tests).\n\
         3. Update the allowlist here AND the blueprint's `Command-execution assurance` section in \
         the SAME PR.\n\n\
         found:    {found:#?}\n\
         expected: {expected_norm:#?}\n"
    );
}

#[test]
fn no_shell_string_execution_anywhere_in_production_source() {
    // No path from data to a shell: forbid `sh -c` / `bash -c` / `Command::new("sh"|"bash"|
    // "cmd"|"powershell")` in production source. A shell string is the one construct that would let
    // query text or fetched bytes become code; its absence is the load-bearing property.
    let root = workspace_root();
    let mut files = Vec::new();
    production_rs_files(&root.join("crates"), &mut files);
    production_rs_files(&root.join("xtask"), &mut files);

    let forbidden = [
        "Command::new(\"sh\")",
        "Command::new(\"bash\")",
        "Command::new(\"cmd\")",
        "Command::new(\"powershell\")",
        "Command::new(\"/bin/sh\")",
    ];
    for file in &files {
        let src = std::fs::read_to_string(file).unwrap();
        let src = strip_cfg_test_modules(&src);
        for f in forbidden {
            assert!(
                !src.contains(f),
                "shell-execution construct `{f}` found in {} — no production spawn site may invoke \
                 a shell (that is the one path from data to arbitrary code the exec lock forbids).",
                file.display()
            );
        }
    }
}
