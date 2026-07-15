---
skill_name: qfs-faq
skill_description: Use when answering an operator "how do I…" or troubleshooting question about qfs — connecting an account (Gmail, Drive, GitHub, Slack, S3/R2, SQL, git), adding a second or different-organization Google account, the describe→preview→commit safety loop, common errors (unknown source, org_internal / access blocked, the irreversible gate), exit codes, and which per-service skill to route a task to.
---

# FAQ & operator reference

Straight answers to the questions an operator actually asks — *"how do I add a Drive connection for
another account?"*, *"why is my Google sign-in blocked with `org_internal`?"*, *"what does PREVIEW
mean, and how do I actually apply a change?"* Every answer maps to a `qfs` command; nothing here
requires reading qfs source. For the full recipe set on any one service, jump to its cookbook
(the [routing index](#which-skill-answers-a-task) is at the bottom).

## The shape of every answer

qfs runs the same loop for every task, so most "how do I…" questions reduce to it:

1. **Describe** a path — `qfs describe /drive` — to learn its columns, verbs, and procedures. Runs
   offline, no credentials, no network.
2. **Write** a query against it.
3. **Preview** — `qfs run "<statement>"` shows the exact effect plan and changes nothing.
4. **Commit** — add `--commit` to apply it (`--commit-irreversible` for the ones that can't be
   undone).

A **read** returns rows the moment the source is connected. A **write** (`insert`, `update`,
`upsert`, `remove`, `call`) **previews by default** and touches nothing until you `--commit`.

## Connection & account setup

### The three one-time gates

Reaching a **cloud** service (Gmail, Drive, GitHub, Slack, S3/R2) takes three one-time steps.
`/local`, `/sys`, and local files need none of them; a local SQL file or git repo needs only the
mount (no secret).

1. **A readied machine** — `qfs init` once per machine creates the encrypted credential store (it
   walks you through choosing the passphrase) and registers the operator. There is no password: your
   OS login is the authentication, the email is an accountability label.
2. **An authorized account** — `qfs account add <provider>` seals the account's token into the vault
   and records your consent. qfs refuses a cloud `account add` until `qfs init` has run.
3. **A mount** — `qfs connect <path> --driver <driver> --account <label>` binds that account to a
   path you choose. A cloud path exists **only after a connect**.

### How do I add a Gmail or Google Drive connection?

Gmail, Drive, and Google Analytics can share **one** Google account consent. Register a labeled
OAuth app, authorize the account through that app, then mount each service where you want it:

```sh
qfs init you@example.com                         # ready the machine (once)
cat credentials.json | qfs app add google qmu     # your OAuth app's client credentials (once)
qfs account add google --app qmu                  # paste-back browser consent; token sealed, never printed
qfs connect /mail  --driver gmail  --account you@gmail.com   # mount Gmail at /mail
qfs connect /drive --driver gdrive --account you@gmail.com   # mount Drive at /drive
```

`qfs account add google --app qmu` prints a URL — open it in your **local** browser, approve, and paste the
redirect URL back (this works over plain SSH). To provision a refresh token out of band instead of
the browser flow, pipe it on stdin with the email as the label:

```sh
printf %s "$REFRESH_TOKEN" | qfs account add google you@gmail.com --app qmu
```

After connecting, `qfs describe /drive` shows the schema and verbs, and `qfs connect --list` shows
the mount. See the **[Gmail](/cookbook/gmail)** and **[Google Drive](/cookbook/gdrive)** cookbooks
for the read/write recipes.

### How do I connect a second account?

The **mount carries the account** — there is no "active" account and no command that switches one.
A second account is simply a second mount at a second path:

```sh
qfs connect /drive      --driver gdrive --account you@gmail.com      # your Drive
qfs connect /work/drive --driver gdrive --account teammate@work.com  # a second Drive, side by side
```

Both mounts coexist in the same process; every `/drive/…` recipe works verbatim as `/work/drive/…`.
Remove a mount with `qfs disconnect /work/drive` (idempotent).

> **OAuth apps are labeled.** Multiple Google app registrations can coexist, such as `qmu` and
> `client-a`. Authorize each Google account with the app that should mint or service its consent,
> then mount paths against the account label.

### Per-service quick setup

For a non-Google service, pipe the credential **value** on stdin — never on argv, where it leaks
into shell history and the process table — then mount the path:

```sh
printf %s "$GH_TOKEN"             | qfs account add github work       # GitHub personal-access token
printf %s "$SLACK_TOKEN"          | qfs account add slack team        # Slack bot/user token
printf %s "$AWS_SECRET_ACCESS_KEY" | qfs account add objstore prod    # S3/R2 secret access key
qfs connect /github --driver github --account work
qfs connect /slack  --driver slack  --account team
qfs connect /s3     --driver s3     --account prod
```

`qfs account list` shows the labels and metadata only, never a secret. A local **SQLite** file or
**git** repo stores no secret, so it needs only a mount — the in-language `CONNECT` statement (no
`ACCOUNT` clause):

```qfs
CONNECT /db TO sqlite AT 'file:app.db'
```

### The in-language twins

Every `qfs connect` / `qfs account add` has a query-language twin, so a connection can live in a
script or a server config instead of a shell command. `CONNECT` binds a path (the `ACCOUNT` clause
names the account label; the optional `HOST` names which qfs host owns the mount), and
`CREATE ACCOUNT` records consent in the language:

```qfs
CONNECT /mail TO gmail ACCOUNT 'you@gmail.com' APP 'qmu' HOST 'local'
```

```qfs
CREATE ACCOUNT google 'you@gmail.com' APP 'qmu'
```

## "Access blocked" / `org_internal` — a different-organization Google account

**Symptom.** Authorizing a Google account fails in the browser with *"アクセスをブロック … 組織内で
のみ利用可能です"* / **"Error 403: `org_internal`"**, or the consent screen refuses the account
outright.

**Cause — this is a Google OAuth configuration, not a qfs bug.** Your OAuth app's consent screen is
set to **Internal**, so only users *inside the Workspace organization that owns the app* may
authorize it. An account that lives in a **different** organization (a client's domain) is refused
before qfs ever sees a token. `org_internal` is Google's error string; qfs never emits it.

**Fixes, ranked.**

1. **Flip the consent screen to External and add the account as a test user** (Google Cloud console →
   the OAuth app's *Audience* / *OAuth consent screen* → set **External**, add the address under
   *Test users*). Note: while the app is in *Testing*, refresh tokens for the sensitive Drive scope
   expire in ~7 days — publish the app to **Production** to make them durable.
2. **Authorize with an OAuth client issued by the target account's own organization** (Internal there
   → it works, with no verification review). Register it under its own label, for example
   `cat credentials.json | qfs app add google client-a`, then authorize with
   `qfs account add google teammate@client.example --app client-a`.
3. **Avoid cross-org auth entirely** — have the other organization **share** the specific Drive files
   or folders into an account you already connect, and reach them from that account's `/drive/shared`.

Option 1 is usually the least disruptive. Option 3 avoids the OAuth question altogether when you only
need a few shared items.

## The query & safety loop in detail

**`describe` and `preview` are always offline.** `qfs describe <path>` and a bare `qfs run "<write>"`
(no `--commit`) build the plan with no credentials and no network — you can inspect any path,
including a cloud one you have not connected yet:

```sh
qfs describe /mail/drafts --json | jq .verbs
qfs run "insert into /mail/drafts values ('alice@example.com', 'Hi', 'Body text')"
```

The second command prints a PREVIEW and creates nothing:

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> mail:/mail/drafts [affected 1]
  total affected: 1
```

**Apply it with `--commit`.** Actions that can't be undone (sending mail, merging a PR, trashing a
file) need a second acknowledgement in a one-shot — `--commit` alone is refused, fail-closed:

```sh
qfs run "insert into /mail/drafts values ('alice@example.com', 'Hi', 'Body')" --commit
qfs run "/mail/drafts |> call mail.send" --commit --commit-irreversible
```

**Output formats.** qfs prints the human table on a terminal and JSON when piped (so it composes with
other tools). Force either explicitly:

```sh
qfs run "/mail/inbox |> select date, subject" --format table   # always the table
qfs run "/mail/inbox |> select date, subject" --json           # always JSON
```

**Warm the session** so repeated commands don't re-prompt for the passphrase (default 8h window);
`qfs auth --lock` drops it again:

```sh
qfs auth
qfs auth --lock
```

## Common errors & fixes

| You see | What it means | Fix |
| --- | --- | --- |
| `unknown source \`mail\`` (`kind: capability`) | The path is not connected — cloud sources fail closed, never return empty rows | `qfs account add <provider>` then `qfs connect <path> --driver <driver> --account <label>` |
| `connect a Google account to read mail` | The path resolved but there is no signed-in operator or recorded consent yet | Run `qfs init`, register an app with `qfs app add google <app>`, then `qfs account add google --app <app>` |
| A write "affected 1" but nothing changed | You saw a **PREVIEW** — writes change nothing without `--commit` | Re-run with `--commit` |
| `--commit` of a send/remove is refused | The plan is **irreversible** and a one-shot needs the explicit extra ack | Add `--commit-irreversible` |
| An `auth` error resolving a secret | The vault is locked, or the account's credential was revoked | `qfs auth` to unlock; `qfs account rotate <provider> <label>` to re-mint |

**Exit codes** are stable so an agent can branch on them:

| Code | Meaning |
| --- | --- |
| `0` | success — rows rendered, or a PREVIEW shown |
| `2` | parse or CLI usage error (a relative path, a bad flag) |
| `3` | capability — unknown source, or a verb the path does not support |
| `4` | a destructive set-wide plan was previewed without `--commit` |
| `5` | an effect failed to apply during commit |
| `6` | auth / credential failure resolving or using a secret |

The JSON error envelope carries a stable `code` and `kind` alongside the message, e.g.
`{"error":{"code":"unknown_source","kind":"capability","message":"unknown source \`mail\`"}}`.

## Skill routing by task

One line each — route "how do I read/write X" to the service's own skill:

- **Gmail** (search, triage, draft, send, label) → `qfs-gmail`
- **Google Drive** (browse, download, upload, folders, trash) → `qfs-gdrive`
- **SQL databases** (filter, aggregate, join, update) → `qfs-databases`
- **Local files & S3/R2, format conversion** → `qfs-files`
- **git** (versioned tree, history, commit) → `qfs-git`
- **GitHub** (pull requests, issues, merge a PR) → `qfs-github`
- **Slack** (read a channel, post a message) → `qfs-slack`
- **A query spanning more than one service** (join, union, federate) → `qfs-cross-service`
- **Server side** (jobs, triggers, HTTP endpoints, cached views) → `qfs-automation`
