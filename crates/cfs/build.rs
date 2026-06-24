//! Build script for the `cfs` binary (ticket t40): emits **reproducibility/observability**
//! metadata into the compiled artifact so `cfs --version` is the field-debug anchor (RFD §9).
//!
//! It sets three `rustc-env` values the [`cfs::version`] module reads via `env!`:
//! - `CFS_GIT_SHA`     — the short git commit the binary was built from (`unknown` off-git).
//! - `CFS_TARGET`      — the target triple (`TARGET`, set by cargo for the build).
//! - `CFS_WASM_CAPABLE`— `"true"` when the target is a `wasm32-*` triple, else `"false"`.
//!
//! It embeds **no secrets** (RFD §10) — only a commit hash, a target triple, and a derived
//! flag. The git lookup is best-effort and never fails the build: a source tarball with no
//! `.git` still builds, emitting `CFS_GIT_SHA=unknown`. Re-run only when git HEAD moves.

use std::process::Command;

fn main() {
    // Target triple cargo is building for (host triple for a native build).
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=CFS_TARGET={target}");

    // wasm-capable flag: true iff building for a wasm32 target (RFD §9 Workers target).
    let wasm_capable = target.starts_with("wasm32");
    println!("cargo:rustc-env=CFS_WASM_CAPABLE={wasm_capable}");

    // Best-effort short git sha; `unknown` if git is unavailable or this is not a checkout.
    let git_sha = git_short_sha().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=CFS_GIT_SHA={git_sha}");

    // Rebuild the version metadata when HEAD moves (so the sha stays honest) without forcing
    // a rebuild on every unrelated source edit.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
}

/// Resolve the short git sha, returning `None` (so the caller defaults to `unknown`) if git
/// is missing, the command fails, or output is empty. Never panics; never fails the build.
fn git_short_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}
