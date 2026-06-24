# Why qfs?

## The problem

Every service you touch has its own API, its own client library, its own auth dance, its own way
of saying "list the things", "find the matching ones", "change this one". Gmail is not Postgres is
not GitHub is not S3. To automate across them you glue together a pile of SDKs, each with its own
concepts and failure modes.

For a person that's tedious. For an **AI agent** it's worse: it has to learn N different toolkits,
and every one is a new chance to do something destructive by accident.

## The qfs answer

qfs collapses all of that into **two ideas**:

1. **Every service is a filesystem of paths.** Your inbox is `/mail/inbox`. A database table is
   `/sql/pg/orders`. A GitHub repo's pull requests are `/github/acme/web/pulls`. A Drive folder is
   `/drive/Reports`. You navigate and address everything the same way.

2. **You query and change paths with one small language.** It looks like SQL with pipes:

   ```text
   FROM /mail/inbox |> WHERE subject LIKE '%invoice%' |> SELECT date, from, subject
   ```

   The same `FROM … |> WHERE … |> SELECT …` shape works on mail, on a database, on GitHub, on
   files. Learn it once; use it everywhere.

That's it. One mental model for everything.

## What makes it powerful

- **It joins across services.** Because everything is the same kind of thing, you can `JOIN` a
  database table to a GitHub issue list, or a CSV file to a SQL table. Different services, one
  query. (See the [Showcase](/showcase).)
- **It transforms data inline.** Built-in codecs read and write JSON, JSONL, YAML, TOML, CSV, and
  Markdown, so `DECODE`/`ENCODE` turns one format into another in a single line.
- **It automates itself.** The same language has a server side: `CREATE TRIGGER`, `CREATE JOB`,
  `CREATE ENDPOINT` turn a query into an event handler, a scheduled task, or an HTTP API.
- **It's one binary.** `qfs` runs as a CLI on your laptop, an interactive shell, or a long-running
  server. No runtime, no services to stand up.

## Built to be safe — for you and for AI

qfs was designed so that an AI agent can drive every one of your services without you holding your
breath:

- **Preview by default.** Every command shows the exact plan — what it touches, how many rows,
  whether it's reversible — *before* anything happens. You opt in to real changes with `--commit`.
- **Irreversible actions are gated.** Sending an email, merging a PR, deleting a file can't be
  undone, so qfs flags them and requires an explicit extra acknowledgement.
- **Least privilege.** Credentials are stored per service and never printed. On the server you can
  write a `POLICY` that allows only specific verbs on specific paths.
- **One grammar to audit.** An agent that knows qfs knows everything. There's a single, small
  surface to reason about instead of a hundred SDKs.

## Who is this for?

- **People** who want one consistent way to query and automate across the services they already
  use.
- **AI agents** that need to operate real systems safely — one language to learn, previews and
  policies as guardrails.
- **Teams** who want automations (triggers, scheduled jobs, small HTTP endpoints) defined as plain,
  reviewable queries.

Ready? [Install qfs](/guide/installation), then run [your first queries](/guide/getting-started).
