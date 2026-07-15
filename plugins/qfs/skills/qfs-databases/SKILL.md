---
name: qfs-databases
description: Use when a task needs to query or modify a SQL database through qfs — filter, aggregate, join, update, and set operations over /sql/<conn>/<table> relational tables, plus creating and dropping tables by writing to the /sql/<conn> catalog (SQLite, Postgres, MySQL).
---

# Databases

Every table in a connected SQL database becomes a queryable path. A table is a directory of rows,
each row a record, and one pipe-SQL language filters, aggregates, joins, and writes them — the same
verbs you already use on a mailbox, a git repo, or a folder of files.

## Example

**Show me my biggest orders** — every order over $100, richest first:

```qfs
/sql/orders/orders
|> where total > 100
|> select customer, total
|> order by total DESC
|> limit 5
```

```text
customer | total
-------- | -----
carol    | 220
alice    | 150
(2 row(s))
```

That read runs against the live database the instant a connection is configured — qfs pushes the
`WHERE`, `ORDER BY`, and `LIMIT` down into the engine and does the rest locally. Now the **smart**
part — one statement creates-or-replaces a row and previews before it touches anything:

```qfs
upsert into /sql/orders/orders
  values (1, 'alice', 999)
```

```text
PREVIEW: 1 effect(s)
  #0 UPSERT -> sql:/sql/orders/orders [affected 1]
  total affected: 1
```

::: tip Reads run now; writes preview
Every **read** returns rows immediately. Every **write** (`insert`, `update`, `upsert`) *previews*
by default and changes nothing — add `--commit` to apply it. The plan tells you the verb, the
target, and how many rows are affected, so you can safely watch what a recipe *would* do first.
:::

A database isn't reachable until you point a connection at it — one environment variable, in
**[Setup](#setup)**. Local `/sql` connections read the instant they're configured; after that every
recipe on this page works verbatim.

## Setup

::: tip Prerequisite for a connected source
A local file / repo needs no passphrase. A **remote / connected** source stores a login behind your
`QFS_PASSPHRASE` — set it up once in **[The QFS passphrase](/guide/passphrase)**.
:::

You register a database once, by name. The happy path is two lines:

```sh
export QFS_SQL_ORDERS=/path/to/orders.db                                   # 1. name a connection `orders`
qfs run "/sql/orders/orders |> select id, customer, total |> limit 5"      # 2. read a table
```

The rest of this section explains the naming rule and the alternatives.

### 1. Name a connection

A connection is named by an environment variable: `QFS_SQL_<CONN>=<path-or-url>`. The recipes below
use a SQLite file registered as `orders` (`QFS_SQL_ORDERS=/path/to/orders.db`), so its `orders`
table is reachable at `/sql/orders/orders` — the shape is always `/sql/<conn>/<table>`.

### 2. Point it anywhere

Postgres, MySQL, and D1 URLs work exactly the same way — swap the SQLite path for a connection URL
under the same `QFS_SQL_<CONN>` variable. Only the verb support differs: tables get full CRUD, while
views are `SELECT`-only.

### 3. Read a real table

```sh
qfs run "/sql/orders/orders |> select id, customer, total |> limit 5"
```

Real rows come back — the read runs against the live database and returns the actual rows with their
schema (the `--json` envelope carries the typed column list).

## The database as paths

Once a connection is configured, a SQL database is a set of **relational tables** mapped onto a
filesystem shape:

| SQL thing | qfs path | it is a… |
| --------- | -------- | -------- |
| a database connection | `/sql/orders` | directory of tables |
| a table | `/sql/orders/orders` | relational table (rows) |
| a view | `/sql/orders/<view>` | read-only table |

A table (`/sql/<conn>/<table>`) supports `SELECT`, `JOIN`, `INSERT`, `UPDATE`, and `UPSERT`; a view
supports `SELECT` only. The `orders` table below has columns `id`, `customer`, and `total`. Run
`qfs describe /sql/orders/orders` for the exact schema and verbs of any node.

## Read

**Filter, project, sort, limit** — the `WHERE`, `ORDER BY`, and `LIMIT` push into the database:

```qfs
/sql/orders/orders
|> where total > 100
|> select customer, total
|> order by total DESC
|> limit 5
```

```text
customer | total
-------- | -----
carol    | 220
alice    | 150
(2 row(s))
```

**Ranges read naturally:**

```qfs
/sql/orders/orders
|> where total BETWEEN 50 AND 100
|> select id, total
```

```text
id | total
-- | -----
2  | 80
4  | 55
(2 row(s))
```

**Sets read naturally too:**

```qfs
/sql/orders/orders
|> where customer IN ('alice', 'bob')
|> select id, customer
```

```text
id | customer
-- | --------
1  | alice
2  | bob
(2 row(s))
```

**Pattern match with `LIKE`:**

```qfs
/sql/orders/orders
|> where customer LIKE 'a%'
|> select id, customer
```

```text
id | customer
-- | --------
1  | alice
(1 row(s))
```

## Summarize

**Count rows per group:**

```qfs
/sql/orders/orders
|> group by customer
|> aggregate count(id) as n
|> order by n DESC
```

```text
customer | n
-------- | -
alice    | 1
bob      | 1
carol    | 1
dave     | 1
(4 row(s))
```

**Sum a column** — total revenue in one line:

```qfs
/sql/orders/orders
|> aggregate SUM(total) as revenue
```

```text
revenue
-------
505
(1 row(s))
```

## Write

Writes **preview** by default — the plan tells you the verb, the target, and how many rows are
affected, and changes nothing until you `--commit`.

**Insert a row, returning its id:**

```qfs
insert into /sql/orders/orders
  values (5, 'eve', 10)
  returning id
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> sql:/sql/orders/orders [affected 1]
  total affected: 1
```

**Update matching rows** (the count is `?` until committed, since it's resolved inside the database):

```qfs
update /sql/orders/orders
  set total = 0
  where id == 1
```

```text
PREVIEW: 1 effect(s)
  #0 UPDATE -> sql:/sql/orders/orders [affected ?]
  total affected: ?
```

**Upsert — the retry-safe write** (create-or-replace; running it twice converges):

```qfs
upsert into /sql/orders/orders
  values (1, 'alice', 999)
```

```text
PREVIEW: 1 effect(s)
  #0 UPSERT -> sql:/sql/orders/orders [affected 1]
  total affected: 1
```

## Create and manage tables

### Create a database

For SQLite a database is just a file, and **declaring a connection to a new path creates it** — qfs
opens (and so creates) the file the first time it is used. Declaring `shop` at a path that does not
exist yet gives you an empty database named `shop`, ready for tables:

```qfs
CREATE CONNECTION shop DRIVER sqlite AT '/data/shop.db'
```

From here `/sql/shop` is the new database's catalog. (Postgres and MySQL name a database the same
way — swap the driver and give a server URL; there the server, not a file, holds the database.)

### Create and drop tables

A table is a **definition**, and qfs declares definitions with first-class `CREATE` statements —
the same family as `CREATE VIEW` and `CREATE CONNECTION`. The path names the table inside its
database, and the column list declares the schema:

**Create a table:**

```qfs
CREATE TABLE /sql/shop/customers (
  id int PRIMARY KEY,
  email text UNIQUE,
  joined timestamp NOT NULL
)
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> sql:/sql/shop [affected 1]
  total affected: 1
```

Like every definition statement it is pure sugar over an effect plan — it previews first, and the
plan shows the truth: a write against the database's catalog (`/sql/shop`). Once committed the
table is reachable at `/sql/shop/customers` — read it, insert rows, and `describe` it exactly like
any other table.

**List the tables** — the `SHOW TABLES` of qfs is just reading the database:

```qfs
/sql/shop
|> select name, kind
|> order by name
```

```text
name      | kind
--------- | -----
customers | table
orders    | table
```

**Drop a table** — `REMOVE` is the one destructive verb everywhere in qfs, and `REMOVE TABLE` is
its definition-layer form:

```qfs
REMOVE TABLE /sql/shop/customers
```

```text
PREVIEW: 1 effect(s)
  #0 REMOVE -> sql:/sql/shop [affected 1]
  total affected: 1
```

::: warning Dropping a table is irreversible
`REMOVE TABLE` destroys the table and every row in it. Like every destructive write it previews
first and changes nothing until you commit — and because it is *irreversible*, plain `--commit` is
not enough: it needs `--commit --commit-irreversible` to apply, so a table can never be dropped by
accident.
:::

::: tip The catalog is still just data
`DESCRIBE /sql/shop` shows the catalog node's row shape — a `name` plus a `columns` array. The
`CREATE TABLE` statement desugars to an ordinary write of that row (exactly as `CREATE ENDPOINT`
desugars to a `/server` write), so the raw form works too if you ever want to script it:
`insert into /sql/shop values ('t', [ { name: 'id', type: 'int', primary_key: true } ])`.
:::

### Constrain a column with a refined type

A **named type** is a reusable, refined shape: a schema plus an optional `WHERE` predicate that
every row/value must satisfy. Declare one with `CREATE TYPE`, attach it to a table with `OF`, and
qfs checks membership before a row is written:

```qfs
CREATE TYPE email (value text NOT NULL) WHERE value LIKE '%@%'

CREATE TYPE customer (
  id int PRIMARY KEY,
  email email UNIQUE
)

CREATE TABLE /sql/shop/customers OF customer
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> sql:/sql/shop [affected 1]
  total affected: 1
```

Now a conforming write is ordinary SQL DML:

```qfs
INSERT INTO /sql/shop/customers VALUES (1, 'alice@example.com')
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> sql:/sql/shop/customers [affected 1]
  total affected: 1
```

But a value that is not a member of the declared type is refused at commit with a structured error
that names the table column and the predicate:

```qfs
INSERT INTO /sql/shop/customers VALUES (2, 'not-an-email')
```

```text
terminal effect failure: row violates `/type/email` for `/sql/shop/customers` column `email`:
row is not a member of the refined type: it fails the predicate `Like ...`
```

The `WHERE` predicate is a **row-local, pure** boolean over the declared columns — comparisons,
`LIKE`, and scalar functions. It is checked two ways:

- **At declare time** the predicate is validated: it must be boolean and name only declared
  columns, and it may not call an aggregate (`COUNT`), a source (`READ`, `http.get`), or a context
  built-in (`NOW`, `env`) — a refinement is a per-row contract, so a malformed one is refused *at
  `CREATE TYPE`*, never at first write.
- **At the boundary a row is delivered** the predicate is evaluated per row; a value that fails it
  is refused with a structured error naming the predicate, exactly like a `CHECK` constraint (it is
  **not** a solver-proven refinement — it is contract-checked where the rows exist).

`ls /type` lists declared types and `DESCRIBE /type/email` teaches the shape — those `/type` paths
are the catalog/shell face, addressing the catalog as **data**. A type is *defined* and *referenced*
by its bare **name**, never by a path (paths are data, names are definitions): you write `CREATE TYPE
email (…)`, `email email`, and `CREATE TABLE /sql/shop/customers OF customer`; the stored catalog
record canonicalizes those references to `/type/email` and `/type/customer`.

## Combine two tables

Set operations stitch two reads together. `UNION` de-duplicates; `EXCEPT` subtracts the second
read from the first.

**Union — every distinct customer across two reads:**

```qfs
/sql/orders/orders
|> select customer
|> union /sql/orders/orders
|> select customer
```

```text
customer
--------
alice
bob
carol
dave
(4 row(s))
```

**Except — customers without a big order:**

```qfs
/sql/orders/orders
|> select customer
|> except /sql/orders/orders
|> where total > 100
|> select customer
```

```text
customer
--------
bob
dave
(2 row(s))
```

::: tip Want to join a database to another *service*?
That's the fun part — join a table to GitHub, a mailbox, or a file in one query. See
[Cross-service](/cookbook/cross-service).
:::
