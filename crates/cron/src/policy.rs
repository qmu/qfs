//! [`MissedPolicy`] — what to do when the process was down across one or more due times
//! (RFD §6 idempotency / catch-up). Pure due-set folding over a [`Schedule`].

use serde::{Deserialize, Serialize};

use crate::schedule::{Instant, Schedule};

/// A safety cap on the number of boundaries the due-set enumerator materialises before folding —
/// so a JOB that has been down for years (with a `last_run_at` far in the past) cannot allocate an
/// unbounded vector. The policy fold is applied to this bounded enumeration; `Coalesce`/`Skip`
/// collapse it to one anyway, and `CatchUp{max}` is itself capped at `max`.
const MAX_ENUMERATED: usize = 10_000;

/// What to do with the boundaries that fell due while the process was down (RFD §6). Default is
/// [`MissedPolicy::Coalesce`] — one run covering the whole gap — to avoid a thundering catch-up
/// after downtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MissedPolicy {
    /// Only the newest missed window: skip straight to the latest due boundary (one run).
    Skip,
    /// Replay capped windows: the oldest `max` due boundaries, in order.
    CatchUp {
        /// The maximum number of missed boundaries to replay.
        max: u32,
    },
    /// One run covering the whole gap: the newest due boundary only (the default). Distinct from
    /// `Skip` in *intent* (the DO body is expected to cover `(last_run, now]` via `LAST_RUN()`),
    /// though the dispatched boundary set is the same single newest boundary.
    #[default]
    Coalesce,
}

impl MissedPolicy {
    /// Compute the **due set** — the boundaries this tick should dispatch — from the last-run
    /// high-water mark and `now`, folded by this policy.
    ///
    /// `last_run_at = None` means the JOB never ran: the due set is the single next boundary
    /// after the schedule's natural anchor that is `<= now` (i.e. fire once on first eligibility),
    /// or empty if no boundary is yet due.
    ///
    /// The enumeration of boundaries in `(from, now]` is bounded by [`MAX_ENUMERATED`]; `Skip` and
    /// `Coalesce` collapse to the newest, `CatchUp{max}` keeps the oldest `<= max`.
    #[must_use]
    pub fn due_set(
        self,
        schedule: &Schedule,
        last_run_at: Option<Instant>,
        now: Instant,
    ) -> Vec<Instant> {
        // The lower exclusive bound to enumerate boundaries from. On first run there is no
        // high-water mark; we look back from one interval-equivalent before `now` so the JOB
        // fires once at its next natural boundary <= now (without replaying all of history).
        let from = match last_run_at {
            Some(t) => t,
            None => {
                // First run: take the most recent boundary at-or-before `now` as the single due
                // boundary (fire once), if one exists.
                return first_run_due(schedule, now);
            }
        };

        // Enumerate boundaries strictly in (from, now].
        let mut boundaries: Vec<Instant> = Vec::new();
        let mut cursor = from;
        while boundaries.len() < MAX_ENUMERATED {
            match schedule.next_after(cursor) {
                Some(b) if b <= now => {
                    boundaries.push(b);
                    cursor = b;
                }
                _ => break,
            }
        }

        if boundaries.is_empty() {
            return Vec::new();
        }

        match self {
            // Newest only.
            MissedPolicy::Skip | MissedPolicy::Coalesce => {
                vec![*boundaries.last().unwrap_or(&from)]
            }
            // Oldest `max`, in order.
            MissedPolicy::CatchUp { max } => {
                let take = (max as usize).min(boundaries.len());
                boundaries.into_iter().take(take).collect()
            }
        }
    }
}

/// First-run due set: the single most-recent boundary at-or-before `now`, if any. We probe one
/// interval-equivalent before `now` and pick the latest boundary `<= now`.
fn first_run_due(schedule: &Schedule, now: Instant) -> Vec<Instant> {
    // Walk forward from a point safely before `now`. For `Every` the boundary spacing gives a
    // natural look-back; for `Cron` the minute granularity does. We look back a bounded window
    // (one day) and pick the latest boundary <= now.
    const LOOKBACK: Instant = 24 * 3600;
    let start = now - LOOKBACK;
    let mut latest: Option<Instant> = None;
    let mut cursor = start;
    let mut steps = 0;
    while steps < MAX_ENUMERATED {
        match schedule.next_after(cursor) {
            Some(b) if b <= now => {
                latest = Some(b);
                cursor = b;
            }
            _ => break,
        }
        steps += 1;
    }
    match latest {
        Some(b) => vec![b],
        None => Vec::new(),
    }
}
