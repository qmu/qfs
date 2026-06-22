//! `cfs-lang` — the closed-core language surface for cfs.
//!
//! This crate is the single home of the **frozen reserved-keyword set** (RFD-0001
//! §3, "Language: closed core + three open registries"). The closed core is the
//! one thing in cfs that is *not* an open registry: new backends add **zero
//! keywords** (a new service = a new mount; a new action = a registered procedure;
//! a new format = a registered codec). Freezing the keyword set in exactly one
//! place is what makes that governance thesis structurally enforceable — a later
//! ticket that wants new behaviour has nowhere to add a keyword except here, and
//! the golden test ([`mod tests`]) fails if it tries (fidelity guard G1 / C1).
//!
//! AST sum types (the `enum`-modelled grammar) land in this crate in E1. E0 ships
//! only the frozen keyword vocabulary plus the [`keywords`] module that exposes it.
//!
//! ## wasm-friendliness (boundary guard B7)
//! This crate is pure data: no threads, no `std::fs`, no sockets. It must stay that
//! way so the `wasm32` target (RFD §9) remains cheap.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod keywords;

pub use keywords::{Keyword, KEYWORDS, OPERATORS};
