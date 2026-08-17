//! Link label normalization (CommonMark's "matches" relation),
//! micromark's `normalizeIdentifier` equivalence:
//!
//! ```js
//! value
//!   .replace(/[\t\n\r ]+/g, ' ')
//!   .replace(/^ | $/g, '')
//!   .toLowerCase()
//!   .toUpperCase()
//! ```
//!
//! Only markdown whitespace collapses (never NBSP / U+3000),
//! and the fold is lowercase-then-uppercase
//! (unifies ß/ẞ/SS, σ/ς, ﬀ/FF, which lowercasing alone does not).
//! The key here is that fold lowercased once more:
//! it induces the same equivalence (`f∘g == f` holds for every char, checked exhaustively),
//! keeps ordinary lowercase labels on the borrowing fast path,
//! and is exactly the id micromark's footnote HTML derives (`normalizeIdentifier(x).toLowerCase()`).
//! An opaque key for equality, not display text.
//! Public because resolving reference links from `Definition` nodes needs
//! the same normalization the parser used.

use std::borrow::Cow;

/// Borrow-through fast path for labels that are already in key form.
///
/// Lookups happen per reference candidate.
/// Unlike [`crate::decode`]'s coarse trigger bytes,
/// `is_normalized` is an exact mirror of the building loop below,
/// the `idempotent` test pins the two in lockstep.
pub fn normalize(raw: &str) -> Cow<'_, str> {
    if is_normalized(raw) {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;
    for c in raw.chars() {
        if is_markdown_space(c) {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.extend(fold(c));
        }
    }
    Cow::Owned(out)
}

/// JS `[\t\n\r ]`.
fn is_markdown_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// `c.toLowerCase().toUpperCase().toLowerCase()`; for ASCII this is `to_ascii_lowercase`.
fn fold(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase().flat_map(char::to_uppercase).flat_map(char::to_lowercase)
}

/// Whether trim/collapse/fold would all be no-ops.
fn is_normalized(raw: &str) -> bool {
    // Leading/trailing space would be trimmed; a space run would collapse.
    if raw.starts_with(' ') || raw.ends_with(' ') || raw.contains("  ") {
        return false;
    }
    raw.chars().all(|c| match c {
        ' ' => true,
        c if is_markdown_space(c) => false,
        c if c.is_ascii() => !c.is_ascii_uppercase(),
        c => {
            let mut folded = fold(c);
            folded.next() == Some(c) && folded.next().is_none()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use std::borrow::Cow;

    /// The fast path must agree with the building path:
    /// a borrowed result means "already normalized", so normalizing again must be a no-op,
    /// and an owned result must itself be in normalized (borrowed) form.
    #[test]
    fn idempotent() {
        for raw in [
            "",
            "a",
            "a b",
            "toc",
            "ß",
            "ẞ",
            "ss",
            "A",
            "Толпой",
            "толпой",
            " a",
            "a ",
            "a  b",
            "a\tb",
            "a\nb",
            "é",
            "É",
            "漢字",
            "ﬀ",
        ] {
            let once = normalize(raw);
            let twice = normalize(&once);
            assert_eq!(once, twice, "not idempotent for {raw:?}");
            assert!(matches!(twice, Cow::Borrowed(_)), "owned twice for {raw:?}");
        }
    }

    #[test]
    fn matches_relation() {
        assert_eq!(normalize("  A  B\t"), "a b");
        assert_eq!(normalize("ẞ"), "ss");
        assert_eq!(normalize("ß"), "ss");
        assert_eq!(normalize("ТОЛПОЙ"), normalize("Толпой"));
        // micromark's fold unifies these; lowercasing alone would not
        assert_eq!(normalize("σ"), normalize("ς"));
        assert_eq!(normalize("ﬀ"), normalize("ff"));
        // Only markdown whitespace collapses or trims
        assert_ne!(normalize("a\u{3000}b"), normalize("a b"));
        assert_eq!(normalize("\u{a0}"), "\u{a0}");
        assert!(matches!(normalize("a b"), Cow::Borrowed(_)));
        assert!(matches!(normalize("A b"), Cow::Owned(_)));
        assert!(matches!(normalize("a  b"), Cow::Owned(_)));
        assert!(matches!(normalize("ß"), Cow::Owned(_)));
    }
}
