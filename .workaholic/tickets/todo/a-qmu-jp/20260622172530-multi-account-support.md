---
created_at: 2026-06-22T17:25:30+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Infrastructure, Config]
effort:
commit_hash:
category:
depends_on: [20260622123701-unify-gmail-gdrive-ftp.md]
---

# Multi-account support: auth several Google accounts and switch between them

## Overview

Today the tool caches exactly one token at `~/.config/<tool>/token.json`, so it serves one Google account. This ticket lets a user **authorize multiple Google accounts** and **choose which one a command runs as** — both interactively (a persistent active account) and one-shot (a per-command flag, essential for agents/pipes that have no session state). Account identity is the **email address**, fetched after consent via Gmail `Users.GetProfile(me).EmailAddress` (covered by the existing `gmail.modify` scope — **no new scope**). Scoped against the merged `gftp` tool (see `depends_on`).

## Surface

```
gftp auth                      # run consent, fetch the account's email, store its token; first account becomes active
gftp accounts                  # list authorized accounts; mark the active one (and -json form)
gftp whoami                    # print the active (or --account-resolved) account email
gftp account use <email>       # switch the persistent active account (interactive / shell sessions)
gftp account rm <email>        # remove one account's cached token (per-account logout)
gftp --account <email> <cmd> … # run a single command as <email>, overriding the active account (one-shot / agents / pipes)
```

**Resolution precedence** for which account a command uses: `--account <email>` flag → persistent active pointer → the only account if exactly one exists → clear error ("no account; run `gftp auth`") if none.

## Storage layout (`~/.config/gftp/`)

```
credentials.json                 # shared OAuth client (one Desktop client for all accounts)
accounts/<email>/token.json      # one cached token per account (0600, under 0700 dirs)
active                           # small file holding the active account's email (or in a config.json)
audit.jsonl                      # shared log; each entry records which account performed the op
```

## Key Files

- `main.go` — `configDir()` stays `~/.config/gftp/`; replace the single `defaultTokenPath()` with **per-account** resolution: parse a global `--account` flag, read the `active` pointer, map email → `accounts/<email>/token.json`. Wire the chosen token path into `auth.Client`. `auth` subcommand now fetches+stores by email and updates `active` on first account.
- `internal/auth/auth.go` — `Client(ctx, credsPath, tokenPath)` is already token-path-parameterized (good). Add a small helper to **fetch the account email after consent** (Gmail `Users.GetProfile`), used by `auth` to name the account dir. Keep the `http://localhost` redirect.
- `internal/shell/commands.go` — add `accounts`, `whoami`, `account use|rm` verbs; thread the resolved account through the shell. Inside the interactive prompt, `account use` flips the active account **for the session** (and optionally persists); show the active account in the prompt string (e.g. `gftp(a@qmu.jp):/mail>`).
- `internal/shell/shell.go` — the shell holds the active-account identity; backend clients are (re)built for the selected account. `--account` (one-shot) selects before the shell/clients are constructed.
- New small package or `main.go` helpers for the **accounts registry** (list `accounts/*/`, read/write `active`), with unit tests.
- `internal/audit/audit.go` — add an `Account` field to the Entry so a shared `audit.jsonl` attributes each mutation to the account that made it.
- `README.md`, `plugins/gftp/skills/gftp/SKILL.md` — document multi-account: `auth` adds accounts, `accounts`/`whoami`, `account use`, and the `--account` flag (call out that **agents/one-shot/pipes must use `--account`** since there's no session).

## Related History

- Both tools' `auth.Client` already takes an explicit `tokenPath` (the trip kept it parameterized), so per-account token files are a natural extension — no auth-flow rewrite.
- `gmail-ftp` deliberately has **no `logout`**; `account rm <email>` provides a scoped logout and complements a future global one. The earlier debugging session established the consent flow (`http://localhost` redirect) that each `auth` invocation reuses.
- Ties to the composability design (Ticket B / the in-vs-out discussion): persistent active account = the **in-prompt** mechanism; `--account` flag = the **out-of-prompt** mechanism. Same operation, two access paths, modeless.

## Implementation Steps

1. **Accounts registry:** helpers to list `accounts/<email>/`, read/write the `active` pointer, resolve an email → token path. Unit-test with a temp config dir.
2. **`auth` adds an account:** run the consent flow into a temp/By-email token, fetch `Users.GetProfile(me).EmailAddress`, move the token to `accounts/<email>/token.json`, set `active` if it's the first account. Re-`auth` of an existing email refreshes its token.
3. **Account resolution:** global `--account <email>` flag parsed before client construction; precedence flag → active → sole-account → error. Applies to ALL commands including `auth` (so `auth` can target re-adding a specific email) and the cross-backend `cp`/pipes.
4. **Verbs:** `accounts` (list + active marker, `-json`), `whoami` (resolved email), `account use <email>` (set active; in-prompt also switches the live session + rebuilds clients), `account rm <email>` (delete that token dir; if it was active, clear/repoint `active`).
5. **Audit attribution:** add `Account` to Entry; record it on every mutation; reader/TUI can show it.
6. **Prompt + UX:** interactive prompt shows the active account; clear errors when an unknown `--account` is given or none are authorized.
7. **Docs** README/SKILL, emphasizing `--account` for non-interactive/agent/pipe use.
8. **Quality gate:** `go build/vet/gofmt/test` clean. Tests (no live creds): registry list/active read-write; resolution precedence (flag > active > sole > none-error); `account rm` repoints active; audit records the account; account email parsing/sanitization for the dir name.

## Considerations

- **Identity fetch needs no new scope:** Gmail `Users.GetProfile` returns the email under the existing `gmail.modify` scope — do NOT add a `userinfo.email`/People scope. (If a Drive-only future build dropped Gmail scopes, identity would need another source — out of scope now, since `gftp` always holds Gmail scopes.)
- **Per-command selection is mandatory for composability (design/modeless):** one-shot invocations, agents, and pipes have no session, so `--account` must work standalone; never require entering an interactive "account mode". The persistent active pointer is purely an interactive convenience.
- **Token isolation (security/defense-in-depth):** each account's `token.json` is 0600 under a 0700 dir; never load two accounts' tokens into one client; the shared OAuth `credentials.json` is the only shared secret. Email used as a directory name must be sanitized (lowercase, filesystem-safe) to avoid path issues.
- **Cross-account transfers are out of scope for v1:** a `cp`/pipe runs as a **single** resolved account for both `/drive` and `/mail` ends. Moving account A's Drive file into account B's draft (two different accounts in one command) is a future extension — for now `--account` selects one identity for the whole command; document this.
- **Active-pointer races:** the `active` file is a tiny single-writer pointer; one-shot commands should prefer `--account` and not mutate `active`. Only `account use` writes it.
- **Migration:** an existing single `~/.config/gftp/token.json` (or a fresh install) — on first `auth`, write into the new `accounts/<email>/` layout and set active; optionally adopt a legacy `token.json` by fetching its email once.
