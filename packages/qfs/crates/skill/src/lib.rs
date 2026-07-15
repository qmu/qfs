//! `qfs-skill` — the embedded agent skill (ticket t39, blueprint §1/§11).
//!
//! This crate ships the authored **`SKILL.md`** operating procedure — the single uniform loop
//! **DESCRIBE → write a qfs statement → PREVIEW → COMMIT** an AI agent follows to drive every
//! service through `qfs` — and the **golden example corpus** (one worked example per driver),
//! both **embedded at compile time via [`include_str!`]** so they ship *inside* the single `qfs`
//! binary (blueprint §11). No `include_dir` crate is pulled (the offline cargo cache lacks it and the
//! disk is tight): each asset is one `include_str!`, listed in [`EXAMPLES`].
//!
//! ## Pure assets — no runtime deps
//! The shipped surface is `&'static str` text and a manifest. The crate has **zero** runtime
//! dependencies; the golden CORPUS (which proves each example parses → evaluates to a `Plan` →
//! its PREVIEW matches a checked-in golden) lives under `tests/` and reuses the **t38 `qfs-test`
//! harness** (`assert_plan` / `golden`) as a dev-dependency — never linked into the binary.
//!
//! ## The thesis: loop uniformity
//! Every example in [`EXAMPLES`] uses the **identical four steps**. That uniformity IS the
//! deliverable: an agent learns one loop, not N SDKs. If an example needed a prose exception, the
//! driver contract (t13) would be under-declaring — the fix belongs there, not here.

#![forbid(unsafe_code)]

/// The authored agent operating procedure, embedded at compile time. An embedding host (the
/// binary, or a skill loader) ships this verbatim so the loop docs travel inside the binary.
pub const SKILL_MD: &str = include_str!("../assets/SKILL.md");

/// One embedded worked example: the driver label + its canonical qfs statement asset. The
/// `source` is the exact statement the golden corpus parses → previews; the `driver` is the
/// service it exercises. All seven share the identical DESCRIBE → statement → PREVIEW → COMMIT
/// structure (the uniformity thesis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Example {
    /// The driver/service this example exercises (e.g. `"mail"`, `"server"`).
    pub driver: &'static str,
    /// The embedded example asset (a `.qfs` file with a comment header + the statement).
    pub source: &'static str,
}

/// The golden example corpus, embedded at compile time (blueprint §11) — one worked example per driver,
/// every one using the identical four-step loop. Ships inside the binary alongside [`SKILL_MD`].
///
/// `sql` is a pure read (no COMMIT); the rest produce an effect plan whose PREVIEW the golden
/// corpus pins. The `server` example is a `CREATE TRIGGER` that desugars to a single
/// `/server/triggers` config write (the PREVIEW-as-CI-test pattern, blueprint §10).
/// The shipped **declared-driver** script for Cloudflare's plain-REST surface, mounted at
/// `/cloudflare` (blueprint §13). Unlike the single-statement golden [`EXAMPLES`] corpus, this is a
/// multi-statement `CREATE DRIVER`/`TYPE`/`VIEW`/`MAP` program an operator installs by previewing +
/// committing each statement (they desugar to `/sys/drivers` rows — a local, audited write, zero
/// network). It coexists with the COMPILED `/cf` driver (D1 SQL, KV, Queues); `/cloudflare` is the
/// broad, user-extensible REST slice. Kept OUT of [`EXAMPLES`] because it is not a single-statement
/// PREVIEW golden — its per-statement parseability + host confinement are ratcheted by the
/// `qfs` crate's `declared_driver` tests instead. Credential-free by construction (no clause carries
/// a token; the value lives in the account layer).
pub const CLOUDFLARE_DRIVER: &str = include_str!("../assets/examples/cloudflare.qfs");

/// The shipped **declared-driver** script for Chatwork's API-key REST surface, mounted at
/// `/chatwork` (blueprint §13). Like [`CLOUDFLARE_DRIVER`] it is a multi-statement
/// `CREATE DRIVER`/`TYPE`/`VIEW`/`MAP` program an operator installs by previewing + committing each
/// statement (they desugar to `/sys/drivers` rows — a local, audited write, zero network). Chatwork
/// authenticates with an API key on an `x-chatworktoken` header (`AUTH HEADER` carries only the
/// header NAME). It covers rooms/messages/files listing + message post fully in the declaration;
/// file download (a cross-host `download_url` follow) and upload (multipart) are recorded in-asset as
/// the two generic evaluator primitives the declared model still lacks. Kept OUT of [`EXAMPLES`] like
/// the Cloudflare asset — a multi-statement install program, not a single-statement PREVIEW golden.
/// Credential-free by construction (no clause carries a token; the value lives in the account layer).
pub const CHATWORK_DRIVER: &str = include_str!("../assets/examples/chatwork.qfs");

/// The shipped **declared-driver** script for a read-only GitHub slice, mounted at `/ghdecl`
/// (blueprint §13, ticket 20260711121534). It is the first declared driver whose auth is an ACCOUNT
/// REFERENCE (`AUTH ACCOUNT 'github'`) rather than a per-driver API key: it reuses the EXISTING
/// `github` account provider's stored token, resolved from the vault at wire time, so an OAuth-style
/// service is expressible in the declared model with a credential-free declaration. Coexists with the
/// compiled `/github` driver (it mounts at the distinct `/ghdecl`). Like [`CLOUDFLARE_DRIVER`] it is a
/// multi-statement install program, kept OUT of [`EXAMPLES`]; its per-statement parseability + the
/// credential-free account descriptor are ratcheted by the `qfs` crate's `declared_driver` tests.
pub const GITHUB_ACCOUNT_DRIVER: &str = include_str!("../assets/examples/github_account.qfs");

pub const EXAMPLES: &[Example] = &[
    Example {
        driver: "mail",
        source: include_str!("../assets/examples/mail.qfs"),
    },
    Example {
        driver: "drive",
        source: include_str!("../assets/examples/drive.qfs"),
    },
    Example {
        driver: "github",
        source: include_str!("../assets/examples/github.qfs"),
    },
    Example {
        driver: "slack",
        source: include_str!("../assets/examples/slack.qfs"),
    },
    Example {
        driver: "sql",
        source: include_str!("../assets/examples/sql.qfs"),
    },
    Example {
        driver: "git",
        source: include_str!("../assets/examples/git.qfs"),
    },
    Example {
        driver: "server",
        source: include_str!("../assets/examples/server.qfs"),
    },
];

impl Example {
    /// The statement line of the example asset — the last non-empty, non-comment line. The asset
    /// header is `--` comment lines documenting the DESCRIBE excerpt + the step; the final line is
    /// the executable qfs statement the golden corpus parses.
    #[must_use]
    pub fn statement(&self) -> &str {
        self.source
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.starts_with("--"))
            .unwrap_or("")
    }
}

/// Render the embedded skill for `qfs skill`: [`SKILL_MD`] alone, or — when `include_examples` —
/// `SKILL_MD` followed by the [`EXAMPLES`] corpus (each driver's canonical `.qfs` asset under a
/// stable `## Examples` heading). Pure string assembly over the `include_str!` consts — no I/O, no
/// allocation beyond the joined output — so the binary's `qfs skill` arm stays logic-free.
#[must_use]
pub fn render(include_examples: bool) -> String {
    if !include_examples {
        return SKILL_MD.to_string();
    }
    let mut out = String::with_capacity(SKILL_MD.len() + 2048);
    out.push_str(SKILL_MD);
    out.push_str("\n\n## Example corpus (embedded golden examples)\n");
    for ex in EXAMPLES {
        out.push_str(&format!("\n### {}\n```text\n", ex.driver));
        out.push_str(ex.source.trim_end());
        out.push_str("\n```\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_md_documents_the_four_step_loop() {
        // The load-bearing structure an agent reads: the four steps and the archetype vocabulary.
        for needle in [
            "DESCRIBE",
            "PREVIEW",
            "COMMIT",
            "append_log",
            "relational_table",
            "object_graph_workflow",
            "blob_namespace",
            "irreversible",
            "least privilege",
            "UPSERT",
        ] {
            assert!(
                SKILL_MD.contains(needle),
                "SKILL.md is missing the load-bearing element `{needle}`"
            );
        }
    }

    #[test]
    fn corpus_covers_every_required_driver() {
        let drivers: Vec<&str> = EXAMPLES.iter().map(|e| e.driver).collect();
        for required in ["mail", "drive", "github", "slack", "sql", "git", "server"] {
            assert!(
                drivers.contains(&required),
                "the worked-example corpus is missing `{required}`"
            );
        }
        assert_eq!(
            EXAMPLES.len(),
            7,
            "exactly the seven required worked examples"
        );
    }

    #[test]
    fn every_example_extracts_a_statement() {
        for ex in EXAMPLES {
            let stmt = ex.statement();
            assert!(
                !stmt.is_empty(),
                "example `{}` has no executable statement line",
                ex.driver
            );
            // No example smuggles a credential shape into the shipped asset (secrets never appear).
            assert!(!stmt.to_lowercase().contains("token"));
            assert!(!stmt.contains("Bearer "));
        }
    }

    #[test]
    fn cloudflare_declared_driver_is_shipped_confined_and_credential_free() {
        // The declared /cloudflare script ships as a standalone asset (not a golden EXAMPLES
        // single-statement). Its behaviour (per-statement parse, loader, describe, two-source) is
        // ratcheted by the qfs crate; here we hold the shipped-asset invariants a reader relies on.
        assert!(CLOUDFLARE_DRIVER.contains("CREATE DRIVER cloudflare"));
        assert!(
            CLOUDFLARE_DRIVER.contains("AUTH BEARER"),
            "declares the bearer scheme (the value lives in the account layer)"
        );
        // Credential-free by construction: no token value, no inline bearer secret.
        assert!(!CLOUDFLARE_DRIVER.contains("Bearer "));
        assert!(!CLOUDFLARE_DRIVER.to_lowercase().contains("sk-"));
        // Host confinement, textual floor: every wire body addresses ONLY /http/cloudflare/… . A
        // foreign /http/<other> body would be dropped at load, so it must never ship.
        for (i, line) in CLOUDFLARE_DRIVER.lines().enumerate() {
            if let Some(pos) = line.find("/http/") {
                let after = &line[pos + "/http/".len()..];
                assert!(
                    after.starts_with("cloudflare/") || after.starts_with("cloudflare "),
                    "line {} addresses a non-cloudflare wire host: {line}",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn chatwork_declared_driver_is_shipped_confined_and_credential_free() {
        // The declared /chatwork script ships as a standalone asset (not a golden EXAMPLES
        // single-statement). Per-statement parse/install is ratcheted by the qfs crate; here we hold
        // the shipped-asset invariants a reader relies on.
        assert!(CHATWORK_DRIVER.contains("CREATE DRIVER chatwork"));
        assert!(
            CHATWORK_DRIVER.contains("AUTH HEADER 'x-chatworktoken'"),
            "declares the API-key header NAME (the value lives in the account layer)"
        );
        // Credential-free by construction: the header carries only a NAME, never a token value, and
        // no inline bearer secret ships.
        assert!(!CHATWORK_DRIVER.contains("Bearer "));
        assert!(!CHATWORK_DRIVER.to_lowercase().contains("sk-"));
        // Host confinement, textual floor: every /http/ wire reference addresses ONLY the chatwork
        // host (a foreign /http/<other> body would be dropped at load, so it must never ship).
        for (i, line) in CHATWORK_DRIVER.lines().enumerate() {
            // The recorded-findings comment block illustrates the missing primitives with `--`
            // comment lines; only executable (non-comment) lines are held to the confinement floor.
            if line.trim_start().starts_with("--") {
                continue;
            }
            if let Some(pos) = line.find("/http/") {
                let after = &line[pos + "/http/".len()..];
                assert!(
                    after.starts_with("chatwork/") || after.starts_with("chatwork "),
                    "line {} addresses a non-chatwork wire host: {line}",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn render_emits_the_loop_and_optionally_the_corpus() {
        // The plain render is exactly SKILL_MD (what `qfs skill` prints).
        let plain = render(false);
        assert_eq!(plain, SKILL_MD);
        for landmark in ["DESCRIBE", "PREVIEW", "COMMIT"] {
            assert!(plain.contains(landmark), "render(false) lost `{landmark}`");
        }
        // `--examples` appends the corpus under a stable heading, with every driver's statement.
        let full = render(true);
        assert!(
            full.starts_with(SKILL_MD),
            "render(true) keeps SKILL.md first"
        );
        assert!(full.contains("## Example corpus"));
        for ex in EXAMPLES {
            assert!(
                full.contains(ex.statement()),
                "render(true) is missing the `{}` example statement",
                ex.driver
            );
        }
    }
}
