//! The host's in-memory member registry. The host is the only state; nothing is persisted.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::frame::Report;

/// A member lapses to `offline` after this many missed heartbeats.
pub const MISSED_HEARTBEATS: u32 = 3;

#[derive(Debug, Clone)]
struct Entry {
    addr: String,
    heartbeat: Duration,
    report: Option<Report>,
    last_seen: Instant,
    last_seen_epoch: u64,
}

/// A `/cluster/members` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberRow {
    /// Member name.
    pub name: String,
    /// Remote address the member connected from.
    pub addr: String,
    /// Reported hostname.
    pub hostname: Option<String>,
    /// CPU usage, percent.
    pub cpu_pct: f32,
    /// Used memory, bytes.
    pub mem_used: u64,
    /// Total memory, bytes.
    pub mem_total: u64,
    /// Used disk, bytes.
    pub disk_used: u64,
    /// Total disk, bytes.
    pub disk_total: u64,
    /// Last frame, seconds since the Unix epoch.
    pub last_seen: u64,
    /// `online` or `offline`.
    pub status: String,
}

/// A `/cluster/sessions` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionRow {
    /// The member that reported the session.
    pub member: String,
    /// Session id.
    pub id: String,
    /// Working directory.
    pub cwd: Option<String>,
    /// Status.
    pub status: Option<String>,
    /// Last visible message.
    pub last_message: Option<String>,
    /// When the member last reported it (epoch seconds).
    pub ts: u64,
}

/// The shared registry handle.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    inner: Arc<Mutex<BTreeMap<String, Entry>>>,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut BTreeMap<String, Entry>) -> R) -> R {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(&mut guard)
    }

    /// Register (or re-register) an admitted member.
    pub fn admit(&self, name: &str, addr: &str, heartbeat: Duration) {
        self.with(|m| {
            let prev = m.remove(name).and_then(|e| e.report);
            m.insert(
                name.to_string(),
                Entry {
                    addr: addr.to_string(),
                    heartbeat,
                    report: prev,
                    last_seen: Instant::now(),
                    last_seen_epoch: crate::now_secs(),
                },
            );
        });
    }

    /// Record a heartbeat from an admitted member.
    pub fn heartbeat(&self, name: &str, report: Report) {
        self.with(|m| {
            if let Some(e) = m.get_mut(name) {
                e.report = Some(report);
                e.last_seen = Instant::now();
                e.last_seen_epoch = crate::now_secs();
            }
        });
    }

    fn is_online(e: &Entry, now: Instant) -> bool {
        now.duration_since(e.last_seen) <= e.heartbeat.saturating_mul(MISSED_HEARTBEATS)
    }

    /// The `/cluster/members` rows, ordered by name.
    #[must_use]
    pub fn members(&self) -> Vec<MemberRow> {
        let now = Instant::now();
        self.with(|m| {
            m.iter()
                .map(|(name, e)| {
                    let r = e.report.clone().unwrap_or_default();
                    MemberRow {
                        name: name.clone(),
                        addr: e.addr.clone(),
                        hostname: e.report.as_ref().map(|r| r.hostname.clone()),
                        cpu_pct: r.cpu_pct,
                        mem_used: r.mem_used,
                        mem_total: r.mem_total,
                        disk_used: r.disk_used,
                        disk_total: r.disk_total,
                        last_seen: e.last_seen_epoch,
                        status: if Self::is_online(e, now) {
                            "online"
                        } else {
                            "offline"
                        }
                        .to_string(),
                    }
                })
                .collect()
        })
    }

    /// The `/cluster/sessions` rows across every member, ordered by member then id.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionRow> {
        self.with(|m| {
            m.iter()
                .flat_map(|(name, e)| {
                    let ts = e.last_seen_epoch;
                    e.report
                        .iter()
                        .flat_map(|r| r.sessions.iter())
                        .map(move |s| SessionRow {
                            member: name.clone(),
                            id: s.id.clone(),
                            cwd: s.cwd.clone(),
                            status: s.status.clone(),
                            last_message: s.last_message.clone(),
                            ts,
                        })
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_lapses_after_three_missed_heartbeats() {
        let reg = Registry::new();
        reg.admit("a", "127.0.0.1:1", Duration::from_millis(20));
        assert_eq!(reg.members()[0].status, "online");
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(reg.members()[0].status, "offline");
        reg.heartbeat("a", Report::default());
        assert_eq!(reg.members()[0].status, "online");
    }

    #[test]
    fn a_heartbeat_from_an_unadmitted_name_is_ignored() {
        let reg = Registry::new();
        reg.heartbeat("ghost", Report::default());
        assert!(reg.members().is_empty());
    }
}
