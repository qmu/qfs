//! `cfs` — the single binary (RFD-0001 §9: one Rust binary, both CLI and server).
//!
//! This entrypoint is deliberately **thin**: it forwards argv to
//! [`cfs_cmd::run`] and exits with the code it returns. All argv parsing, dispatch,
//! and rendering live in `cfs-cmd`; all domain logic lives below `cfs-core`. There
//! is no logic here to test, and nothing else depends on this crate.

fn main() {
    let code = cfs_cmd::run(std::env::args_os());
    std::process::exit(code);
}
