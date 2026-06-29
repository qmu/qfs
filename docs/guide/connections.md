# Connections & credentials

You don't need any credential to **describe** a path or **preview** a query — both are completely
offline. You only need one to **commit** against a live service.

## Unlocking the store with `QFS_PASSPHRASE`

Stored credentials live in an encrypted local vault. `QFS_PASSPHRASE` is the **master passphrase
that unlocks that vault** — qfs derives an argon2id AEAD key from it to encrypt and decrypt the
store. It is **not** a service credential (not your mail token, not an API key); it only protects
the local file at rest. The per-store salt is created automatically — the passphrase is the one
thing you supply.

`connection add`, `connection list`, and `connection remove` all need it exported in the shell that
runs them (`connection use` does not — it only flips which stored connection is active). With it
unset, those commands fail closed with a clear error.

Set it without leaking it into your shell history:

```sh
read -rs QFS_PASSPHRASE; export QFS_PASSPHRASE   # typed value isn't echoed or saved to history
```

Alternatives: source a `0600`-permissioned file you keep outside the repo, or (with
`HISTCONTROL=ignorespace`) prefix the `export` with a leading space. Avoid a bare
`export QFS_PASSPHRASE=secret` typed inline — it lands in your history.

This is **at-rest confidentiality only**: a process or someone with access to your running shell can
read `QFS_PASSPHRASE` straight out of the environment. It protects the stored blob, not a live host.

## Storing a credential

```sh
qfs connection add <service> <name>
```

- `<service>` is the driver the connection belongs to — `mail`, `s3`, `github`, `slack`, `sql`, …
- `<name>` is your label for it — `work`, `personal`, `prod`, …

The credential **value is read from stdin** — pipe it in, never pass it on argv (argv is visible in
the process table and your shell history). qfs stores the secret securely and **never prints it
back**. The connection *name* is just metadata (safe to show); the credential itself is write-only
from your shell's perspective.

```sh
# QFS_PASSPHRASE must already be exported (see above).
printf %s "$MAIL_TOKEN"  | qfs connection add mail work
printf %s "$AWS_SECRET"  | qfs connection add s3 prod
printf %s "$GH_TOKEN"    | qfs connection add github personal
```

## Listing connections

```sh
qfs connection list            # all services
qfs connection list mail       # just one service
```

This prints **names and metadata only** — never a secret.

## Choosing the active connection

A service can have several connections (e.g. `work` and `personal` mail). Set which one is active:

```sh
qfs connection use mail work
```

## Removing a connection

```sh
qfs connection remove mail work     # idempotent — fine to run twice
```

## Rotating, revoking, and rekeying

Offboarding and key hygiene are first-class. The new secret (or passphrase) is read from **stdin**,
never argv:

```sh
printf %s "$NEW" | qfs connection rotate mail work   # re-mint the secret in place, clear any revoke
qfs connection revoke mail work                      # mark the connection unresolvable (fails closed)
printf %s "$NEWPASS" | qfs connection rekey          # re-wrap the store's data-key under a new passphrase
```

- **rotate** replaces a connection's secret (the offboarding answer — *replace*, not un-grant) and
  clears any prior revocation. Other connections are untouched.
- **revoke** marks one connection unresolvable: a later bind fails closed and the secret is never
  returned. Other connections keep working.
- **rekey** re-wraps the store's data-key under a new `QFS_PASSPHRASE` (the current one is the old
  one, read from the environment; the new one from stdin). Existing secrets stay decryptable; the
  old passphrase stops unlocking. It is one re-wrap of the data-key, not an N-way re-encryption.

::: tip Where the store lives
Stored credentials are **envelope-encrypted at rest** in qfs's SQLite store: a random data-key
encrypts each secret value, and that data-key is itself wrapped under an argon2id key derived from
`QFS_PASSPHRASE`. The `/sys/connections` admin path shows the registry — driver, connection name,
and `created_at` only; there is structurally no column a secret could ride in.
:::

## Least privilege

qfs is built so a credential is only ever used for the exact plan you commit, and never appears in
output, logs, or a `describe` report. On the **server**, you can go further and attach a
[`POLICY`](/server) that allows only specific verbs on specific paths — so an automation or an AI
agent can act, but only within the bounds you set:

```qfs
create policy api ALLOW select DENY INSERT, update, remove, call
create policy uploads ALLOW UPSERT on 's3/*'
```

The guiding rule: grant the **minimum** a task needs, and let `preview` confirm a plan stays inside
those bounds before you commit.
