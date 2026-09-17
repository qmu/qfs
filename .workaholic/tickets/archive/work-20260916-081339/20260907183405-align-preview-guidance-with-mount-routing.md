---
created_at: 2026-09-07T18:34:05+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
merge_policy: review
claim: work-20260916-081339
---

# Align preview guidance with the requirement for a routed mount

## Overview

Correct QFS's human and agent guidance so following a documented preview example produces the stated result under its stated prerequisites. Distinguish offline schema description from evaluating a write against an installed mount, and distinguish absence of network effects from absence of setup requirements.

At repository HEAD `277504f`, the installed `qfs 0.0.129` reported the same commit. On 2026-09-07, `qfs describe /mail/drafts --json` returned the compiled schema, but the following command, with no commit flag, returned `unrouted_path` rather than a preview:

```sh
qfs run "insert into /mail/drafts values ('alice@example.com','Hi','Body')"
```

```json
{"error":{"code":"unrouted_path","kind":"capability","message":"path `/mail/drafts` routes to no mounted driver, so no schema can be described for it"}}
```

`docs/cookbook/faq.md` currently promises that this works even for a cloud path the user has not connected and shows a successful preview. Its generated `qfs-faq` skill repeats that claim. The base plugin skill and binary-embedded skill contain related broad credential/offline claims and examples that need to be checked, not assumed accurate because generation is synchronized.

Historical evidence indicates that rejecting unrouted writes is intentional: the August 16 fix removed a literal-path fallback that misleadingly previewed impossible writes. Preserve that refusal. The audit used the current machine state, not a freshly isolated configuration, so the first implementation step must reproduce and localize the precise preconditions.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — retain existing Cookbook, embedded skill, and CLI test locations.
- `workaholic:implementation` / `policies/coding-standards.md` — use repository Rust conventions and typed outcomes for any test changes.
- `workaholic:implementation` / `policies/objective-documentation.md` — document measured behavior, prerequisites, and concrete expected outcomes.
- `workaholic:implementation` / `policies/test.md` — regression verification must exercise evaluation and routing, not only parse CLI arguments.
- `workaholic:implementation` / `policies/command-scripts.md` — reuse the existing local generation and checking commands.

## Key Files

- `docs/cookbook/faq.md` — authored claim that unconnected cloud writes preview successfully.
- `plugins/qfs/skills/qfs-faq/SKILL.md` — generated FAQ, updated through gen-skills only.
- `plugins/qfs/skills/qfs/SKILL.md` — separately authored base operating instructions.
- `packages/qfs/crates/skill/assets/SKILL.md` — operating instructions embedded in `qfs skill`.
- `packages/qfs/crates/core/src/eval.rs` — write routing and UnroutedPath refusal.
- `packages/qfs/crates/qfs/src/describe.rs` — compiled credential-free describe registry, distinct from execution mounts.
- `packages/qfs/crates/cmd/tests/e2e_cli.rs` — real CLI behavior and isolated test fixture patterns.
- `packages/qfs/crates/cmd/tests/faq_cli_surface.rs` — existing FAQ clap coverage, not preview execution verification.
- `packages/qfs/xtask/tests/cookbook_skills.rs` — recipe grammar/column validation with known execution limits.
- `plugins/qfs/.claude-plugin/plugin.json`, `plugins/qfs/.codex-plugin/plugin.json`, `.claude-plugin/marketplace.json` — four synchronized plugin version fields.

## Related History

The earlier fix explicitly removed successful plans for unrouted writes; the instructions still teach that removed behavior. Existing Cookbook tests validate grammar and selected query columns, not this runtime precondition.

- `.workaholic/tickets/archive/work-20260816-213314/20260816213014-a-write-with-no-declared-map-previews-as-if-it-would-work.md` — read its Final Report, which corrects the initial diagnosis and explains removal of eval_write's unrouted fallback.
- `.workaholic/tickets/archive/work-20260816-184034/20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md` — actual scope and skipped paths of semantic recipe checks.
- `.workaholic/tickets/archive/work-20260816-211551/20260813024753-slack-file-upload-and-download-are-documented-but-unimplemented.md` — precedent for correcting source instructions, regenerating skills, and synchronizing plugin versions.

## Implementation Steps

1. Reproduce the describe success and write-preview refusal using a freshly built binary and an isolated configuration/project store, with no service credentials and no commit flag. Record version, setup, exit statuses, and structured output. Trace the difference between describe's compiled registry and the runtime mount registry in eval_write; read the historical Final Report before choosing a fix.
2. Establish and test an explicit behavior matrix: compiled describe for an unconnected supported path; write preview to an unrouted path; and a documented valid preview using a hermetic routed fixture. Determine exactly which setup a real-service example requires. Keep the intentional unrouted refusal rather than reintroducing a permissive fallback.
3. Correct the FAQ source, base plugin instructions, embedded binary skill, and directly related guide claims found by a focused search. Show either an actually runnable credential-free example with all setup, or the required mount setup and expected refusal/recovery. Qualify general claims about credentials and qfs run so they do not imply live reads are credential-free or all statements merely preview.
4. Regenerate Cookbook-derived skills. Add or extend a meaningful CLI regression assertion for the stated setup and expected refusal/success, using hermetic fixtures and verifying no committed effect; reuse existing routing tests where they already prove the requirement. Do not treat the existing clap test as runtime verification.
5. Bump the four plugin version fields together according to CLAUDE.md, apply the binary patch rule for the implementation PR, and run the quality gate. Do not hand-edit generated references; regenerate only if their source changes.

## Quality Gate

**Acceptance criteria**

- A user following the FAQ understands why describe can succeed while a write refuses, and can reach the documented preview result by following the explicitly stated setup.
- Unrouted writes still fail before commit; no documentation repair weakens routing or the irreversible-operation gate.
- Human FAQ, generated FAQ skill, base skill, and embedded qfs skill make consistent claims about the verified preview prerequisites.
- Each example presented as executable is checked in its documented environment; any deliberate schematic example is identified as such.
- Updated generated content and all four plugin versions remain in sync.

**Verification method**

- Decided: reproduce with isolated stores and no service credentials, verify a routed fixture and an unrouted negative case, and make no live-service writes — the defect concerns plan-time routing and instruction accuracy (developer may override at /drive).
- Record the behavior matrix's raw output and exit statuses, and run the relevant CLI integration tests.
- From `packages/qfs`, run `cargo test --workspace`, `env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check`, and `cargo run -p xtask -- gen-skills --check`. Run `npm run docs:build` at repository root.

**Gate**

- Documented examples agree with measured outcomes, hermetic routing checks pass, and the applicable Rust/generation/docs checks are green before the implementation is reported complete.

## Considerations

- Missing Cargo in the audit environment is a verification limitation, not a permanent handoff requirement. Obtain the normal development toolchain when implementing; do not mark checks as passed based on source inspection.
- The hypothesis is stale documentation following an intentional evaluator change; reproduction decides the exact correction. Changing registration requirements solely to make an old example pass is not the proposed fix.
- The companion `20260907183404-verify-codex-plugin-distribution-and-drift.md` addresses distribution checks and Codex onboarding. There is no implementation prerequisite between the tickets; rebase shared version edits when shipping independently.
- Keep this ticket standalone. `review` is the default merge policy and does not authorize merging the ticket publication PR.

## Final Report

Reconciled the duplicate queued ticket against the completed archive in
`archive/work-20260907-205639/` and merged PR #106. The FAQ, base skill, generated
FAQ skill, and unrouted CLI regression were already implemented on the base.
This follow-up corrects the remaining embedded Quick reference sentence: reads
execute immediately, while write previews require an installed route. No evaluator
change or duplicate regression test was needed. The original archive is preserved.

The binary patch is 0.0.132; the four synchronized plugin version fields are 0.22.4.
No service write, release tag, or deployment was performed.

### Isolated CLI behavior matrix

A freshly built binary ran in a fresh directory with a fresh XDG_CONFIG_HOME and
no service credentials, using neither --commit nor --commit-irreversible. System
and project databases were isolated under that config directory. Raw results follow.
The reads before and after the policy preview are byte-identical with no rows.

Command arguments: ["--version"]

Exit: 0

stdout:
```text
qfs 0.0.132
commit:  c7fa350
target:  aarch64-unknown-linux-gnu
wasm32:  false
```

stderr:
```text
(empty)
```

Command arguments: ["describe", "/mail/drafts", "--json"]

Exit: 0

stdout:
```text
{"path":"/mail/drafts","archetype":"append_log","native_verbs":"SELECT(tail) INSERT(append) UPSERT","columns":[{"name":"id","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"thread_id","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"date","ty":"Timestamp","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"from","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"subject","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"snippet","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"label_ids","ty":{"Array":"Text"},"nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"attachments","ty":{"Array":{"Struct":{"columns":[{"name":"filename","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"mime","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"size","ty":"Int","nullable":false,"provenance":{"driver":null,"source_col":null}}]}}},"nullable":false,"provenance":{"driver":null,"source_col":null}}],"verbs":{"select":true,"insert":true,"upsert":true,"update":false,"remove":false,"ls":false,"cp":false,"mv":false,"rm":false},"procedures":[{"name":"send","params":[{"name":"to","ty":"Text"},{"name":"subject","ty":"Text"},{"name":"body","ty":"Text"},{"name":"attachments","ty":{"Array":{"Struct":{"columns":[{"name":"filename","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"mime","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"bytes","ty":"Bytes","nullable":false,"provenance":{"driver":null,"source_col":null}}]}}}}],"irreversible":true,"returns":null,"requires_scopes":["https://www.googleapis.com/auth/gmail.compose"]},{"name":"reply","params":[{"name":"body","ty":"Text"},{"name":"to","ty":"Text"},{"name":"cc","ty":"Text"},{"name":"subject","ty":"Text"},{"name":"attachments","ty":{"Array":{"Struct":{"columns":[{"name":"filename","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"mime","ty":"Text","nullable":false,"provenance":{"driver":null,"source_col":null}},{"name":"bytes","ty":"Bytes","nullable":false,"provenance":{"driver":null,"source_col":null}}]}}}}],"irreversible":false,"returns":null,"requires_scopes":["https://www.googleapis.com/auth/gmail.compose"]}],"aliases":[{"name":"SEND","desugars_to":"mail.send"}],"pushdown":{"where_":true,"project":false,"limit":true,"order":false,"join":false,"aggregate":false,"distinct":false,"group_by":false},"child_address":{"kind":"none"},"schema_contract":{"kind":"compiled"},"children":[]}
```

stderr:
```text
(empty)
```

Command arguments: ["run", "insert into /mail/drafts values ('alice@example.com','Hi','Body')", "--json"]

Exit: 3

stdout:
```text
(empty)
```

stderr:
```text
{"error":{"code":"unrouted_path","kind":"capability","message":"path `/mail/drafts` routes to no mounted driver, so no schema can be described for it"}}
```

Command arguments: ["run", "/sys/policies |> select name", "--json"]

Exit: 0

stdout:
```text
{"schema":[{"name":"name","type":"text"}],"rows":[],"meta":{"row_count":0,"truncated":false,"limit":null,"offset":null,"affected":null}}
```

stderr:
```text
(empty)
```

Command arguments: ["run", "insert into /sys/policies values (name, allow) ('preview-example', 'ALLOW INSERT')", "--json"]

Exit: 0

stdout:
```text
{"preview":{"rows":[{"id":0,"verb":"INSERT","target":{"driver":"sys","path":"/sys/policies"},"affected":{"exact":1},"irreversible":false}],"irreversible":[],"total_affected":{"exact":1},"is_pure":false},"committed":false}
```

stderr:
```text
(empty)
```

Command arguments: ["run", "/sys/policies |> select name", "--json"]

Exit: 0

stdout:
```text
{"schema":[{"name":"name","type":"text"}],"rows":[],"meta":{"row_count":0,"truncated":false,"limit":null,"offset":null,"affected":null}}
```

stderr:
```text
(empty)
```

### Verification

- Rust workspace: 2801 passed, zero failed.
- Serialized qfs library with XDG_CONFIG_HOME unset: 529 passed, one existing ignored test, zero failed.
- Clippy with warnings denied, rustfmt, gen-docs --check, and gen-skills --check passed.
- Plugin distribution check and all 11 negative fixtures passed.
- VitePress docs build passed, with its existing large-chunk warning.
- The full suites used a private user/mount namespace binding a repository-local temporary directory to /tmp; no host mount or system setting changed.
- A read-only story reviewer found no blocking issue in the final wording, historical reconciliation, or version alignment.
