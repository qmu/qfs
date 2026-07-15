# concerns

Grouped by **home** since the 2026-07-15 missions/tickets reframing. A concern belongs to a mission
only when it is evidence that the mission's property does not hold yet. **Belonging to no mission is
a legitimate state** — most concerns are isolated defects, deliberate cuts, or verification debt, and
forcing each one to claim a parent is what previously left every open concern homed to a mission
that was already finished. Mission-free concerns are picked up as plain tickets, no mission needed.

## Mission: [declared drivers are the normal way to add a service](../missions/active/declared-drivers-are-the-normal-way-to-add-a-service/mission.md)

Adopted 2026-07-15 from the archived capability-tryout mission's unfinished goal #2.

* [/cf live (203090) unimplemented; /cf and /rest are placeholder mounts](cf-live-203090-unimplemented-cf-and.md)
* [CREATE ACCOUNT's SECRET reference form is unimplemented (no bind-time account credential resolution)](create-account-ships-the-core-two.md)
* [Declared-model and scheduling follow-ups](declared-model-and-scheduling-follow-ups.md)
* [Duplicate declaration rows still resolve oldest-first outside the type lookup](duplicate-declaration-rows-still-resolve-oldest.md)
* [Postgres/MySQL declarations for the declared-registry path are partial](postgres-mysql-declarations-for-the-declared.md)
* [Project DB configuration events are not yet in the DDL event log](project-db-configuration-events-are-not.md)
* [The config `--` comment stripper truncates paths containing `--`](the-config-comment-stripper-truncates-paths.md)

## Mission: [an agent is a first-class principal](../missions/active/support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources/mission.md)

_None yet — the mission has no shipped surface to leave residue._

## Mission-free

### Live verification, owner-attended

Debt that accrues **structurally**: everything ships hermetic-first and its live round is
owner-gated, so it is always behind. Considered as a mission on 2026-07-15 and deliberately left
mission-free — it is operational debt, not a product property.

* [170000 Quality Gate #5 — owner live vault-unlock confirmation](170000-quality-gate-5-owner-live.md)
* [Artifacts repo token is sealed but live round-trip is owner-gated](artifacts-repo-token-is-sealed-but.md)
* [Bearer-gated (non-loopback) reconcile round is not live-verified](bearer-gated-non-loopback-reconcile-round.md)
* [Console bundle pin unset; live serve + release stamp pending the plgg bundle](console-bundle-pin-unset-live-serve.md)
* [Live-only providers remain outside local proof](live-only-providers-remain-outside-local.md)
* [Live provider acceptance still needs credentials](live-provider-acceptance-still-needs-credentials.md)
* [Remaining owner-attended live rounds](remaining-owner-attended-live-rounds.md)

### Shell-face residue

Defects in the shell face shipped through v0.0.71 (PR #41). Fixes are clear; no design judgment
pending. Formerly homed to the language mission, which is now archived `achieved`.

* [`cd` into a blob file is still admitted](cd-into-a-blob-file-is.md)
* [Definition-catalog `cp`=clone and `mv`=rename are refused, not implemented](definition-catalog-cp-clone-and-mv.md)
* [`/sys` and `/slack` do not describe their roots, so `cd` there fails before the gate](sys-and-slack-do-not-describe.md)
* [The interactive shell's `/local` reads from the cwd but writes to the filesystem root](the-interactive-shell-s-local-reads.md)
* [The `/type` catalog and the type resolver translate the stored key differently](the-type-catalog-and-the-type.md)

### Isolated defects

* [EXTEND on the read path is now a real operation (behaviour change)](extend-on-the-read-path-is.md)
* [/local write materialization is narrow](local-write-materialization-is-narrow.md)
* [Policy-less or denied job re-fires every sweep](policy-less-or-denied-job-re.md)
* [Redirect off a follow URL is refused by the confined transport](redirect-off-a-follow-url-is.md)
* [Slack workspace-namespace still advertises Verb::Rm with no query grammar](slack-workspace-namespace-still-advertises-verb.md)
* [The `api` policy row gates MCP, dashboard, and reconcile alike](the-api-policy-row-gates-mcp.md)

### Tooling and watch items

* [The branch-safety scanner false-positives on Rust `Token::Variant`, hard-blocking `/ship`](the-branch-safety-scanner-false-positives.md) — **cross-repo**: the fix lives in the workaholic plugin, deliberately not vendored into qfs, so it cannot be closed from here
* [qfs-runtime span-buffer test flakes under parallel workspace tests](qfs-runtime-span-buffer-test-flakes.md)
* [Scope cuts and monitored items](scope-cuts-and-monitored-items.md) — a standing watch list, not a defect

---

* [archive/](archive/)
