# Round 3 Review — Architect (Coding Phase, Iteration 1 re-review)

- **Reviewer**: Architect (Neutral / Structural)
- **Scope**: Commit `f90099c` — S5b fix: route pre-auth fatal errors through the JSON `{"error":…}` envelope in `-json` mode
- **Files reviewed**: `main.go` (new `exitErr` helper + 3 call-site re-routings), `internal/shell/shell_test.go` (new `TestEncodeErrorJSONPreAuthEnvelope`)
- **Mode**: Analytical review only — no build/test executed
- **Traceability**: plan Amendment 2 ("schedule S5b JSON-envelope fix for Iteration 1")

## Decision: Approve with minor suggestions

The fix is structurally sound, minimal, and faithful to Amendment 2. The defect (pre-auth fatal errors bypassed the JSON envelope and printed a human-readable line even under `-json`) is closed by funneling all fatal exits through a single `exitErr(jsonOut, err)` helper.

## Structural verification

1. **Single-encoder, identical envelope shape — confirmed.** Both the pre-auth path (`exitErr` → `shell.EncodeErrorJSON`) and the post-auth one-shot path now call the same exported `EncodeErrorJSON`, which delegates to the one unexported `encodeErrorJSON` (`internal/shell/output.go:109`). That encoder marshals the single `errorResult{Error string `json:"error"`}` type with `SetEscapeHTML(false)` and newline termination. There is no duplicated formatting: the previous inline `EncodeErrorJSON + os.Exit(1)` block at the `sh.Execute` call site was removed and replaced by the shared helper. Envelope shape is provably identical across both paths because they share the one type and the one encoder.

2. **Call-site routing — confirmed correct.** All three pre/at-auth fatal exits (`auth.Client` failure L71, `gmailpkg.New` failure L83, and the one-shot `sh.Execute` failure L94) now route through `exitErr(*jsonOut, err)`. The remaining bare `fatal(...)` calls are all on genuinely non-JSON paths: `completion zsh` usage (L47), the interactive `sh.Run` exit (L101), and `runLog` (L108–126, which owns its own `-json` branch via `audit.WriteJSON`). Leaving those as `fatal` is correct — they are not `-json` one-shot failure paths.

3. **Non-JSON behavior unchanged — confirmed.** `exitErr` falls through to `fatal(err)` when `jsonOut` is false, preserving the exact `gmail-ftp: <err>` stderr line and `os.Exit(1)`. No human-readable output changed.

4. **Scope / boundary integrity — no regression.** The change touches only `main.go` wiring and adds one test. The OAuth scope, `send`/`label`/`unlabel` deferred stubs, and all `internal/**` command logic are untouched (diff confined to `main.go`, `shell_test.go`, `event-log.md`). No new exported surface beyond reusing the already-exported `EncodeErrorJSON`.

5. **Test fidelity — confirmed.** `TestEncodeErrorJSONPreAuthEnvelope` pins the contract `exitErr` depends on: a credentials-style error serializes to the exact `{"error":"…"}\n` envelope. It exercises the exported entry point (`EncodeErrorJSON`) that main actually calls, complementing the existing unexported-path test. No live credentials involved; the assertion is on serialization only.

## Concern + proposal (Critical Review Policy)

**Concern (translation-fidelity nuance, not a defect):** The error *text* differs subtly between the interactive REPL path and the one-shot/pre-auth path. The REPL applies `friendlyErr(err)` before encoding (`shell.go:566`), whereas the one-shot `Execute` error and the pre-auth errors are encoded raw, with no `friendlyErr` normalization. So a `-json` consumer can observe two different message strings for what may be the same underlying failure depending on whether it ran one-shot vs. interactive. The envelope *shape* is identical (which was the S5b requirement and is satisfied), but the *message contract* is not uniformly normalized. This is pre-existing and out of S5b scope — flagging it for fidelity, not blocking.

**Proposal:** In a follow-up (not this iteration), consider whether the one-shot/pre-auth paths should also pass errors through `friendlyErr` so scripted consumers get one normalized message vocabulary across all `-json` failures. If intentional (raw errors preferred pre-auth for debuggability), add a one-line comment at `exitErr` documenting that the pre-auth/one-shot envelope carries the raw error string by design, so the asymmetry with the REPL path is a recorded decision rather than an accident. The added test could then assert that documented intent.

## Regression verdict

**No regressions.** The change is additive and substitutive on fatal-exit wiring only: non-JSON output is byte-for-byte unchanged, the JSON envelope reuses the single existing encoder/type, no command logic or boundaries moved, and the deferred stubs and OAuth scope are untouched. The new test pins the contract without introducing live-credential dependencies. Traceability to Amendment 2 holds.
