---
type: Feedback
title: A SIGINT shutdown e2e test flakes and reddens main
kind: instruction
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-08-18T13:19:46+00:00
author: a@qmu.jp
supersedes: 
---

# A SIGINT shutdown e2e test flakes and reddens main

Source: https://github.com/qmu/qfs/issues/81

`serve_boots_mixed_fixture_and_drains_audit_on_sigint`
(`packages/qfs/crates/cmd/tests/e2e_binding_ddl.rs:696`) flakes: at the merge of PR #74 the
merge commit's own run on `main`
(https://github.com/qmu/qfs/actions/runs/32139568841) failed with

```
clean shutdown on SIGINT must exit 0, got ExitStatus(unix_wait_status(2))
```

while every other job in the run was green and the test binary reported
`FAILED. 21 passed; 1 failed`. The next push to `main` three minutes later
(https://github.com/qmu/qfs/actions/runs/32139615576) — the same tree plus three feedback
files — ran `build + test (native)` to success, and the same test passed twice on the
branch immediately before the merge.

The reporter's reading: exit status 2 is what the process reports when the SIGINT is not
handled by the clean-shutdown path, which reads like the signal arriving before the
handler is installed.

The ask: stabilise it — either make the test wait until the server is demonstrably ready
to handle the signal before sending it, or, if that ordering cannot be made deterministic
from outside the process, decide explicitly what the test should assert instead. What
should not stand is a merge to `main` being red at random.
