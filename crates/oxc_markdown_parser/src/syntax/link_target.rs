//! Link-target productions: destination, title, label
//! (= micromark's factory-destination / factory-title / factory-label).
//! Used by both the reference-definition parser (block phase)
//! and inline link suffixes so the grammar exists exactly once.

use std::ops::Range;

use super::{escapes_next, skip_ws};

/// Link destination at `start`: `<…>` (no unescaped `<`/`>` or line ending)
/// or a bare run with balanced parens, ended by whitespace or a control character.
/// Returns (range including any `<>`, angle flag);
/// `None` for a malformed angle form or unbalanced parens.
/// A bare empty destination yields an empty range, callers decide whether that is acceptable.
pub fn destination(t: &str, start: usize) -> Option<(Range<usize>, bool)> {
    let bytes = t.as_bytes();
    if bytes.get(start) == Some(&b'<') {
        let mut p = start + 1;
        loop {
            match bytes.get(p)? {
                b'\\' => p += if escapes_next(bytes, p) { 2 } else { 1 },
                b'>' => return Some((start..p + 1, true)),
                b'<' | b'\n' => return None,
                _ => p += 1,
            }
        }
    }
    let mut p = start;
    let mut depth = 0i32;
    while let Some(&c) = bytes.get(p) {
        match c {
            b'\\' => p += if escapes_next(bytes, p) { 2 } else { 1 },
            b'(' => {
                depth += 1;
                p += 1;
            }
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                p += 1;
            }
            c if c <= b' ' || c == 0x7F => break,
            _ => p += 1,
        }
    }
    (depth == 0).then(|| (start..p.min(bytes.len()), false))
}

/// Link/definition title at `start` (which must hold `"`, `'` or `(`).
/// May span line endings; a `(` title rejects unescaped nested `(`.
/// Returns the range including the quotes.
pub fn title(t: &str, start: usize) -> Option<Range<usize>> {
    let bytes = t.as_bytes();
    let open = *bytes.get(start)?;
    if !matches!(open, b'"' | b'\'' | b'(') {
        return None;
    }
    let close = if open == b'(' { b')' } else { open };
    let mut p = start + 1;
    loop {
        match bytes.get(p)? {
            b'\\' => p += if escapes_next(bytes, p) { 2 } else { 1 },
            &c if c == close => return Some(start..p + 1),
            b'(' if open == b'(' => return None,
            _ => p += 1,
        }
    }
}

/// Link label at `start` (which must hold `[`): no unescaped nested `[`,
/// at most 999 characters between the brackets,
/// at least one of them not a space, tab or line ending
/// (micromark's factory-label; a collapsed reference's `[]` is matched literally by its caller).
/// Returns the position just past the `]`.
pub fn label_end(t: &str, start: usize) -> Option<usize> {
    let bytes = t.as_bytes();
    let mut p = start + 1;
    loop {
        match bytes.get(p)? {
            b'\\' => p += if escapes_next(bytes, p) { 2 } else { 1 },
            b'[' => return None,
            b']' => break,
            _ => p += 1,
        }
        if p - start > 999 * 4 {
            return None;
        }
    }
    if skip_ws(bytes, start + 1, usize::MAX) == p || t.get(start + 1..p)?.chars().count() > 999 {
        return None;
    }
    Some(p + 1)
}

/// GFM footnote label at `start` (which must hold `[`):
/// `[^…]` with no whitespace, line ending or `[` inside; `\` escapes `[`, `\`, `]`;
/// at least one non-whitespace character, at most 999.
/// Returns (label range, position past the `]`).
pub fn footnote_label(t: &str, start: usize) -> Option<(Range<usize>, usize)> {
    let bytes = t.as_bytes();
    if bytes.get(start) != Some(&b'[') || bytes.get(start + 1) != Some(&b'^') {
        return None;
    }
    let mut i = start + 2;
    let mut data = false;
    loop {
        match bytes.get(i)? {
            b']' => break,
            b'[' | b' ' | b'\t' | b'\n' => return None,
            b'\\' => {
                data = true;
                i += if matches!(bytes.get(i + 1), Some(b'[' | b'\\' | b']')) { 2 } else { 1 };
            }
            _ => {
                data = true;
                i += 1;
            }
        }
        if i > start + 999 + 2 {
            return None;
        }
    }
    data.then_some((start + 2..i, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations() {
        assert_eq!(destination("<a b>", 0), Some((0..5, true)));
        assert_eq!(destination("<a\\>b>", 0), Some((0..6, true)));
        assert_eq!(destination("<a\nb>", 0), None);
        assert_eq!(destination("<a<b>", 0), None);

        assert_eq!(destination("/u(x) t", 0), Some((0..5, false)));
        assert_eq!(destination("/u) t", 0), Some((0..2, false)));
        assert_eq!(destination("/u(x t", 0), None, "unbalanced parens");
        assert_eq!(destination("", 0), Some((0..0, false)));
    }

    #[test]
    fn titles() {
        assert_eq!(title("\"t\" x", 0), Some(0..3));
        assert_eq!(title("'a\\'b'", 0), Some(0..6));
        assert_eq!(title("(a\nb)", 0), Some(0..5));
        assert_eq!(title("(a(b))", 0), None, "nested paren in paren title");
        assert_eq!(title("\"open", 0), None);
        assert_eq!(title("x", 0), None);
    }

    #[test]
    fn labels() {
        assert_eq!(label_end("[a]", 0), Some(3));
        assert_eq!(label_end("[a\\]b]", 0), Some(6));
        assert_eq!(label_end("[ ]", 0), None, "needs a non-whitespace character");
        assert_eq!(label_end("[a[b]]", 0), None);
        assert_eq!(label_end("[a", 0), None);
        let long = format!("[{}]", "x".repeat(1000));
        assert_eq!(label_end(&long, 0), None);
    }

    #[test]
    fn footnote_labels() {
        assert_eq!(footnote_label("[^a]:", 0), Some((2..3, 4)));
        assert_eq!(footnote_label("[^a\\]b]", 0), Some((2..6, 7)));
        assert_eq!(footnote_label("[^]", 0), None);
        assert_eq!(footnote_label("[^a b]", 0), None);
        assert_eq!(footnote_label("[a]", 0), None);
    }
}
