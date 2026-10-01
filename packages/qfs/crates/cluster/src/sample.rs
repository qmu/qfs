//! Resource sampling on a member via `sysinfo`.

use sysinfo::{Disks, System};

/// A reusable sampler (CPU usage needs a previous refresh to diff against).
pub struct Sampler {
    sys: System,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    /// A sampler primed with one CPU refresh.
    #[must_use]
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Self { sys }
    }

    /// Sample `(hostname, cpu_pct, mem_used, mem_total, disk_used, disk_total)`.
    pub fn sample(&mut self) -> (String, f32, u64, u64, u64, u64) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let disks = Disks::new_with_refreshed_list();
        // Prefer the root volume (APFS lists several volumes sharing one container, so summing
        // would double-count); fall back to the sum when no `/` is listed.
        let root = disks
            .list()
            .iter()
            .find(|d| d.mount_point() == std::path::Path::new("/"));
        let (total, avail) = match root {
            Some(d) => (d.total_space(), d.available_space()),
            None => disks.list().iter().fold((0u64, 0u64), |(t, a), d| {
                (
                    t.saturating_add(d.total_space()),
                    a.saturating_add(d.available_space()),
                )
            }),
        };
        (
            System::host_name().unwrap_or_else(|| "unknown".to_string()),
            self.sys.global_cpu_usage(),
            self.sys.used_memory(),
            self.sys.total_memory(),
            total.saturating_sub(avail),
            total,
        )
    }
}
