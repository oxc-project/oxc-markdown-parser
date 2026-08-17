//! CommonMark core block starts: thematic breaks, headings, code fences, list markers.

use std::ops::Range;

use crate::syntax::skip_ws;

use super::{is_blank, trim_range};

/// Thematic break: 3+ of the same `-` / `_` / `*`,
/// interleaved with spaces/tabs only.
pub fn thematic_break(tail: &str) -> bool {
    let mut marker = 0u8;
    let mut count = 0usize;
    for b in tail.bytes() {
        match b {
            b' ' | b'\t' => {}
            b'-' | b'_' | b'*' => {
                if marker == 0 {
                    marker = b;
                } else if b != marker {
                    return false;
                }
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}

/// ATX heading: `#{1,6}` followed by space/tab or EOL.
/// Returns the level and the content byte range within `tail` (closing `#` run stripped).
#[expect(clippy::cast_possible_truncation)] // level is 1..=6
pub fn atx_heading(tail: &str) -> Option<(u8, Range<usize>)> {
    let bytes = tail.as_bytes();
    let level = bytes.iter().take_while(|&&b| b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    match bytes.get(level) {
        None => return Some((level as u8, level..level)),
        Some(b' ' | b'\t') => {}
        Some(_) => return None,
    }
    // Trim surrounding whitespace,
    // then a trailing `#` run preceded by space/tab (or making up the whole content).
    let start = skip_ws(bytes, level, 0);
    let mut end = bytes.len();
    while end > start && matches!(bytes[end - 1], b' ' | b'\t') {
        end -= 1;
    }
    let closing = (start..end).rev().take_while(|&i| bytes[i] == b'#').count();
    if closing > 0 {
        let before = end - closing;
        if before == start {
            end = start;
        } else if matches!(bytes[before - 1], b' ' | b'\t') {
            end = before;
            while end > start && matches!(bytes[end - 1], b' ' | b'\t') {
                end -= 1;
            }
        }
    }
    Some((level as u8, start..end))
}

/// Setext underline: a run of `=` or `-` plus trailing whitespace.
/// Returns the heading level (1 for `=`, 2 for `-`).
pub fn setext_underline(tail: &str) -> Option<u8> {
    let bytes = tail.as_bytes();
    let marker = *bytes.first()?;
    if marker != b'=' && marker != b'-' {
        return None;
    }
    let run = bytes.iter().take_while(|&&b| b == marker).count();
    if !is_blank(&tail[run..]) {
        return None;
    }
    Some(if marker == b'=' { 1 } else { 2 })
}

/// Opening code fence: 3+ backticks or tildes.
/// Returns (fence char, fence length, info string byte range within `tail`, trimmed).
/// Backtick info strings must not contain backticks.
#[expect(clippy::cast_possible_truncation)] // fence runs fit in-line lengths
pub fn fence_open(tail: &str) -> Option<(u8, u32, Range<usize>)> {
    let bytes = tail.as_bytes();
    let marker = *bytes.first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let run = bytes.iter().take_while(|&&b| b == marker).count();
    if run < 3 {
        return None;
    }
    let info = trim_range(tail, run..tail.len());
    if marker == b'`' && tail[info.clone()].contains('`') {
        return None;
    }
    Some((marker, run as u32, info))
}

/// Closing code fence: a run of `marker` at least `min_len` long, then only whitespace.
pub fn fence_close(tail: &str, marker: u8, min_len: u32) -> bool {
    let bytes = tail.as_bytes();
    let run = bytes.iter().take_while(|&&b| b == marker).count();
    run >= min_len as usize && is_blank(&tail[run..])
}

pub struct ListMarkerScan {
    /// Bullet char (`-*+`) or ordered delimiter (`.)`).
    pub marker: u8,
    pub ordered: bool,
    /// Bytes of the whole marker (digits + delimiter, or 1 for bullets).
    /// Markers are ASCII, so bytes == columns.
    pub len: usize,
    /// For ordered markers, whether the number is exactly `1`.
    pub starts_at_one: bool,
}

/// List marker: `-` / `*` / `+`, or 1–9 digits + `.` / `)`,
/// followed by space/tab or EOL.
pub fn list_marker(tail: &str) -> Option<ListMarkerScan> {
    let bytes = tail.as_bytes();
    let first = *bytes.first()?;
    let (scan, after) = if matches!(first, b'-' | b'*' | b'+') {
        (ListMarkerScan { marker: first, ordered: false, len: 1, starts_at_one: false }, 1)
    } else if first.is_ascii_digit() {
        let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 9 {
            return None;
        }
        let delimiter = *bytes.get(digits)?;
        if delimiter != b'.' && delimiter != b')' {
            return None;
        }
        // Leading zeros are allowed; `001` still counts as starting at 1
        let starts_at_one = tail[..digits].parse::<u64>() == Ok(1);
        (
            ListMarkerScan { marker: delimiter, ordered: true, len: digits + 1, starts_at_one },
            digits + 1,
        )
    } else {
        return None;
    };
    match bytes.get(after) {
        None | Some(b' ' | b'\t') => Some(scan),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thematic_breaks() {
        assert!(thematic_break("***"));
        assert!(thematic_break("- - -"));
        assert!(thematic_break("__\t_ "));
        assert!(!thematic_break("--"));
        assert!(!thematic_break("-*-"));
        assert!(!thematic_break("--- a"));
    }

    #[test]
    fn atx_headings() {
        assert_eq!(atx_heading("# h"), Some((1, 2..3)));
        assert_eq!(atx_heading("###"), Some((3, 3..3)));
        assert_eq!(atx_heading("#\tx  "), Some((1, 2..3)));
        // Closing run is stripped only when preceded by whitespace (or alone)
        assert_eq!(atx_heading("## h ##"), Some((2, 3..4)));
        assert_eq!(atx_heading("## ##"), Some((2, 3..3)));
        assert_eq!(atx_heading("# h#"), Some((1, 2..4)));
        assert_eq!(atx_heading("#hashtag"), None);
        assert_eq!(atx_heading("####### seven"), None);
    }

    #[test]
    fn setext_underlines() {
        assert_eq!(setext_underline("==="), Some(1));
        assert_eq!(setext_underline("-  "), Some(2));
        assert_eq!(setext_underline("= ="), None);
        assert_eq!(setext_underline("~~~"), None);
    }

    #[test]
    fn fences() {
        assert_eq!(fence_open("```rust "), Some((b'`', 3, 3..7)));
        assert_eq!(fence_open("~~~~"), Some((b'~', 4, 4..4)));
        assert_eq!(fence_open("``"), None);
        // Backtick info strings cannot contain backticks; tilde ones can
        assert_eq!(fence_open("``` a`b"), None);
        assert_eq!(fence_open("~~~ a`b"), Some((b'~', 3, 4..7)));

        assert!(fence_close("````  ", b'`', 3));
        assert!(!fence_close("``", b'`', 3));
        assert!(!fence_close("``` x", b'`', 3));
        assert!(!fence_close("~~~", b'`', 3));
    }

    #[test]
    fn list_markers() {
        let m = list_marker("- a").unwrap();
        assert_eq!((m.marker, m.ordered, m.len), (b'-', false, 1));
        assert!(list_marker("+").is_some(), "marker at end of line");

        let m = list_marker("1. a").unwrap();
        assert_eq!((m.marker, m.ordered, m.len, m.starts_at_one), (b'.', true, 2, true));
        let m = list_marker("001)\tx").unwrap();
        assert_eq!((m.marker, m.len, m.starts_at_one), (b')', 4, true));
        assert!(!list_marker("2.").unwrap().starts_at_one);

        assert!(list_marker("-a").is_none());
        assert!(list_marker("1234567890. ten digits").is_none());
        assert!(list_marker("1: x").is_none());
    }
}
