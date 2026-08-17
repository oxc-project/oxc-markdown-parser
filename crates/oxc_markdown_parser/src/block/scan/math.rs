//! Math flow (`$$`) fence start.

use std::ops::Range;

use super::trim_range;

/// Opening math fence: 2+ `$`s, then meta text that must not contain `$`.
/// Returns (fence length, meta byte range within `tail`), trimmed like a code fence's info string
/// (micromark keeps the trailing run: `DIVERGENCES.md`, "Fenced code meta keeps trailing whitespace").
#[expect(clippy::cast_possible_truncation)] // fence runs fit in-line lengths
pub fn math_fence_open(tail: &str) -> Option<(u32, Range<usize>)> {
    let bytes = tail.as_bytes();
    let run = bytes.iter().take_while(|&&b| b == b'$').count();
    if run < 2 {
        return None;
    }
    let rest = &tail[run..];
    if rest.contains('$') {
        return None;
    }
    Some((run as u32, trim_range(tail, run..tail.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_open() {
        assert_eq!(math_fence_open("$$"), Some((2, 2..2)));
        assert_eq!(math_fence_open("$$$  "), Some((3, 5..5)), "whitespace only is no meta");
        // Surrounding whitespace is stripped
        assert_eq!(math_fence_open("$$ meta "), Some((2, 3..7)));
        assert_eq!(math_fence_open("$"), None);
        assert_eq!(math_fence_open("$$a$"), None, "meta cannot contain `$`");
    }
}
