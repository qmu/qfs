//! Dev-only dependency-graph guard (t38 acceptance): the shipped `cfs` binary must **not**
//! link `cfs-test`. The harness is a dev-dependency-only support crate; if it ever leaked into
//! the binary's normal (non-dev) dependency closure, dead test-support code (and its scrub /
//! golden machinery) would ship. This test shells out to `cargo metadata` (via the `CARGO`
//! env var) and walks the `cfs` package's **normal + build** dependency edges, asserting
//! `cfs-test` never appears. It mirrors the established `crates/plan/tests/purity_deps.rs`
//! cargo-metadata style.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

#[test]
fn cfs_binary_does_not_link_cfs_test() {
    let json = cargo_metadata();

    // package id -> name.
    let id_to_name: BTreeMap<String, String> = json["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|p| {
            (
                p["id"].as_str().expect("id").to_string(),
                p["name"].as_str().expect("name").to_string(),
            )
        })
        .collect();

    // The resolve graph, with per-edge dep_kinds so we can EXCLUDE dev-dependencies (a dev-dep
    // edge to cfs-test is exactly what we DO allow; a normal/build edge is the violation).
    let nodes = json["resolve"]["nodes"].as_array().expect("resolve.nodes");
    let node_by_id: BTreeMap<&str, &serde_json::Value> = nodes
        .iter()
        .map(|n| (n["id"].as_str().expect("node id"), n))
        .collect();

    let cfs_id = id_to_name
        .iter()
        .find(|(_, name)| name.as_str() == "cfs")
        .map(|(id, _)| id.clone())
        .expect("cfs package present");

    // BFS the resolve graph from `cfs`, following ONLY normal + build edges (skip dev-dep
    // edges). If cfs-test is reachable along that path, it would be linked into the binary.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![cfs_id];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = node_by_id.get(id.as_str()) else {
            continue;
        };
        for dep in node["deps"].as_array().into_iter().flatten() {
            // `dep_kinds` is an array of {kind, target}. kind == null is a normal dep; "build"
            // is a build-script dep; "dev" is a dev-dependency (NOT linked into the binary).
            let is_runtime_edge = dep["dep_kinds"].as_array().into_iter().flatten().any(|k| {
                let kind = k["kind"].as_str();
                kind.is_none() || kind == Some("build")
            });
            if !is_runtime_edge {
                continue;
            }
            if let Some(pkg) = dep["pkg"].as_str() {
                stack.push(pkg.to_string());
            }
        }
    }

    // Assert cfs-test is NOT in the binary's runtime closure.
    let leaked = seen
        .iter()
        .filter_map(|id| id_to_name.get(id))
        .any(|name| name == "cfs-test");
    assert!(
        !leaked,
        "dev-only violation: `cfs-test` is in the `cfs` binary's normal/build dependency \
         closure — the harness must be a dev-dependency only (never linked into the shipped binary)"
    );

    // Sanity: cfs-test IS a workspace member (so the test is exercising a real crate, not a typo).
    assert!(
        id_to_name.values().any(|n| n == "cfs-test"),
        "cfs-test should be a workspace member"
    );
}

fn cargo_metadata() -> serde_json::Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .output()
        .expect("failed to run `cargo metadata`");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata produced invalid JSON")
}
