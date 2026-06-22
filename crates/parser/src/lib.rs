//! `cfs-parser` — the parser front door (RFD-0001 §2.2, §9).
//!
//! **This crate is the empty-but-typed skeleton reserved for ticket t02.** E0 (t01)
//! creates it as the 9th workspace member, wires its dependency on [`cfs_lang`]
//! (boundary B6: the parser consumes the frozen keyword consts and AST), and stubs
//! the public surface. **t02 fills it**: the winnow-vs-chumsky decision spike under
//! `spikes/`, the owned [`ParseError`], the real `parse_statement`, the golden error
//! corpus, and the ADR `docs/adr/0001-parser-library.md`.
//!
//! ## Reversibility (fidelity guard G6, boundary B6)
//! The chosen parser library's types must **never** appear in this crate's public
//! API — they are wrapped behind the owned [`ParseError`] and the `parse_statement`
//! signature, so E1 can swap libraries without breaking callers.
//!
//! ## Reserved upward edge (acceptance criterion C5)
//! The intended dependency direction is `cfs-core → cfs-parser` (core calls
//! `parse_statement` to turn DSL text into AST). That edge is **declared here, not
//! yet wired**, so E1 adds it in one direction and cannot accidentally introduce a
//! cycle (`cfs-parser` must never depend on `cfs-core`). See the crate-level note
//! and `ARCHITECTURE.md`.
//!
//! ## wasm-friendliness (boundary guard B7)
//! `cfs-parser` must build for `wasm32` (RFD §9). No threads, no `std::fs`, no
//! sockets — t02 keeps the chosen library's features pure.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

/// Owned parse error (skeleton). t02 fills in byte/char span, expected-set, and a
/// machine code; the chosen library's error type is wrapped here and never exposed
/// (fidelity guard G6). Kept as an opaque owned type at E0.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ParseError {
    /// A short, machine-facing description. Replaced by structured span/expected
    /// fields in t02.
    pub message: String,
}

/// Parse one cfs statement into an AST (skeleton).
///
/// E0 returns the not-implemented error; t02 implements the real grammar with the
/// library its ADR locks. The signature is the stable, reversible boundary: the AST
/// type and this function shape do not depend on the chosen parser library.
///
/// # Errors
/// At E0 always returns [`ParseError`] indicating the parser is not yet implemented.
pub fn parse_statement(_src: &str) -> Result<(), ParseError> {
    // TODO(t02/E1): implement the real grammar (winnow default per RFD §9; chumsky
    // if the golden error-recovery corpus is decisive). Return an owned AST node.
    Err(ParseError {
        message: "parser not yet implemented (reserved for t02/E1)".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The skeleton compiles and the reserved consts from cfs-lang are reachable —
    /// proving the B6 edge (`cfs-parser -> cfs-lang`) is wired.
    #[test]
    fn skeleton_returns_not_implemented_and_sees_lang_keywords() {
        assert!(parse_statement("FROM /mail").is_err());
        // The frozen keyword vocabulary is visible from the parser crate.
        assert!(cfs_lang::KEYWORDS.contains(&"FROM"));
    }
}
