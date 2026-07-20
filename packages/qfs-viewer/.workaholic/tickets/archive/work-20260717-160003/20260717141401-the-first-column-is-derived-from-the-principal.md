---
created_at: 2026-07-17T14:14:01+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission: qfs-viewer-mvp
---

# The first column is derived from what qfs declares, not hardcoded

> **REVISED 2026-07-17.** This ticket was drafted against a premise that turned
> out to be false, and the developer re-decided the design. The title changed
> with it: the first column is derived from **what qfs declares**, not from a
> **principal**. The original framing, its measurements, and why each one
> collapsed are kept below rather than deleted — the falsification is the most
> useful thing this ticket now carries, and a queued ticket that quietly
> rewrote its own premise would teach nobody why the second design is the right
> one.

## Overview

From today's design discussion. The developer, on what the first column is:

> 実際に github に接続して querying する際のパスと、driver 一覧があって、
> そのドライバーにどんなコネクションがあるか、というのは別のはずです

**These are two different axes, and this is the substance of the ticket.** They
are not two views of one list:

| axis | what it answers | what it is a function of |
| --- | --- | --- |
| **query paths** | the path you actually query (`/github/qmu/qfs/pulls`) | the operator's *configured connections* |
| **admin view** | which drivers exist, and what connections each driver has | the connections registry |

Today the viewer's root column is the markdown corpus alone — which answers
neither axis. It gains both, **derived from qfs, never held as a literal**.

The developer also placed a future explicitly, so it is not smuggled into the
default:

> 1列目によく使うパスへのリンクや qfs クエリ実行リンクを持つ、というのは将来あり得る

Frequently-used path links and a query-execution link are **a possible future,
not the default**. Do not build them here. Note the distinction that makes axis
1 legal anyway: **`/sys/paths` is not "よく使うパス"**. "Frequently used" is a
ranking derived from *behaviour* — it would require the viewer to track usage
nobody declared, which is the exact defect this ticket exists to refuse.
`/sys/paths` is the *declared registry of paths that exist*. One is invented
from watching you; the other is read from what the operator configured.

## The original premise, and why it is false — MEASURED

The ticket was drafted on this quote:

> by default でサインインしていない状態はサインインのみがメニューにあり、
> サインイン後には qfs の admin としてのメニューと、ユーザーとしてのメニューが並ぶ

**There is no sign-in to be unsigned from.** Measured against the installed
`qfs` binary, read-only:

- `qfs identity --help` lists exactly two subcommands: **`whoami`** and
  `help`. Sign-up is gone — "(Signing up moved to `qfs init` — ADR 0008.)"
- `qfs identity --help` states it outright: "**AUTHENTICATION ONLY** (decision
  §4.1: identity is not authorization). A signed-up user can do nothing
  privileged yet — there is local sign-up, **no session**."
- `qfs init --help`: "register the operator identity — one operator per OS
  user, **no password (your OS login is the authentication**; the email is an
  accountability label)".

So the unsigned/signed-in axis **does not exist in the substrate**. qfs does
not have sessions to be in or out of; the OS login already authenticated you,
and there is one operator per OS user. Rendering a "Sign in" entry would
**invent an undeclared distinction** — precisely the defect this ticket
correctly refused for the admin menu. The refusal was right and the premise it
was applied to was wrong.

**The viewer already has a `Principal`, and it contradicts the old
acceptance.** `src/domain/model/Principal.ts` models a config-declared bearer
token (`{name, key, role: reader|editor}`) with `Access = Open|Granted|Denied`,
wired through `api.ts`, and **open by default** by an argued decision:

> npx qfs-viewer at your own repository root, on your own machine, reading your
> own files. Demanding a token there would be security theatre performed for an
> audience of one.

The old acceptance criterion — "there is no path by which the corpus appears in
an unsigned root" — **contradicts that shipped decision**. It is struck. The
corpus is not gated behind an access control qfs does not enforce.

## The developer's rulings (2026-07-17) — settled

1. **`Role` remains NOT a grant.** `identity::Role` (`Owner|Admin|Member`,
   `invite.rs:141`) is an invite label. qfs's own comments: "a [`Role`] here is
   a coarse label for a later ACL, **not a grant**" (`invite.rs:15`); `Admin`
   is "**not privileged yet**" (`invite.rs:144`); the taxonomy is an "**OPEN
   PRODUCT DECISION (flagged, t55)**" (`invite.rs:135`). The `User` struct
   (`model.rs:80`) has no role field — there is no `User → Role` edge to read.
   **Render nothing that treats a role as permission.**
2. **Threading a request principal through qfs is its own mission** — not this
   ticket's. Do not wait for it and do not fake it.
3. **Build the root column from `/sys/paths` and `/sys/connections`.** They
   answer the developer's two axes **today**, machine-readably, over the
   interface this viewer already speaks — no session required.

## The two axes are real — verified live against `/sys`

Both paths describe and run clean (`qfs describe`, exit 0; `qfs run … |> limit
200`, exit 0). Both are `relational_table`:

| path | columns | verbs |
| --- | --- | --- |
| `/sys/paths` | `path, driver, at, secret_ref, alias_of, host, account, app, created_at` | `select`, `insert`, `remove` |
| `/sys/connections` | `driver, connection, created_at` | `select` only |

**The decisive measurement: the two axes do not even share a vocabulary.** On
the machine this was measured on, the distinct `driver` values differ between
them — `/sys/paths` carries driver names like `gmail` and `gdrive` where
`/sys/connections` carries `google`; `/sys/paths` carries `ghdecl` where
`/sys/connections` carries `github`. **One connection backs several query
paths.** A single account is the substrate for two different things you query,
under two different driver names.

So fusing the axes into one list is not merely against the developer's
correction — **it is not expressible**. There is no 1:1 map to fuse along. This
is qfs's own design, stated in its own source at `catalog.rs:110`:

> The COMPILED registry only — gen-docs must be a pure function of the binary,
> never of the operator's live CONNECT-ed/declared mounts (which would pollute
> + de-idempotent the catalog).

The catalog is a function of the *binary*; the connections are a function of
what the *operator* CONNECTed. Two axes, in the substrate, independently
confirmed by the developer.

**Do not fuse them into one first column.**

### The generated catalog is the wrong source, and this is why

An earlier draft of this ticket proposed driving the column from the generated
`docs/drivers.md` / `driver_catalog`. That source is **holed**, and the
mechanism matters:

- **`/cf` is registered and still absent — a real bug.** `describe.rs` puts
  `CfDriver` in `compiled_describe_registry()` explicitly "so `qfs describe
  /cf` … resolve[s] and the t40 driver catalogue surfaces them". The registry
  holds 16; the catalog prints 15. `catalog.rs`'s best-effort `continue` drops
  it. **The code's own stated intent is silently unmet.**
- **`/sql` and `/git` are absent by design** — connection-conditional
  (`commit.rs:265-290`), so the catalog *cannot* see them.

A column built on the catalog would lose drivers and say nothing about it.
`/sys/paths` and `/sys/connections` are the operator's live registry, which is
what both axes are actually about. **Ask qfs; do not count by hand** — this
ticket's own first attempt at the driver list was wrong in three places, which
is the argument.

## Implementation Steps

1. **Derive axis 1 from `/sys/paths`.** Each declared path is a link that opens
   that path as a qfs column — the existing `qfsStop` machinery, no new
   navigation concept. These are the openable things, so they join the root
   `MenuLevel`'s entries.
2. **Derive axis 2 from `/sys/connections`,** grouped by driver: which drivers
   exist, and what connections each has. It is a **view, not navigation** — it
   gets no links and contributes no `MenuLevel` entries. That is how the two
   axes stay separate in the Scene and not just on screen.
3. **Model the unanswerable honestly.** "qfs could not be asked" is not "qfs
   declares nothing". A closed union — `Declared` / `Undeclared` /
   `Unanswerable` — exhaustively matched, so an added variant fails `tsc`.
   `Undeclared` renders nothing; `Unanswerable` renders qfs's own words.
4. **Keep the corpus in the root column, ungated.** `Connection.ts:101` ships
   the rule: "Markdown browsing does not need qfs — only qfs paths do." A root
   reachable only through a qfs-derived column would break `npx qfs-viewer` on
   a machine with no qfs — the product's headline case — and would be the same
   structural error as gating it behind a sign-in that does not exist.
5. **Build no admin *operation*.** Axis 2 is `select`-only (verified: every
   write verb on `/sys/connections` is `false`). It reads what the operator
   configured; it confers nothing.
6. **Do not build the future.** No frequently-used-path links, no
   query-execution link (developer-scoped as 将来あり得る).

## Policies

- `workaholic:design` / `policies/admin-isolation.md` — **the policy to answer,
  not to wave past.** It requires admin functions on a separate surface with a
  separate authentication path. This axis 2 satisfies it *by construction*
  rather than by exception: it is `select`-only over the operator's own
  registry, on the operator's own machine, and qfs's model is "one operator per
  OS user, no password — **your OS login is the authentication**". The OS login
  *is* the separate authentication event; there is no privileged operation to
  isolate and no role check standing in for one. The moment axis 2 gains a
  write verb, this reasoning expires and the policy binds for real.
- `workaholic:design` / `policies/no-dark-patterns.md` — a control that does
  nothing is worse than absent. No disabled menus, no admin entry claiming a
  privilege qfs never conferred.
- `workaholic:design` / `policies/self-explanatory-ui.md` — "Empty states that
  display nothing" are a defect; an error must say what happened and what to do
  next. This is why `Unanswerable` renders qfs's own words instead of
  collapsing to silence — and why `Undeclared` renders nothing at all, because
  an absent *feature* is not an empty *state*.
- `workaholic:implementation` / `policies/coding-standards.md` — the answer
  states are a closed union; an unhandled state is a compile error, not a blank
  column.
- `workaholic:implementation` / `policies/objective-documentation.md` — the
  falsified premise is recorded above rather than quietly rewritten.
- `workaholic:safety` — identity ≠ authorization is qfs's own §4.1 rule. The
  viewer must not be the layer that turns a label into a grant.

## Key Files

- `src/domain/model/Declaration.ts` — **new.** The two axes as types, and the
  `Declared` union. Pure: takes the runner's raw answer, so it is testable
  without a qfs.
- `src/entrypoints/columns.ts` — `corpusColumn` → `rootColumn`; the two axes
  rendered, the landmark rule (ADR 0010 D2) untouched.
- `src/domain/usecase/scene.ts` — `corpusLevel` → `rootLevel`: its entries are
  no longer only the corpus.
- Reference only, never modified — `qfs/packages/qfs/crates/`:
  `identity/src/invite.rs:135-145` (`Role`, the flagged open taxonomy),
  `identity/src/model.rs:80` (`User`, no role), `qfs/src/catalog.rs:109-110`
  (the best-effort skip, and the two-axes statement).

## Quality Gate

### Acceptance Criteria

- The root column's two qfs axes are a function of what qfs declares, and stay
  **separate**: axis 1 navigates, axis 2 does not.
- **No driver/mount/prefix list is a literal in this repository.** Naming
  `/sys/paths` as the question asked is not holding the answer.
- Where qfs declares nothing, **nothing renders** — not an empty menu, not a
  disabled one.
- The answer union is exhaustive; an added variant fails `tsc`. "Cannot say" is
  never collapsed into "none".
- No frequently-used-path links and no query-execution link ship.
- The root page keeps **exactly one `main`** landmark (ADR 0010, D2).
- **STRUCK** (contradicts the shipped open-by-default `Principal` decision):
  ~~"Unsigned renders sign-in and nothing else; there is no path by which the
  corpus appears in an unsigned root."~~
- **STRUCK** (no such state exists in qfs): ~~"`SoleUser::None`/`Many` is a
  distinct handled state."~~ The criterion's *substance* — the unanswerable is
  its own state, in a closed union, never collapsed into "none" — is preserved
  and now binds on the axis answers, which is where the distinction is real.
  `whoami` itself is out of scope: it is not one of the developer's two axes,
  and there is no session that would make it a viewer concern.

### Verification Method

```sh
# no hardcoded prefix list anywhere in the source
grep -rnE '"/(mail|drive|github|slack|sql|cf|git)"' packages/qfs-viewer/src/

./scripts/check-all.sh   # raw exit code, unmasked
```

### Gate

- `./scripts/check-all.sh` exits 0.
- **No admin operation ships.** Axis 2 reads; it does not act.

## Considerations

- **Client data.** `/sys/paths` and `/sys/connections` return the operator's
  real accounts and real connection names. Nothing from a live `/sys` response
  belongs in source, tests, fixtures, or commit messages. The tables above are
  schema and driver *vocabulary* only, deliberately.
- **Do not repeat `catalog.rs`'s silent skip.** A row missing `path` or
  `driver` must not be quietly dropped — that is exactly the best-effort
  `continue` this ticket criticises upstream, reproduced locally. It fails the
  whole answer closed, with a reason.
- **Aliases are declared, and so is their alias-ness.** `/sys/paths` carries
  `alias_of`. Listing an alias as though it were an independent connection
  would overstate what is connected — the same class of defect as inventing a
  menu. Show the alias, and show that it is one.
- **Cost: two qfs invocations per root render**, since the root column renders
  on every page and ADR 0003 forbids caching. Consistent with the shipped
  architecture (a live table's value is being live), but it is a real cost and
  the first candidate if the root page ever needs to get faster.
- **`/cf`'s disappearance is still worth filing upstream.** The catalog's
  best-effort `continue` silently drops a driver the code deliberately
  registered *so that the catalog would show it*. This ticket routes around it
  by using the live registry instead, but qfs's catalog is still lying by
  omission about its own binary.
