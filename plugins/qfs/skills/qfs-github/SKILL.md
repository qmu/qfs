---
name: qfs-github
description: Use when a task needs GitHub through qfs — list and filter pull requests and issues over /github, and merge a PR with a CALL procedure behind the irreversible gate. Covers connecting a GitHub account.
---

# GitHub

Every pull request, issue, review, and release in a repo becomes a queryable path. One pipe-SQL
language filters your open PRs, reports throughput, tags stale issues, and merges a PR — the same
verbs you already use on a mailbox, a database, or a folder of files.

## Example

**Show me the open pull requests, newest first** — the ones actually waiting on someone:

```qfs
/github/acme/web/pulls
|> where state == 'open'
|> select number, title
|> order by number DESC
|> limit 10
```

```text
number  title
128     Cache invalidation for the search index
127     Bump axum to 0.7
125     Fix flaky auth integration test
… 10 rows
```

That read runs the instant you connect an account. Now the **decisive** part — one statement
squash-merges a PR, and previews the one-way door before it touches anything:

```qfs
/github/acme/web/pulls/42
|> call github.merge(method => 'squash')
```

```text
PREVIEW: 1 effect(s)
  #0 CALL github.merge -> github:/github/acme/web/pulls/42 [affected 1] (!)
  (!) irreversible: 1 node(s) [#0]
  total affected: 1
```

The `(!)` marks the irreversible gate: a merge can't be undone.

::: tip Reads run now; writes preview
Every **read** returns rows immediately. Every **write** (`update`, `insert`, `call`) *previews* by
default and changes nothing — add `--commit` to apply it, `--commit-irreversible` for the ones that
can't be undone (merging a PR, dispatching a workflow run). Paste any recipe below and safely watch
what it *would* do first.
:::

GitHub isn't reachable until you connect an account to it — one command, once, in
**[Setup](#setup)**. After that every recipe on this page works verbatim.

## Setup

::: tip Prerequisites — an operator, an account, a mount
Reaching a cloud service takes three one-time steps: a signed-in operator (`qfs init` —
**[The operator identity](/guide/operator)**), an authorized account (`qfs account add …`), and a
mount binding that account to a path (`qfs connect …`). The happy path below is exactly those
three.
:::

qfs pre-mounts nothing for GitHub. A read (and the `CALL` that targets a PR) needs a token-backed
account bound to a mount:

```sh
qfs init you@example.com                                # 1. the operator + the vault (once per machine)
printf '%s' "$GITHUB_TOKEN" | qfs account add github work   # 2. the token, labeled `work`
qfs connect /github --driver github --account work          # 3. mount it at /github
```

The token comes in on **stdin**, never argv, and is sealed in qfs's encrypted credential store; the
label defaults to `default` if you omit it. Until the mount is bound, a read fails with an
actionable hint naming the `qfs account add github …` / `qfs connect …` to run. Once bound, every
`/github/<owner>/<repo>/…` path resolves against the GitHub API. `qfs account list` and
`qfs connect --list` show the account and the mount.

## The repo as paths

Once connected, a repo is `/github/<owner>/<repo>` and its collections hang off it as directories of
rows:

| GitHub thing | qfs path | it is a… |
| ------------ | -------- | -------- |
| a repo | `/github/acme/web` | directory of collections |
| pull requests | `/github/acme/web/pulls` | directory of PRs |
| one pull request | `/github/acme/web/pulls/42` | file (the `CALL` target) |
| a PR's reviews | `/github/acme/web/pulls/42/reviews` | directory of reviews |
| issues | `/github/acme/web/issues` | directory of issues |
| an issue's comments | `/github/acme/web/issues/87/comments` | directory of comments |
| releases | `/github/acme/web/releases` | directory of releases |
| branches | `/github/acme/web/branches` | directory of branch refs |

The columns are exactly these — nothing else is queryable, and a `where` on a name that is not on
this list is **refused** (a structured `unknown_column` at a non-zero exit), not answered with zero
rows:

| collection | columns |
| ---------- | ------- |
| `pulls` | `number`, `title`, `body`, `state`, `user`, `head_ref`, `head_sha`, `base_ref`, `merged`, `created_at` |
| `issues` | `number`, `title`, `body`, `state`, `user`, `assignees`, `labels`, `created_at`, `updated_at` |
| `comments` | `id`, `user`, `body`, `created_at` |
| `reviews` | `id`, `user`, `state`, `body` |
| `releases` | `id`, `tag_name`, `name`, `body`, `draft`, `prerelease`, `created_at` |
| `branches` | `name`, `sha`, `protected` |

`user` is the author login (there is no `author` column); `assignees` and `labels` are **text
arrays** you `expand` to get one row per element. Run `qfs describe /github/acme/web/pulls` for the
exact schema and verbs of any node.

## List & filter pull requests

**Open pull requests authored by the platform team**, oldest first — the review queue for a group:

```qfs
/github/acme/web/pulls
|> where state == 'open'
     AND user IN ('rin', 'kenji', 'sora', 'mei')
|> select number, title, user, created_at
|> order by created_at ASC
```

**Who has already reviewed PR 42, and what they said** — reviews are a sub-collection of the PR
itself, so the PR number is part of the path:

```qfs
/github/acme/web/pulls/42/reviews
|> where state == 'CHANGES_REQUESTED'
|> select id, user, body
|> order by id ASC
```

**Which branch each open PR targets** — `head_ref`/`base_ref` are plain columns:

```qfs
/github/acme/web/pulls
|> where state == 'open' AND base_ref == 'main'
|> select number, title, head_ref, head_sha
|> order by number DESC
```

## List & filter issues

**Read the open issues** — number, assignees, and labels, lowest number first:

```qfs
/github/acme/web/issues
|> where state == 'open'
|> select number, title, assignees, labels
|> order by number ASC
```

`assignees` is a text array. `expand` explodes it into **one row per assignee**, so a per-person
queue is one stage away:

```qfs
/github/acme/web/issues
|> where state == 'open'
|> expand assignees
|> select number, title, assignees
|> order by number ASC
```

## Report

`GROUP BY` then `AGGREGATE … AS …` rolls raw rows into a report — the same two-stage shape you use on
any other source.

**PR throughput** — count merged pull requests per author since a date. `merged` is a boolean
column; there is no `merged_at`, so bound the window with `created_at`:

```qfs
/github/acme/web/pulls
|> where merged == true AND created_at >= '2026-03-28'
|> group by user
|> aggregate count(number) as merged_prs
|> order by merged_prs DESC
```

**Issue load per label** — `labels` is an array, so `expand` it into one row per label first, then
group:

```qfs
/github/acme/web/issues
|> where state == 'open'
|> expand labels
|> group by labels
|> aggregate count(number) as open_issues
|> order by open_issues DESC
```

## Update issues — reversible

Field updates on issues preview like any write and only apply on `--commit`; the preview lists the
affected issue numbers first.

**Tag every stale open issue** so a backlog-grooming bot picks it up. The writable issue fields are
`state`, `title`, `body`, and `labels` (a label set that **replaces** the existing one):

```qfs
/github/acme/web/issues
|> where state == 'open' AND updated_at < '2026-03-26'
|> update set labels = 'stale'
```

## Comment — reversible

A comment is an `INSERT` into an issue's (or PR's) `comments` sub-collection, so the number it
attaches to is part of the **path**, and the only field is `body`:

```qfs
insert into /github/acme/web/issues/87/comments
  values (body)
         ('CI is red on this PR - please take a look.')
```

It previews like any write and posts on `--commit`. A POST is **at-least-once**: if the request
times out the driver never silently retries, because the comment may already have landed.

## Merge & review — the `CALL` procedures

`/github` declares exactly three procedures: `merge`, `dispatch`, and `review`. `merge` and
`dispatch` are irreversible; `review` is not (a later review supersedes it).

| procedure | params | irreversible |
| --------- | ------ | ------------ |
| `github.merge` | `method`, `sha` | yes |
| `github.dispatch` | `workflow`, `ref`, `inputs` | yes |
| `github.review` | `event`, `body` | no |

**Squash-merge one PR** — `merge` takes the PR number from the **path**, not from an argument, and
`github.merge` is a one-way door:

```qfs
/github/acme/web/pulls/42
|> call github.merge(method => 'squash')
```

**Merge only if nobody pushed since you looked** — pass the `head_sha` you read as the `sha`
precondition, and GitHub refuses the merge if the branch moved:

```qfs
/github/acme/web/pulls/42
|> call github.merge(method => 'squash', sha => 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678')
```

To bulk-merge, read the candidates first and then merge each by its own path — a `CALL` addressed at
a collection has no PR number to act on:

```qfs
/github/acme/web/pulls
|> where user == 'dependabot[bot]' AND state == 'open'
|> select number, title, head_sha
```

**Request changes on a PR** — a submitted review, reversible in the sense that a later review
supersedes it:

```qfs
/github/acme/web/pulls/87
|> call github.review(event => 'REQUEST_CHANGES', body => 'CI is red on this PR - please take a look.')
```

::: warning Irreversible
`CALL github.merge` and `CALL github.dispatch` can't be undone. In a one-shot each needs
`--commit --commit-irreversible`; the `(!)` in the `PREVIEW` marks the gate. Reads, comments,
`github.review`, and the preview of a merge run with no extra flags once the account is connected.
:::
