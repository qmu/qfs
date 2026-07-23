# The module system: how qfs is extended

qfs is a single binary, but the set of services it can reach is not fixed at compile time.
This page explains the architecture that makes that true: what a driver is, why some drivers
are written in Rust and others are written in the query language itself, where a service's
configuration lives, and how a change to that configuration is applied and made durable. If you
have read the blueprint, this is the reader-facing companion to sections 13 and 16.

## One principle: configuration is data

Everything that follows is a consequence of a single decision. qfs made its entire configuration
into data — ordinary rows in ordinary registries, addressed by path like everything else in the
system. A server endpoint is a row. A scheduled job is a row. A third-party driver is a set of
rows. Because configuration is data, adding a capability is never a special act of installation
with its own machinery; it is a write, and it travels through the same describe → write →
preview → commit loop that every other change in qfs travels through. Once you internalise that
one idea, the rest of the module system stops looking like several unrelated features and starts
looking like one.

## Where configuration lives: two stores and a vault

Configuration is not kept in one place, because two different kinds of configuration have two
different lifecycles. Understanding the split is the key to understanding everything else.

The first store is the **System DB**, a local SQLite database addressed under `/sys`. This is
where declared drivers live (`/sys/drivers`), alongside defined-path connections, account
consents, settings, and system-scoped policies. Everything in the System DB is persisted
directly and unconditionally: it is on disk the moment you commit it, and it is still there after
a restart, whether or not any server is running. A declared driver is System DB data, which is
why — as we will see — you can install and use one without ever starting a daemon.

The second store is the **server state**, addressed under `/server`. This is where the hosted,
automation-facing configuration lives: HTTP endpoints, scheduled jobs, triggers, cached and
materialized views, server policies, and webhooks. Unlike the System DB, server state is held in
memory inside the running daemon, because those bindings are live things — a route is mounted, a
watcher is attached, a schedule is armed. Server state is still durable, but it is made durable
differently, by a mechanism described later in this page: after every committed change the daemon
re-writes its boot configuration file, and that file is what replays the state on the next start.

A third component, the **Project DB**, is the vault proper. It holds secret material and the
path-binding registry that records your connections. It is mentioned here only to be set aside:
no script and no configuration document ever carries a secret value, so the vault stays out of
the configuration surface entirely. Credentials are always referenced, never embedded.

## Two kinds of driver, and why both exist

A driver is the thing that turns a mount path such as `/mail` or `/cloudflare` into real reads
and writes against a service. qfs has two kinds, and the difference between them is the heart of
the module system.

A **compiled driver** is Rust code, built into the binary. `/mail`, `/drive`, `/github`,
`/slack`, `/sql`, `/git`, `/cf`, `/claude`, and the local `/local` and `/sys` surfaces are all
compiled. A compiled driver is always present and cannot be added, changed, or removed without
rebuilding and reshipping the binary. Compiled drivers are fast and they can express anything Rust
can, but every one of them is a piece of the product's own source code — which does not scale to
the hundreds of thousands of web services a user might want to reach.

A **declared driver** is not code at all. It is a set of rows in `/sys/drivers`, produced by
running a short script written in qfs's own query language. `/cloudflare` and `/chatwork` are
declared. A declared driver needs no Rust, ships no plugin binary, and is added, revised, or
retired by editing data. The design intent is unambiguous: a declared driver is the *normal* way
to add a service, and the compiled set is meant to shrink over time toward a small core of
primitives — the wire transport, the codecs, the secret store, and the OAuth machinery — rather
than growing a new Rust crate per service.

The reason both kinds exist at once is not indecision; it is a deliberate, honest transition,
which the next two sections explain from opposite ends — first what a declared driver is made of,
then how the compiled set is retired into declared ones.

## What a declared driver actually is

A declared driver is assembled from a handful of statements whose nouns are ordinary identifiers,
not new keywords. `CREATE DRIVER` names the service and its base URL and states *how* it
authenticates — bearer token, a named header, an OAuth2 flow, or a reference to an existing
account provider — but it never carries the credential itself, which lives in the account layer
and the vault. `CREATE TYPE` declares the stable outward shape of a resource, the contract that a
reader can rely on. `CREATE VIEW` maps a mount path to a read expressed as a pipeline over the
wire, and `CREATE MAP` maps a write or a call on a mount path to a wire effect. Running these
statements is an ordinary previewed, audited, credential-free local write; the effects are simply
new `/sys/drivers` rows.

Two properties make this safe enough to accept a script an LLM generated. The first is that every
declared driver is built on **one wire primitive**: its pipelines can only address its own
declared host, a rule the evaluator enforces structurally at install time and again when the read
plan is built. A declared driver is therefore incapable, by construction, of reading one service
and posting the result to another — it physically has no resolver for any mount but its own. The
second is that scripts are **credential-free by construction**: no clause in the grammar can hold
a secret value, so a declaration can be committed to a repository and reviewed as plain data
without leaking anything.

Reading a declared mount is not a shortcut around the engine; it runs the view's stored pipeline
through the real planner, with the driver's confined wire transport as the only resolvable source.
That single rule is how the awkward shapes of real services are absorbed without inventing a
descriptor for each quirk: an envelope is unwrapped with the ordinary expand operator, a service's
own endpoint naming is decoupled from the mount path because the body names the wire, and the
declared type is enforced against the delivered rows the same way qfs reconciles any type. That
last check — declared contract versus what the live service actually returns — is **conformance**,
and it is precisely the acceptance test a person or an agent runs after generating a script.

## The ratchet: how the compiled set shrinks

This is the part that most often reads as inconsistency, so it is worth stating plainly. The fact
that only Cloudflare and Chatwork are declared today, while Gmail and GitHub and the rest remain
compiled, is not a gap in the design. It is the design working as intended, mid-transition.

The rule is a ratchet. A compiled driver stays exactly where it is until a declared script twin of
it passes the conformance bar — until the twin's reads are row-equivalent to the compiled driver's
on the same fixtures. Only then is the compiled driver eligible to be deleted. Rewrites are not the
migration path; the twin-and-retire ratchet is. Cloudflare and Chatwork are simply the services
whose twins have already been written and proven, so they lead. The others are still compiled
because their twins have not yet cleared the bar, and in some cases because the shapes they need —
Gmail's multipart uploads, MIME assembly, and push channels, or GraphQL and websocket transports —
are honestly named as not yet expressible in the declared surface.

The coexistence of `/cf` and `/cloudflare` is the same story made concrete. The declared
`/cloudflare` now serves Cloudflare's relational D1 surface, KV, and queue pushes, because plain
declared REST can express them. The compiled `/cf` remains as a minimal fallback for exactly the
two things declared REST cannot yet express — pulling from a queue, which is a POST that reads, and
the Artifacts git-repository surface. When a name would collide, the compiled driver wins and the
shadowed declared one is reported rather than silently dropped, which is why the declared surface
mounts under the distinct name `/cloudflare` while the transition is underway. The direction of
travel is constant: capability moves from compiled to declared as each twin passes, and the
compiled core keeps shrinking toward primitives.

## Applying configuration: the reconcile loop

Because configuration is data, a change to it is not applied by re-running an installer. It is
applied by converging the live configuration toward a document that describes the desired state —
Terraform's shape, but with none of Terraform's apparatus. There is no separate state file,
because the configuration store *is* the state. There are no provider plugins, because the driver
model is already the extension point. There is no second configuration language, because the
definition layer of the query language is the language.

The source of truth is a canonical `.qfs` document. `qfs dump` produces it by emitting the whole
of your configuration — server bindings, declared drivers, connections, settings, path bindings —
as a normalized, deterministic list of `CREATE` statements, so that two dumps of the same state are
byte-for-byte identical. You edit that document and hand it back as the authoritative desired
state. `qfs plan <file.qfs>` then reads the document, reads the live current state, and computes
the difference without writing anything; its exit code distinguishes "no changes" from "changes
pending" so that an agent can gate on it. `qfs apply <file.qfs>` recomputes the same difference
against live state and commits it.

Drift is decided as set difference, per collection, and aimed at the machine's own configuration.
A row in the document but not in the live state is an addition; a row in the live state but absent
from the document is a removal, full stop; a row present in both but canonically unequal is a
change. The word canonically matters: equality is decided on a normalized, parsed form of each
row, never on its source text, so reformatting a body, rewrapping a line, or refreshing a
materialized view's cache is not drift. Cosmetic difference is not difference. This is the same
"redefinition, not migration" stance the rest of qfs takes — there is no migration subsystem and
no deprecation period, only a desired document that redefines the configuration and a machine that
converges to it. Because a removal is inherently irreversible, a plan that contains one requires an
explicit acknowledgement to apply, and because applying a document fetched against an older state
could silently revert someone else's concurrent change, `apply` refuses when it detects that the
base has moved unless you explicitly allow it. The cheap, correct response to a moved base is to
re-fetch, not to override.

## Where changes are applied, and whether they persist

The last question is the most practical one: when you apply a `.qfs` document, does the change
reach a running server, and does it survive a restart? The answer differs by store, and the
difference is the whole point.

The `/sys` half — declared drivers, connections, settings — is read and written directly against
the local System DB, exactly as backups have always been. It is persistent the instant it commits
and it does not depend on any server being up. This is why installing a declared driver and then
querying it works from a plain one-shot command with no daemon in sight.

The `/server` half is applied *through* the running daemon rather than around it. `apply` submits
the server bindings as ordinary statements to the daemon's public statement face — the same
executor that serves the dashboard and the agent protocol — and the reconcile runs inside the
daemon process, where the bindings actually live. Routes mount and unmount, watchers attach and
detach, hot, with no restart of the server required. If no daemon is running, `plan` and `apply`
still handle the whole `/sys` half and report the `/server` half honestly as "host not serving,"
so an unreachable daemon is never mistaken for an empty configuration that would plan the entire
document as new additions.

Persistence of the `/server` half is handled by a mechanism that makes "the configuration store is
the state" literally true. After every committed server change, the daemon re-emits its
post-commit state as a canonical `.qfs` document at its boot configuration path, written atomically.
That boot file is the at-rest form of the running state, and it is what the daemon replays when it
next starts. A hot reconfiguration no longer dies with the process; it is captured the moment it
commits. So the answer to "is the applied configuration persistent" is yes for both stores — the
System DB persists directly, and the server state persists by re-emitting the boot file it replays.
