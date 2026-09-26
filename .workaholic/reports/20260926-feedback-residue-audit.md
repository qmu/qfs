# Feedback #128 / #131 reconciliation — 2026-09-26

## Preserved branch inventory

All seven branches remain intact. No branch was blindly merged or deleted.

| Branch | Evidence | Resolution |
| --- | --- | --- |
| work-20260818-200743 | PR #92, d0e49cc: tree identical to branch tip | Landed |
| work-20260916-071056 | PR #116, 6dd72ab: tree identical | Landed |
| work-20260916-073743 | PR #118, 3e12f31: tree identical | Landed |
| work-20260916-081339 | PR #119, ba75eb1: tree identical | Landed |
| work-20260916-083806 | PR #120, 0b44ca2: tree identical | Landed |
| work-20260916-091802 | PR #121, cc2f6e1: remaining tree differences are the integrated PR #116 diagnostics, validation, tests and documentation; attachment implementation matches | Landed |
| work-20260817-023958 | PR #65 explicitly closed as superseded by #46 | Superseded |

The supersession decision is recorded at
https://github.com/qmu/qfs/pull/65#issuecomment-5333747490 . Main implements the declared
describe surface through `declared_describe_mount_with_types` with column and zero-network
tests. The missing `declared_surface.rs` path is an alternative implementation, not a lost
feature. The five salvaged Chatwork fields (`sticky`, `icon_path`, `mytask_num`, `file_num`,
`task_num`) are present in the shipped declaration; ticket 20260819061500 is archived.
The branch-only removal ticket's purpose was delivered by PR #92 under ticket 20260729163000.

Consequently, #131's historical premise that seven branches uniquely hold undelivered
product changes does not describe current main. Claim retirement and squash/supersession
recognition belong to the external workaholic implementation. This qfs change neither
modifies that external repository nor claims its lifecycle repair is complete.

## Slack destination and remaining verification

The existing binding ticket's 2026-09-18 correction confirms private channel `dev-qfs`,
`C0BM2ASB63G`, through mount `/slack-cc-for-qmu`, account `cc-for-qmu` in workspace `qmu`.
This corrects earlier public-only discovery evidence. The declaration is now prepared in
CLAUDE.md; it does not choose a new audience and does not authorize posts.

Validation: `read-declared-binding.sh --root .` returned `ok:true`, `declared:true`,
`missing:[]`, `conflicts:[]`, `unknown_keys:[]`, `invalid:[]`, `errors:[]`.
`complete:false` is intentional because no verified sender_id is available.

The old global/debug/release qfs binaries were 0.0.7 and could not read `/sys/connections`.
The macOS environment also lacked `timeout`, used by the transport wrapper. With a new
0.0.141 binary and local timeout/sha256sum adapters, discovery executes and returns
`no_connection`. This is an absent local connection, not proof the channel is absent.
No account was changed and no Slack message or email was sent.

To complete the live gate, restore the existing selected account connection, verify its
identity and channel, and explicitly authorize the one verification post and read-back.
Pending notifications must remain held until that succeeds.

## Separate Cloudflare handoff

GitHub's repository-secret name listing now succeeds and includes CLOUDFLARE_ACCOUNT_ID
and CLOUDFLARE_API_TOKEN. No secret values were retrieved. Local Cloudflare token environment
variables are unset; token issuance/revocation privileges remain unverified. PR #103's
credential-dependent token narrowing therefore remains separate outstanding work.
