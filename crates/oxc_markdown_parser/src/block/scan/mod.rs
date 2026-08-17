//! Line-level scanners for block starts.
//!
//! Every function takes the tail of a physical line
//! (container prefixes and leading indent already consumed by the caller)
//! and answers "does a block of this kind start here", returning byte ranges relative to the tail.
//! They are pure lexical facts, the interrupt/priority ordering between them lives in the engine.
//!
//! One file per construct family; everything is re-exported flat
//! so the engine's probe table reads as one list.

use std::ops::Range;

mod commonmark;
mod directive;
mod footnote;
mod html;
mod math;
mod table;

pub use commonmark::*;
pub use directive::*;
pub use footnote::*;
pub use html::*;
pub use math::*;
pub use table::*;

/// Only spaces and tabs to the end.
pub fn is_blank(tail: &str) -> bool {
    tail.bytes().all(|b| b == b' ' || b == b'\t')
}

/// Shrinks a byte range over `s` past its leading/trailing spaces and tabs.
fn trim_range(s: &str, range: Range<usize>) -> Range<usize> {
    let slice = &s[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start_matches([' ', '\t']).len());
    let end = range.start + slice.trim_end_matches([' ', '\t']).len();
    // An all-whitespace range trims to empty (end would land before start)
    start..end.max(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines() {
        assert!(is_blank("") && is_blank(" \t "));
        assert!(!is_blank(" x") && !is_blank("\u{A0}"));
    }

    #[test]
    fn trim_range_shrinks_to_content() {
        let s = "  a b \t";
        assert_eq!(trim_range(s, 0..s.len()), 2..5);
        assert_eq!(trim_range(s, 1..3), 2..3);
        // All-whitespace collapses to empty at its trimmed start
        assert_eq!(trim_range(s, 5..7), 7..7);
    }
}
