---
type: Feedback
title: [FB] Support authenticated Slack attachment downloads through the selected QFS mount
kind: instruction
source: discussion
subject: person:tamurayoshiya
created_at: 2026-09-16T06:57:46+09:00
author: a@qmu.jp
supersedes: 
---

# [FB] Support authenticated Slack attachment downloads through the selected QFS mount

kind: instruction / source: discussion / subject: person

Please support downloading Slack attachments through the already authenticated QFS mount. With QFS 0.0.130, channel message/thread reads and the channel file listing succeed, including PDF metadata, but the installed Slack declarations expose no operation for reading the file bytes. This prevents an agent from inspecting a document supplied in the same conversation and forces the user to provide it again elsewhere. The shipped Slack guide explicitly documents this gap: Slack private file URLs require authentication, while generic FOLLOW deliberately forwards no credentials. Add a discoverable, Slack-scoped authenticated file-read/download operation that reuses the selected mount/account, preserves channel/file access checks, and does not expose credentials or forward them to arbitrary hosts or redirects. Completion should be demonstrated by listing an attached PDF and retrieving its intact bytes using that same mount, with clear errors for inaccessible files. Update describe output and the Slack skill with an executable example. The request is to support the missing file-byte capability, not to weaken generic FOLLOW credential isolation.


Source: https://github.com/qmu/qfs/issues/111
