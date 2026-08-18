---
type: Feedback
title: create.sh refuses source: development, which its own header and the corpus use
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T08:57:34+00:00
author: a@qmu.jp
supersedes: 
---

# create.sh refuses source: development, which its own header and the corpus use

`feedback/scripts/create.sh` refuses `source: development`, and both its own usage header and the records already in this repository say it should not.

The code (v1.0.181, and the marketplace copy alongside it) reads:

```sh
case "$SOURCE" in
    meeting|slack|discussion) : ;;
    *) echo '{"created": false, "reason": "bad_source"}'; exit 1 ;;
esac
```

while the usage block twelve lines above it reads `source:  meeting | slack | discussion | development`, and `.workaholic/feedbacks/20260817184536-bootstrap-installs-the-plugin-but-does.md` — among others already on main — carries `source: development` in its frontmatter.

So three surfaces disagree about one closed vocabulary: the header documents four values, the guard accepts three, and the corpus contains the fourth. A caller following the documented contract gets `bad_source` and no record, which is how this was found — the persist-gap record filed alongside it had to be written under `discussion` and say so in its own body.

The fix is one of two, and which one it is decides what the existing records mean:

- **Add `development` to the guard.** The corpus is then consistent, the header is already right, and the axis keeps the value that names "arose while working, not in a meeting or a channel" — the channel most of this stream actually arrives through, since a routine writes most of it.
- **Drop `development` from the header** and treat the existing records as legacy. Then something must say what those records' `development` now means, and the routines that would naturally reach for it need a documented substitute.

Filed rather than fixed: the vocabulary is a schema decision, not a typo, and `feedback/reference/schema.md` is where the axis is defined.
