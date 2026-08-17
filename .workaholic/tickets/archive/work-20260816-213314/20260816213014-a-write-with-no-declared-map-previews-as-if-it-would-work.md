---
created_at: 2026-08-16T21:30:14+00:00
status: done
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260816-213314
---

# A write to a declared driver with no matching `CREATE MAP` previews as if it would work

## Overview

PREVIEW is the safety instrument: an operator (or an agent) reads the preview to learn what a
statement would do before committing it. For a declared driver it answers for a write the driver
**cannot perform at all** — one whose path and verb match no `CREATE MAP` — with an ordinary effect
row, indistinguishable from a write that would succeed.

Measured 2026-08-16 against `qfs 0.0.107` with the shipped `slack_driver.qfs` installed (17 rows),
which declares no `MAP UPSERT` for any `/slack/…` path:

```
$ qfs run "/local/tmp/t.csv |> decode csv |> select 'a.txt' as filename, a as bytes \
    |> upsert into /slack/acme/files"
{"preview":{"rows":[{"id":0,"verb":"READ","target":{"driver":"local","path":"/local/tmp/t.csv"},…},
                    {"id":1,"verb":"UPSERT","target":{"driver":"slack","path":"/slack/acme/files"},
                     "affected":"unknown","irreversible":false}],…},"committed":false}
```

Contrast a **read** of an unrouted path, which refuses at plan time with `unknown_source` / exit 3.
The write side is the asymmetry: nothing between the parser and the applier asks whether a map
exists, so the refusal — if one comes at all — arrives at commit, after the operator has read a
preview that said the write is fine.

This is how `docs/cookbook/slack.md` could teach an `UPSERT INTO /slack/<ws>/files` for months after
the compiled driver that implemented it was deleted (ticket `20260813024753`): every hand-check of
the recipe previewed cleanly. The cookbook ratchet did not catch it either, and
`20260725143000` (typecheck the ratchet) has since raised that half — but the ratchet checks
recipes in the repository, while this is what an operator's own statement does at the terminal.

## Scope

**In scope:** make a declared-driver write whose (path, verb) matches no installed `CREATE MAP`
refuse at **plan time**, with a structured code naming the path and the verb, the way an unrouted
read already refuses.

**Out of scope:**

- Compiled drivers, which resolve their write capability through `Capabilities` at resolve time —
  the gap is specific to the declared path.
- The preview's `affected: unknown` for declared writes; estimating row counts is a separate
  question and not what this ticket is about.
- `describe` on a declared mount, which is its own defect (`20260728085253`).

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — the preview must refuse what the driver
  cannot do rather than render a plausible row for it.
- `workaholic:implementation` / `policies/coding-standards.md`.
- `workaholic:operation` / `observability` — a preview that cannot distinguish "will work" from
  "cannot work" is an instrument reporting something other than the system's state.

## Key Files

- `packages/qfs/crates/exec/src/declared.rs` — `eval_map_body` and the declared write seam; the map
  lookup that would have to run (or be mirrored) at plan time.
- `packages/qfs/crates/core/src/eval.rs` — where a plan-time write refusal is raised
  (`EvalError::DriverWrite` / `UnroutedPath` are the neighbouring shapes and codes).
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — the declaration whose missing
  `MAP UPSERT` this was measured against.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — where a test would sit, beside the existing
  declared-write tests.

## Implementation Steps

1. Reproduce with the command above and record the raw preview, then the raw commit outcome — the
   commit-time behaviour decides whether this is "late refusal" or "no refusal at all", and the
   ticket's wording must match what the binary does.
2. Decide where the check belongs: the evaluator (needs the installed maps at plan time) or the
   declared read/write seam that already loads them.
3. Refuse with a structured, AI-consumable error naming the path and the verb, and listing the verbs
   the mount does declare — the recovery information an agent needs.
4. Test it beside the declared-driver tests: an unmapped `UPSERT` refuses at plan time; a mapped one
   (the post map, the file detach) still previews and commits unchanged.

## Quality Gate

**Acceptance criteria**

- An `UPSERT INTO /slack/<ws>/files` against the shipped declaration refuses at plan time with a
  structured code and a non-zero exit, naming the verbs `/slack/<ws>/files` does declare.
- A mapped declared write (`INSERT INTO /slack/<ws>/<channel>/messages`,
  `REMOVE /slack/<ws>/files/<id>`) previews and commits exactly as before.
- The refusal reaches the operator as prose, like every other plan-time refusal.

**Verification method**

- The command in the Overview, re-run against the built binary with its raw output and `EXIT=` code
  pasted into the ticket outcome.
- The new tests, plus `cargo test --workspace`.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-16 while driving `20260813024753`, whose
  step 1 asked for the gap to be confirmed "from the binary rather than from the source". Confirming
  it is what surfaced this: the probe that was supposed to *fail* for a retired upload previewed
  green.
- Worth checking whether the same asymmetry exists for a declared `CALL` naming no map.

## Final Report

Development completed as planned, with the ticket's own premise corrected by its step 1 measurement
(the correction was made on this branch's earlier pass and is unchanged: the defect is not
`CREATE MAP`-specific — it is that an effect target which routes to **no mount at all** built a
plan against its literal path).

**What ships.** `Evaluator::eval_write` (`crates/core/src/eval.rs`) no longer falls back to the
literal path when `mounts.resolve_path` misses; it returns `EvalError::UnroutedPath`, the same
refusal the read source beside it has always raised. The retired fallback's comment said it existed
so "the verb/irreversible semantics are testable without a mount" — the tests seed a mount instead,
which is what production always has.

**The verification the ticket asks for**, against the built binary (`qfs 0.0.108`) with nothing
CONNECTed — the Overview's own command:

```
$ qfs run "/local/tmp/t.csv |> decode csv |> select 'a.txt' as filename, a as bytes \
    |> upsert into /slack/acme/files" --json
{"error":{"code":"unrouted_path","kind":"capability","message":"path `/slack/acme/files` routes to no mounted driver, so no schema can be described for it"}}
EXIT=3

$ qfs run "/slack/acme/files |> limit 1" --json          # the read side, for contrast
{"error":{"code":"unknown_source","kind":"capability","message":"unknown source `slack`"}}
EXIT=3

$ qfs run "insert into /nosuchdriver/x values ('a')" --json
{"error":{"code":"unrouted_path","kind":"capability","message":"path `/nosuchdriver/x` routes to no mounted driver, so no schema can be described for it"}}
EXIT=3
```

Before this change the first and third previewed `affected: {exact: 1}` at exit 0.

**Quality gate, item by item:**

| Ticket gate | Status |
| --- | --- |
| `UPSERT INTO /slack/<ws>/files` against the shipped declaration refuses at plan time, structured code, non-zero exit | ✅ both routings: unrouted (`unrouted_path`, exit 3, above) and routed (`unsupported_verb`) |
| …naming the verbs `/slack/<ws>/files` does declare | ✅ `supported: [SELECT]` — but see Concerns: that list is the leading segment's aggregate, not the node's, and reaching even that needed the `{param}` repair below |
| A mapped declared write (`INSERT INTO /slack/<ws>/<channel>/messages`, `REMOVE /slack/<ws>/files/<id>`) previews and commits unchanged | ✅ `an_unperformable_declared_write_refuses_at_plan_time`'s control — and it did **not** hold before this change (below) |
| The refusal reaches the operator as prose | ✅ the `Display` arm renders it; the CLI prints the envelope above |
| `cargo test --workspace` green | ✅ 2725 passed, 0 failed |
| clippy / fmt / gen-docs / gen-skills / check-migrations | ✅ all exit 0 |

**A second defect the measurement forced open.** Removing the fallback made the routed refusal
reachable for the first time, and it refused *everything*: `supported: []` for every `/slack/…`
path, mapped or not. `DeclaredDriver::resources` keys a resource by the segment after the driver
name, so the shipped slack declaration — whose every node is `/slack/{ws}/…` — assembles to one
resource literally keyed `{ws}`, which no concrete workspace id matches. So the documented
"post a message" map could not plan either. `RestApiConfig::resource_for_segment` now falls back to
a `{param}`-token resource when no literal segment matches, restoring the aggregation this layer
already intends (the cloudflare `accounts` case states it explicitly). The finer question — one
answer per declared node rather than per leading segment — is `20260817001110`.

### Discovered Insights

- **The read and write halves of `eval` consulted the same mount table and disagreed on a miss.**
  `fold_source` raised `UnroutedPath`; `eval_write` fell through to a literal-path plan. Both arms
  were deliberate and both were commented; what nobody had written down is that the pair makes
  PREVIEW answer "this write is fine" for a path whose read says "no such source". A divergence
  introduced for testability is still a product behaviour.
  **Context:** the fallback's own comment named its reason — "so the verb/irreversible semantics are
  testable without a mount". Fourteen tests depended on it, and every one of them was testing CLI
  plumbing (exit codes, output format, stdout/stderr separation) with an effect statement chosen
  only because it needed no mount. Seeding a mount was the cheaper half of this change; noticing
  that the convenience had become a contract was the expensive half.

- **A declared driver's capability gate is keyed by leading path segment, and a per-tenant service
  has a parameter there.** `/chatwork/rooms/…` keys on `rooms` and works; `/slack/{ws}/…` keys on
  `{ws}` and matches nothing. Reads never surfaced it because the read path does not consult
  capabilities — only writes do, and no write on a parameterised declared mount could reach the
  gate while the unrouted fallback was in front of it.
  **Context:** this is why "a routed declared mount already refuses an unmapped write at plan time"
  read as true from the source and was false in practice for slack. Two mechanisms in series, each
  individually defensible, hid each other.

- **`crates/cmd/tests/e2e_cli.rs`'s module header carried a stale premise for months.** It said
  effect plans build "against the one-shot mounts (incl. the cred-free Google describe mounts, so
  `/mail/drafts` PLANS)". Since the CONNECT model nothing third-party is pre-mounted; `/mail` routes
  nowhere on a fresh host, and those plans were the fallback, not a Google mount.
  **Context:** a comment asserting *why* a test passes is load-bearing documentation and drifts
  silently, because nothing checks it. The tests kept passing for a different reason than the one
  written above them.
