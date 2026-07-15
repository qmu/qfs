---
created_at: 2026-07-05T17:42:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
commit_hash: d5817f8
category: Added
depends_on: [20260705174100-tier2-view-body-evaluation.md]
---

# §13 tier-2 (3/3): MAP `VALUES (<expr>)` + the row-equivalence parity proof

## Overview

Finish the blueprint §13 **Tier 2** decision: the write-side row→wire-body mapping, then the
sharpened acceptance bar — the Slack script twin's reads must be **row-equivalent** to the
compiled driver's on the same fixtures (what tier 1 could only record as named gaps).

1. **MAP `VALUES (<expr>)`** (parity gap ⑤): `CREATE MAP INSERT <path> AS INSERT INTO <wire>
   VALUES (<expr>)` where `<expr>` is any expression over the bound row — `row` (passthrough,
   the tier-1 case) or a struct literal (`struct(channel: row.channel, text: row.text)`;
   struct literals are already expression constructors since the PR #11 hard break). At apply
   time, evaluate the expression per incoming row with the core expression evaluator
   (`crates/core/src/eval.rs`), encode the result with the driver codec, send through the
   confined applier. Purity holds: the mapping constructs the wire effect; only the applier
   performs I/O at COMMIT.
2. **slack.qfs goes tier-2** — the honest twin:
   - view: `CREATE VIEW /slack/history OF /type/slack/message AS
     /http/slack/conversations.history |> DECODE json |> EXPAND messages`
     (envelope unwrapped, mount path decoupled from the dotted method)
   - cursor: `PAGINATE CURSOR (next 'response_metadata.next_cursor' param 'cursor' MAX 50)`
     (the real nested Slack cursor)
   - map: `CREATE MAP INSERT /slack/post AS INSERT INTO /http/slack/chat.postMessage
     VALUES (struct(channel: row.channel, text: row.text))`
3. **The row-equivalence test**: run the compiled Slack driver and the declared twin over the
   SAME MockHttp fixture (a real-shaped `conversations.history` envelope, two pages via the
   nested cursor) and assert the delivered rows are EQUAL (same columns, same values, same
   order after a deterministic sort). The write side: both drivers' `chat.postMessage` produce
   the same wire body for the same input row. Delete the five recorded §13 parity parks that
   this closes (concern `21-13-conversion-parity-the-slack-script`); keep only what genuinely
   remains parked (none expected for Slack; GraphQL/websockets etc. stay §13-level parks).

## Key files

- `packages/qfs/crates/parser/src/grammar.rs` — map VALUES accepts an expression over `row`
- `packages/qfs/crates/qfs/src/declared_driver.rs` + `commit.rs` — apply-time expr evaluation
- `packages/qfs/crates/parser/tests/fixtures/slack.qfs` + `tests/slack_twin.rs` — tier-2 twin
- `packages/qfs/crates/qfs/src/declared_driver.rs` tests — the row-equivalence proof vs
  `crates/driver-slack`

## Resume knowledge (scouted 2026-07-05 — read ticket 2/3's Resume section first)

- Work continues on branch `work-20260705-173620` (blueprint Tier-2 decision + ticket 1/3
  already landed there; version already bumped to 0.0.23 — do NOT bump again this branch).
- The core expression evaluator for the `VALUES (<expr>)` mapping: `eval_value(expr, schema,
  row)` exists at `crates/engine/src/eval.rs:173` but is `pub(crate)` — the parser-level
  `Expr` → engine `ScalarExpr` lowering lives inside `qfs_core::plan_query`; check
  `crates/core/src/eval.rs` (a separate core-level expression evaluator over parsed `Expr`)
  first — it is likely the right seam for apply-time row→body mapping without touching the
  engine crate.
- The compiled Slack driver for the row-equivalence comparison: `crates/driver-slack` (find
  its fixture shape in its tests; reuse the SAME envelope JSON for both drivers).
- The current twin test to upgrade:
  `crates/qfs/src/declared_driver.rs::slack_twin_reads_hermetically_and_records_the_envelope_parity_gap`
  (asserts the gap — flip to row-equivalence once EXPAND + OF shaping land) and
  `crates/parser/tests/slack_twin.rs` + `crates/parser/tests/fixtures/slack.qfs`.
- The five parity parks to close are recorded on archived ticket
  `.workaholic/tickets/archive/work-20260705-032203/20260704145138-driver-conformance-and-first-conversion.md`
  and as active concern `.workaholic/concerns/21-13-conversion-parity-the-slack-script.md`.

## Resume knowledge — DEEP SCOUT (2026-07-05, after T2 landed; start here, do NOT re-scout)

Ticket 2/3 (view-body evaluation) is DONE and archived on branch `work-20260705-173620`
(commit `a86977c`): the READ side fully works via `qfs_exec::declared::eval_view_body` (a public
fn taking a fetch closure; the binary's `RestReadDriver` injects `rest_read_rows`). The blueprint
Tier-2 decision + T1 (dotted cursor + redirect confinement) also landed. Version already 0.0.23.

### The MAP expression form ALREADY PARSES — no grammar change

`{channel: row.channel, text: row.text}` parses as `Expr::Struct([("channel",
Expr::Path(["row","channel"]), …)])` (curly `{…}` is the struct constructor at `grammar.rs:481`;
`row.channel` is a dotted path `Expr::Path` at `ast.rs:448` / `grammar.rs:564`). The blueprint's
old `struct(…)` example was WRONG and is now fixed to `{…}`. So `CREATE MAP INSERT /slack/post AS
INSERT INTO /http/slack/chat.postMessage VALUES ({channel: row.channel, text: row.text})` parses
today — the map body is stored as `Statement::Effect(EffectBody::Values(Values{rows:[[Expr::Struct]]}))`.

### The apply side does NOT evaluate the map body yet (the real work of part 1)

`commit.rs` (~line 327, the `declared_mounts()` loop) registers `rest_apply_driver(&driver)` — the
STOCK applier, which POSTs the incoming row as-is; the stored MAP body is never consulted. To
implement `VALUES (<expr>)`: thread the declared maps into the apply path (mirror how
`RestReadDriver` threads views), and per incoming row evaluate the map body's single `Expr` with
`row` bound to the incoming row, producing the wire body the applier POSTs.
- **KEY SEAM TO VERIFY FIRST:** how the core expression evaluator resolves `Expr::Path(["row",
  "field"])`. Check `crates/core/src/eval.rs` (the parser-`Expr` evaluator) — is there struct
  navigation for a dotted path? Likely approach: bind the incoming row as a single `Struct` column
  named `row`, then `eval` the map `Expr` against schema `[row: Struct(incoming cols)]`, so
  `row.channel` navigates the struct. Then encode the resulting `Value::Struct` as JSON (the driver
  codec) → the POST body. The engine's `eval_value` (`engine/eval.rs:173`) is `pub(crate)`; the
  core-level evaluator over parsed `Expr` is the right seam (keep apply-eval in qfs-exec like the
  read eval, binary stays off the spine — see T2's dep-direction lesson).

### Row-equivalence: the compiled Slack read shape (so the OF type can MATCH it)

`qfs_driver_slack` message schema (`dto.rs` `MessageDto::schema`) is **5 columns**:
`(ts:Text non-null, user:Text null, text:Text null, thread_ts:Text null, subtype:Text null)`.
`From<&MessageDto> for Row` (`dto.rs:66`) null-handling: `user`/`subtype` empty→`Null`, `thread_ts`
via `ts_text` empty→`Null`, `ts` always `Text`. `decode_messages` (`read.rs:144`) accepts the
`{messages:[…]}` envelope OR a bare array. The compiled read entry is `qfs_driver_slack::read_rows`
(`read.rs:45`) over `MockSlackClient` (`lib.rs:89`).
- **Therefore** `slack.qfs`'s `CREATE TYPE /type/slack/message` must declare all 5 columns
  `(ts, user, text, thread_ts, subtype)` for the declared twin's `OF`-shape to match the compiled
  schema's column NAMES. A homogeneous fixture of `{ts,user,text}` messages → `thread_ts`/`subtype`
  absent → `Null` in BOTH drivers (compiled: empty→Null; declared `shape_to_type`: absent→Null) → MATCH.
- **Compare column NAMES + row VALUES only** (the "delivered rows are EQUAL" bar), NOT schema
  type/nullability: declared `shape_to_type` sets every col `nullable=true` and an absent col's type
  to `Unknown`, while the compiled schema pins types/nullability — that metadata differs but the
  values are equal. State this in the test.
- **EXPAND heterogeneity caveat:** `eval::expand` splices element values POSITIONALLY from the FIRST
  array element's struct schema (`Value::Array::type_of` uses `items.first()`), so the fixture
  messages MUST be homogeneous (same key set) or a later differently-shaped message misaligns. Use a
  homogeneous fixture; note the heterogeneous case as a §13 park if it matters.

### The 5 parks + files to touch
- Parks recorded on `.workaholic/tickets/archive/work-20260705-032203/20260704145138-*.md` and the
  active concern `.workaholic/concerns/21-13-conversion-parity-the-slack-script.md` (delete on close).
- Twin to upgrade: `crates/parser/tests/fixtures/slack.qfs` (view→`/slack/history OF … AS /http/slack/
  conversations.history |> DECODE json |> EXPAND messages`; cursor→`next 'response_metadata.next_cursor'`;
  map→the `{channel,text}` struct) + `crates/parser/tests/slack_twin.rs` (still parses to 4 installs).
- The old envelope-gap assertion test `declared_driver.rs::slack_twin_reads_hermetically_and_records_
  the_envelope_parity_gap` uses `rest_read_rows` directly (still valid at the low level) — the NEW
  row-equivalence test belongs in `crates/exec/src/declared.rs` tests (or a new crates/test e2e),
  comparing `qfs_driver_slack::read_rows(MockSlackClient)` vs `eval_view_body(fetch=envelope)`.

## Quality gate (hermetic)

- `VALUES (struct(...))` maps a row into the exact wire body Slack expects (asserted on the
  recorded MockHttp request body).
- The declared twin's read is row-equivalent to the compiled Slack driver's on the same
  two-page envelope fixture (the tier-2 acceptance bar from the blueprint).
- Conformance on the twin's read PASSES (flip the tier-1 expectation that recorded the gap).
- The closed parity parks are removed from the active concern corpus.
- All gates green, sequential.
