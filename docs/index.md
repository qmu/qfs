---
layout: home
hero:
  name: qfs
  text: One query language for every service you use
  tagline: Mail, files, databases, GitHub, Slack, git, cloud storage — all addressed like a filesystem and queried with one small pipe-SQL language. A single binary. Nothing happens until you say COMMIT.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: See what it can do →
      link: /showcase
    - theme: alt
      text: Why qfs?
      link: /guide/introduction
features:
  - title: Everything is a path
    details: "/mail/inbox, /sql/pg/orders, /github/acme/web/pulls, /drive/Reports, /s3/bucket/key — every service is a tree of paths you can list, read, and write."
  - title: One language, every backend
    details: A pipe-SQL grammar with FROM … |> WHERE … |> SELECT … |> JOIN. Filter mail, join a database to GitHub, transcode JSON→CSV — the same way, everywhere.
  - title: Preview before you commit
    details: Every command shows you exactly what it will do first. Nothing touches the real world until you add --commit. Irreversible actions (sending mail, merging a PR) need an explicit extra OK.
  - title: Safe for AI agents
    details: One small grammar instead of a hundred vendor SDKs. An agent learns it once and can drive every service — with previews and least-privilege policies as guardrails.
---

## The idea in 30 seconds

Every tool you use — your inbox, your databases, your GitHub repos, your cloud drives — speaks a
different API. qfs gives all of them **one shape**: a filesystem of paths, queried with **one small
SQL-like language**.

```text
# Find unread invoices in your inbox
FROM /mail/inbox |> WHERE subject LIKE '%invoice%' |> SELECT date, from, subject

# Join a database table to your GitHub issues — across two completely different services
FROM /sql/pg/orders |> JOIN /github/acme/web/issues ON id = issue_id |> SELECT id, title

# Turn a JSON file into a YAML file
FROM /local/config.json |> DECODE json |> ENCODE yaml

# Automate it: every time mail lands, post to Slack
CREATE TRIGGER notify ON /mail/inbox DO INSERT INTO /slack/acme/general/messages VALUES (NEW.subject)
```

You **preview** each one to see precisely what would happen, then add `--commit` to make it real.

**Next:** [Install qfs](/guide/installation) · [Your first queries](/guide/getting-started) ·
[The Showcase](/showcase)
