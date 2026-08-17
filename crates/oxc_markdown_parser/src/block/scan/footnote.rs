//! GFM footnote definition start (`[^label]:`).

use std::ops::Range;

use crate::syntax::link_target::footnote_label;

/// GFM footnote definition start: `[^label]:`.
/// Label grammar lives in [`footnote_label`].
/// Returns (label byte-range within `tail`, position past the `:`).
pub fn footnote_definition_start(tail: &str) -> Option<(Range<usize>, usize)> {
    let (label, close) = footnote_label(tail, 0)?;
    (tail.as_bytes().get(close) == Some(&b':')).then(|| (label, close + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_start() {
        // Label grammar is pinned by `footnote_label`'s own tests
        assert_eq!(footnote_definition_start("[^1]: note"), Some((2..3, 5)));
        assert_eq!(footnote_definition_start("[^1]"), None, "colon required");
    }
}
