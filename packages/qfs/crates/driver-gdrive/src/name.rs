//! Unicode canonical normalization for Drive **name lookups** (ticket 20260717120100).
//!
//! Drive stores a file's name as the uploading client sent it, byte for byte, and matches
//! `name = '…'` in a `q` search by those same bytes. macOS clients upload **NFD** (decomposed:
//! `が` = `か` + U+3099), while a name copied out of a listing and pasted into a path arrives
//! **NFC** (precomposed) from essentially every terminal. The two spell the same name and render
//! identically, so an exact-bytes lookup answers `not_found` for a file the listing plainly
//! shows — a name the user can see but cannot address.
//!
//! This module is the pure normalization leaf that closes that gap: [`match_forms`] enumerates the
//! canonical spellings a segment could be **stored** under, and the read/write walks
//! ([`crate::read`]) match a name against all of them in ONE Drive query. No I/O, no allocation
//! beyond the returned strings.
//!
//! ## Scope (honest limits)
//! Only the two canonical composition forms are enumerated — NFC and NFD. A name stored in a
//! *partially* decomposed spelling (neither fully-composed nor fully-decomposed; not produced by
//! any mainstream client) still misses, and misses **fail closed** as `not_found` rather than
//! resolving to a guess. Compatibility normalization (NFKC/NFKD) is deliberately NOT applied: it
//! folds distinctions Drive treats as different names (`①` → `1`, full-width → half-width), so
//! two genuinely different files could collapse onto one address.

use unicode_normalization::UnicodeNormalization;

/// Normalization Form C (canonical composition) of `s`.
#[must_use]
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Normalization Form D (canonical decomposition) of `s`.
#[must_use]
pub fn nfd(s: &str) -> String {
    s.nfd().collect()
}

/// The distinct spellings `name` could be **stored** under: the segment exactly as written, plus
/// its NFC and NFD forms, deduplicated and order-stable (as-written first).
///
/// For an ASCII name — and for any already-canonical name with no composable characters — all
/// three coincide and the result is the single as-written form, so the emitted Drive query is
/// **byte-identical** to the pre-normalization one (no cost, no behaviour change, no widening).
/// Only a name that actually has two canonical spellings produces two (or three) forms.
///
/// The as-written form is kept even when it is neither NFC nor NFD, so a segment that already
/// matches the stored bytes exactly always resolves regardless of its composition form.
#[must_use]
pub fn match_forms(name: &str) -> Vec<String> {
    let mut forms = vec![name.to_string()];
    for form in [nfc(name), nfd(name)] {
        if !forms.contains(&form) {
            forms.push(form);
        }
    }
    forms
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// The NFC (precomposed) and NFD (decomposed) spellings of `が`.
    const NFC_GA: &str = "\u{304C}";
    const NFD_GA: &str = "\u{304B}\u{3099}";

    #[test]
    fn the_two_forms_of_the_same_name_are_different_bytes_but_normalize_together() {
        // The premise of the whole module: these render identically and mean the same name, yet
        // an exact-bytes Drive `name =` match treats them as different files.
        assert_ne!(NFC_GA, NFD_GA);
        assert_eq!(nfc(NFD_GA), NFC_GA);
        assert_eq!(nfd(NFC_GA), NFD_GA);
    }

    #[test]
    fn an_ascii_name_yields_exactly_one_form() {
        // The no-op guarantee: an ASCII segment must not widen the query at all.
        assert_eq!(match_forms("report.txt"), vec!["report.txt".to_string()]);
    }

    #[test]
    fn a_composable_name_yields_both_canonical_forms() {
        let forms = match_forms(NFC_GA);
        assert_eq!(forms.len(), 2, "NFC as written + its NFD spelling");
        assert_eq!(forms[0], NFC_GA, "as written comes first");
        assert!(forms.contains(&NFD_GA.to_string()));

        // ...and symmetrically from the decomposed side.
        let forms = match_forms(NFD_GA);
        assert_eq!(forms.len(), 2);
        assert_eq!(forms[0], NFD_GA, "as written comes first");
        assert!(forms.contains(&NFC_GA.to_string()));
    }

    #[test]
    fn forms_are_deduplicated() {
        // A name with non-ASCII characters that have no decomposition: NFC == NFD == as written.
        let forms = match_forms("日本語.txt");
        assert_eq!(forms, vec!["日本語.txt".to_string()]);
    }

    #[test]
    fn compatibility_folding_is_not_applied() {
        // NFKC would fold `①` to `1` and full-width `Ａ` to `A` — different Drive names that must
        // never collapse onto one address.
        assert_eq!(match_forms("①.txt"), vec!["①.txt".to_string()]);
        assert_eq!(match_forms("Ａ.txt"), vec!["Ａ.txt".to_string()]);
    }
}
