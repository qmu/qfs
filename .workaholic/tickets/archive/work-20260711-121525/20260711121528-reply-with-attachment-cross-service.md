---
created_at: 2026-07-11T12:15:28+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: [20260711121525-slack-file-bytes-upload-attach-detach-parity.md, 20260711121526-chatwork-declared-driver-with-file-handling.md]
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Reply with an attached file sourced from Drive / Slack / Chatwork

## Overview

Complete the mission's third file-handling flow: **reply** to an existing conversation with a
file whose bytes come from another service. Three concrete pipes: (a) Gmail thread-reply carrying
a Drive file (thread-reply and reply-with-attachments both shipped — this composes them
cross-service), (b) Slack channel post referencing/attaching a file pulled from Drive or Gmail
(needs the Slack bytes upload from the dependency ticket), (c) Chatwork room reply with a file
(needs the Chatwork declared driver). Each pipe is one statement built on
`materialize_pipeline_source`; the ticket verifies each hermetically and lands the taught recipes.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/email-sending-restraint.md` — every pipe here ends in an outbound send/post; the send gate and irreversible marking stay intact in each recipe
- `workaholic:implementation` / `policies/test.md` — hermetic mocks per pipe; live rounds owner-gated

## Key Files

- `packages/qfs/crates/driver-gmail/src/effect.rs` - Reply effect already accepting attachments: Vec<Attachment>
- `packages/qfs/crates/driver-slack/src/effect.rs` - message post + file upload effects (bytes upload from the dependency ticket)
- `packages/qfs/crates/exec/src/lib.rs` - materialize_pipeline_source, the cross-service composition channel
- `docs/cookbook/` - cross-service cookbook article (gen-skills source)

## Related History

Reply plumbing and the Drive→Gmail attach pipe shipped; this ticket is their cross-product with the two new write surfaces.

- [20260709010930-gmail-thread-reply-support.md](.workaholic/tickets/archive/work-20260708-171710/20260709010930-gmail-thread-reply-support.md) - Gmail thread reply (In-Reply-To/References/threadId)
- [20260709010931-gmail-attach-detach-every-draft-send-form.md](.workaholic/tickets/archive/work-20260708-171710/20260709010931-gmail-attach-detach-every-draft-send-form.md) - attachments on every send/reply form
- [20260701192440-cross-service-drive-to-gmail-attach-and-send.md](.workaholic/tickets/archive/work-20260629-110121/20260701192440-cross-service-drive-to-gmail-attach-and-send.md) - the composition idiom to reuse

## Implementation Steps

1. Write the three target statements (spec by example): Gmail reply + Drive-sourced attachment (ARRAY_AGG(STRUCT) into the reply's attachments column); Slack post + cross-sourced file; Chatwork reply + file. Identify per-pipe gaps at PREVIEW.
2. Fix composition gaps only (projection shapes, reply-target addressing); the write surfaces themselves come from the dependency tickets.
3. One hermetic end-to-end test per pipe asserting the reply threading fields and the attached bytes survive intact.
4. Cookbook recipes for all three, parse-checked; regenerate docs/skills.

## Quality Gate

**Acceptance criteria**

- Each of the three reply-with-attachment statements commits hermetically with correct threading/room addressing and byte-identical attachment content.
- All three remain behind the send gate (no reply leaves without `--commit`; irreversible marking where declared).

**Verification method**

- `cargo test --workspace` green including three new cross-service tests; `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; live rounds (real reply into a self-addressed thread/room) run owner-attended afterwards.

## Considerations

- Slack "reply" semantics differ (thread_ts vs channel post) — pick the thread form and record the ruling (`packages/qfs/crates/driver-slack/src/effect.rs`)
- Chatwork reply markup (`[rp aid=…]`) is a message-body convention, not a header — decide whether the recipe encodes it or posts plain (docs)

## Live Round Evidence

### Round 4 — reply into a real thread carrying a Drive file (2026-07-13, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a). Owner's standing approval covers the round (both sends
  self-addressed only).
- **Thread:** a one-shot `call mail.send` created the self-addressed starter
  (msg `19f593c23715c43b`, thread `19f593c221c016f4`).
- **The one-statement cross-service reply:** `/drive/my/report.pdf |> select
  {filename: name, mime: mime_type, bytes: content} as att |> aggregate array_agg(att) as
  attachments |> extend body = '…' |> insert into /mail/inbox/19f593c23715c43b/replies` —
  previewed (`affected 1`, reversible), committed. The draft read back with the parent's
  `thread_id`, a defaulted `Re:` subject, and the full attachment
  (`report.pdf`, `application/pdf`, 342,628 bytes) — Drive bytes materialized at
  commit, nothing cached.
- **Send + thread read-back:** `call mail.send` on the reply draft (irreversible gate), then the
  inbox thread listed BOTH messages under `thread_id 19f593c221c016f4`, the reply carrying the
  PDF. Reply-with-attachment sourced from Google Drive is live-proven end to end.
- **Ticks acceptance:** the mission's "Reply with an attached file sourced from Google Drive /
  Slack / Chatwork" — Drive is the one source expressible today (Slack threaded file-reply and
  Chatwork file-reply are recorded follow-ups on the PR #33 concern).
- **Residue:** one self-addressed thread (2 messages, one PDF attachment) in the owner's mailbox.
