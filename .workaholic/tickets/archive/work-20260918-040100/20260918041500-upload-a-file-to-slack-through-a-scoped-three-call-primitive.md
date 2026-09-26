---
created_at: 2026-09-18T04:15:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback:
merge_policy:
verification_handoff: 
---

# Upload a file to Slack through a scoped three-call primitive

## Overview

Slack is the one connected service qfs can read files from and not write files to. The shipped
declaration says so in a comment, and the cookbook repeats it under **What the file surface does
not do**:

> Slack's current upload is a three-call external flow (reserve an upload URL, PUT the bytes
> out-of-band, then complete the share); `files.upload` is retired for new apps. A declared map is
> **one** request, so the flow cannot be written as one.

That reasoning is sound about *declared maps* and wrong as a conclusion about *qfs*: the download
side has exactly the same problem — `files.info` then a private GET on another host — and it is
solved, by the scoped transport primitive `qfs.file-content`. The upload needs the same treatment
and does not have it, so an operator who can read a Slack attachment still cannot put one back.

Reported by the operator on 2026-09-18 as the practical complaint: reading and writing Slack
attachments is what they want, and only one direction exists.

## Policies

- `workaholic:design` — a service is either reachable through the language or it is not; a
  half-present surface is the expensive kind of gap
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:safety` — the credential goes to `slack.com/api` only; the delivered upload URL is
  validated before any byte leaves the machine, and redirects are refused

## Key Files

- `packages/qfs/crates/driver-http/src/applier/slack_file.rs` — the template. `slack_file_content`
  does `files.info` → validate the returned URL → private GET, with `validate_download_url` and
  `require_success` as the confinement and status discipline. The upload is its mirror image.
- `packages/qfs/crates/driver-http/src/applier.rs` — `apply_effect` (line 85) is where a scoped
  primitive intercepts before `build_request`; `resource_path_of` / `resource_segment_of` give the
  resource key. The read side intercepts in `read_facets.rs` by the same convention.
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — the declaration. The `REMOVE`
  map at the end is the whole file-write surface today; its comment carries the retired claim.
- `packages/qfs/crates/driver-http/src/multipart.rs` — the generic `ENCODE multipart` upload
  primitive Chatwork uses. Read it before choosing a carrier for the bytes (see Considerations).
- `docs/cookbook/slack.md` — the file section and the "does not do" list.

## Implementation Steps

1. Add `applier/slack_upload.rs` beside `slack_file.rs`: one `slack_file_upload` entry point that
   runs Slack's three calls with the mount's own account —
   `files.getUploadURLExternal?filename=&length=` → POST the bytes to the returned `upload_url`
   → `files.completeUploadExternal` with the returned file id, the destination channel and the
   optional initial comment. Every failure is an `HttpError::Application` with its own code and
   hint, in the vocabulary `slack_file.rs` already established.
2. Confine it exactly as the download is confined: refuse a non-`https://slack.com/api` base or a
   non-bearer mount up front; validate the delivered `upload_url` (https, `files.slack.com`, no
   userinfo, no fragment, no control characters) before sending bytes; refuse every redirect.
3. Intercept the resource `qfs.file-upload` in `apply_effect`, exactly as the read path intercepts
   `qfs.file-content`.
4. Declare the map:
   `CREATE MAP UPSERT /slack/{ws}/{channel}/files/{filename}` → `/http/slack/qfs.file-upload`,
   binding `{channel}` through the existing `slack/cid` lookup so a channel NAME works as it does
   for every CALL map.
5. Document it in the cookbook: the round trip (read a file's `content`, upsert it into another
   channel's `files/<name>`), the `files:write` scope, and the memory cost stated in
   Considerations. Regenerate the skills.
6. Bump the patch, and all four plugin `version` fields (minor — the taught surface grows).

## Quality Gate

**Acceptance criteria**

- `upsert into /slack/<ws>/<channel>/files/<name>` with a `content: bytes` row uploads that file
  and shares it into the named channel, and the three calls are the only requests issued.
- A channel NAME resolves through `slack/cid` exactly as it does for `slack.react`.
- A delivered upload URL that is not an `https://files.slack.com/…` URL is refused before any
  byte is sent, and no redirect is followed on either leg.
- No credential reaches any host but `slack.com/api` and the validated `files.slack.com` URL.
- The cookbook no longer lists uploading under "what the file surface does not do".

**Verification method**

- `cd packages/qfs && cargo test --workspace` — including `slack_upload.rs` unit tests over the
  mock client: the happy three-call sequence, a refused upload URL, a redirect, a `missing_scope`
  on each leg, and a byteless row.
- `cd packages/qfs && cargo clippy --workspace --all-targets -- -D warnings`
- `cd packages/qfs && cargo run -p xtask -- gen-skills --check`
- Live, on the operator's own workspace: read one attachment and upsert it back into a channel
  the account is in.

## Considerations

- **How the bytes reach the primitive, and what it costs.** The declared write path encodes a
  map body with `value_to_json`, where `Value::Bytes` becomes a JSON array of numbers — so a
  10 MB attachment is roughly 40 MB of JSON in flight. That is the shipped behavior of every
  declared write today, and using it keeps this change to one primitive with no new language
  surface. The cost is real and is stated in the docs: for very large files (the workspace this
  was reported from has a 94 MB video) prefer the Slack UI until a raw-bytes wire encoding exists.
  A follow-up ticket should add `ENCODE bytes` — a `Value::Bytes` body posted verbatim as
  `application/octet-stream` — which this primitive can then consume unchanged.
- **Why not a declared three-view pipeline.** `FOLLOW` is a credential-free GET on a delivered
  URL; there is no declared form for "POST these bytes to a delivered URL, then POST again with
  the id it returns". Inventing one for a single caller would be a larger and less safe change
  than the primitive the download side already established as the pattern.
- **`files.upload` is retired** and must not be used as a shortcut: new apps are refused by Slack.
