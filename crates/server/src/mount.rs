//! The `/server/...` mount stub (RFD-0001 §8).
//!
//! ## The server IS a driver (fidelity guard G4, boundary B5)
//! §8 is explicit: the server is **a driver**, not a privileged subsystem. Its
//! endpoints, triggers, jobs, views, policies, and webhooks are **data** managed via
//! cfs (`CREATE …` forms are sugar over `INSERT INTO /server/...`). E0 must reserve
//! that seam so the rewrite does not quietly reintroduce a "server is special"
//! boundary the RFD rejects.
//!
//! E0 ships only this module and the mount string. The actual `Driver` impl over
//! `/server/...` (registered into the `MountRegistry` like any other driver) is E7.

/// The reserved mount point for the server-as-a-driver (RFD-0001 §8).
pub const SERVER_MOUNT: &str = "/server";

// TODO(E7): register /server as a Driver.
// `cfs-server` will implement `cfs_core::Driver` over `SERVER_MOUNT` and register it
// into the `Engine`'s `MountRegistry` exactly like any other driver. Endpoints /
// triggers / jobs / views / webhooks / policies become rows under `/server/...`, and
// `CREATE ENDPOINT|TRIGGER|JOB|[MATERIALIZED] VIEW|WEBHOOK|POLICY` desugars to
// `INSERT INTO /server/...`. The server must NOT be a bespoke entrypoint that
// bypasses the mount registry (that would violate §8 / boundary B5).
