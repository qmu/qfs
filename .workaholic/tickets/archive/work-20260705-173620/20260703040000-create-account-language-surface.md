---
created_at: 2026-07-03T04:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort:
commit_hash: 7d92ace
category: Added
depends_on: [20260703030000-paste-back-browser-consent.md]
---

# In-language account declaration: a CREATE ACCOUNT statement (values stay out-of-band)

Owner ask (2026-07-03, first-user session): the setup surface should be expressible in the QUERY
LANGUAGE, not only as CLI subcommands — "I was not expecting qfs subcommand but a part of
syntax". Mounts already are (`CONNECT /mail TO gmail ACCOUNT 'a@qmu.jp'` / `DISCONNECT`, verified
live; the CLI is the twin). The remaining CLI-only layer is the ACCOUNT declaration
(`qfs account add/remove/rotate/revoke`).

## The boundary to keep (RFD §10 / §4.5 — non-negotiable)

A qfs statement is pure, previewable, logged, audited TEXT. A secret VALUE never appears in it.
Secret REFERENCES are fine (`SECRET 'env:VAR'` / `'vault:…'`, as `CREATE CONNECTION` already
does). So the in-language surface declares the account's EXISTENCE and METADATA; sealing the
token bytes stays out-of-band (stdin import or the paste-back browser consent,
`20260703030000`).

## Sketch (design to confirm in-ticket, not prescribed)

```qfs
CREATE ACCOUNT google 'a@qmu.jp'                          -- declare; token sealed separately
CREATE ACCOUNT github 'work' SECRET 'vault:github/work'   -- reference form, mirrors CREATE CONNECTION
REMOVE /sys/accounts/google/a@qmu.jp                      -- if accounts get a /sys surface
```

## Open design decisions (flag, decide with the owner before implementing)

1. **What state does the statement write?** `account add` today does two things: seal the token
   (out-of-band — stays CLI) and record consent rows keyed by `(kind, account)`. Does CREATE
   ACCOUNT record the consent (it is metadata, but the t54 gate requires a signed-in operator —
   the statement path must enforce the same gate), or only declare a label the CLI later fills?
2. **A `/sys/accounts` read surface?** `qfs account list` reads the vault listing today; a
   `/sys/accounts` node would make accounts queryable like `/sys/paths` (consistent with the
   CONNECT desugar to `/sys/paths` effects) and give REMOVE a natural target.
3. **Rotate/revoke in-language?** Rotate needs a new secret VALUE (out-of-band by rule);
   revoke is metadata and could be a statement. Decide one-concept-one-word naming.
4. Grammar: contextual idents like CONNECT (no new frozen keywords — the additive-by-
   contextual-ident contract; see `connect_and_disconnect_add_no_frozen_keyword`).

## DECISION (2026-07-06, owner — resolved one by one, all A)

1. **What state does the statement write? → RECORD CONSENT (gated).** `CREATE ACCOUNT` writes the
   same `connection_consent` rows `qfs account add` does (Google → gmail/gdrive/ga three rows keyed
   by the email; cloud → one row keyed by `(provider,label)`), SHARING `account.rs`'s logic (not a
   fork). The apply path enforces `require_signed_in` (`connection.rs:279`) to fill `subject` —
   unlike CONNECT's ungated `/sys/paths` write, because recording consent needs the operator
   identity. The token VALUE stays out-of-band (stdin import / paste-back consent); a "consent
   recorded but token not yet sealed" middle state is safe (bind fails closed until the secret
   resolves).
2. **A `/sys/accounts` read surface? → YES.** Add `SysNode::Accounts` mirroring `/sys/paths`: a
   selectors+metadata scan over `connection_consent` (`provider/account/subject/scope/created_at`),
   STRUCTURALLY no token column. Makes accounts queryable (`SELECT * FROM /sys/accounts`) and gives
   REMOVE its natural target (`REMOVE /sys/accounts/<provider>/<account>`, deleting BOTH the consent
   row(s) AND the vault token — the complete-deletion contract of `account remove`).
3. **Rotate/revoke in-language? → NEITHER this ticket.** In-language verbs are `CREATE ACCOUNT`
   (declare+consent) and `REMOVE` (complete delete) only. `rotate` needs a new secret VALUE →
   CLI-only by rule. `revoke` (disable-but-keep, a state flag) is a different concept that does not
   map cleanly to INSERT/REMOVE and is lower urgency (remove+recreate covers it) — deferred.
4. **Grammar → contextual ident, decided (not an owner call).** `create_account_stmt` slots into the
   CREATE alt like `create_driver_stmt`; `"account"` is already non-frozen (`tests.rs:632`). No new
   frozen keyword. The `SECRET '<ref>'` clause reuses `conn_secret_clause` (reference-only enforced
   at RESOLVE time via `secret_ref.rs`, as CREATE CONNECTION does — no new parse-time check).
   `CREATE ACCOUNT` desugars to `INSERT INTO /sys/accounts` (an ordinary effect, no new `Statement`
   variant), applied by the SysApplier through a new backend method that gates + shares the consent
   writer.

## Key files

- `packages/qfs/crates/parser/src/grammar.rs` (`connect_stmt` as the pattern),
  `crates/qfs/src/sys.rs` (the `/sys/paths` apply — the model for a `/sys/accounts` surface)
- `packages/qfs/crates/qfs/src/account.rs` (the bookkeeping the statement must share, not fork)
- `docs/guide/connect.md`, `docs/guide/account-model.md` (document the statement forms)

## Quality Gate

- The new statement(s) parse, preview, and commit through the standard gates; the cookbook parse
  ratchet covers any recipe added to docs.
- No secret VALUE can ride in a statement (parser/test-enforced: the SECRET clause accepts only
  `env:`/`vault:` references, as today).
- The CLI verbs and the statements write the SAME state (one source of truth, like
  connect/`/sys/paths`); `cargo test --workspace` / clippy / fmt / gen-docs / gen-skills green.
