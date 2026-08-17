//! HTML tag grammar past the tag name (CommonMark 6.4 / 4.6),
//! shared by inline raw HTML and single-line HTML block starts.

use super::skip_ws;

/// Scans the remainder of a tag at `i` (just past the name) through its `>`:
/// whitespace only for a closing tag, whitespace-separated attributes and an optional `/` for an open tag.
/// Returns the position past the `>`.
/// `newlines` says whether intra-tag whitespace may include line endings
/// (inline HTML: yes; single-line HTML block starts: no).
pub fn tag_end(bytes: &[u8], mut i: usize, closing: bool, newlines: bool) -> Option<usize> {
    let max_newlines = if newlines { usize::MAX } else { 0 };
    if closing {
        i = skip_ws(bytes, i, max_newlines);
        return (bytes.get(i) == Some(&b'>')).then_some(i + 1);
    }
    loop {
        let before_ws = i;
        i = skip_ws(bytes, i, max_newlines);
        match bytes.get(i)? {
            b'>' => return Some(i + 1),
            b'/' => return (bytes.get(i + 1) == Some(&b'>')).then_some(i + 2),
            _ => {
                // An attribute requires preceding whitespace
                if i == before_ws {
                    return None;
                }
                i = attribute(bytes, i, newlines)?;
            }
        }
    }
}

/// One tag attribute: name, optionally `= value`.
/// Returns the position past it
/// (past the name for a valueless attribute, trailing whitespace is the caller's).
/// `newlines` as in [`tag_end`].
fn attribute(bytes: &[u8], mut i: usize, newlines: bool) -> Option<usize> {
    let max_newlines = if newlines { usize::MAX } else { 0 };
    let first = *bytes.get(i)?;
    if !first.is_ascii_alphabetic() && first != b'_' && first != b':' {
        return None;
    }
    i += 1;
    while bytes
        .get(i)
        .copied()
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-'))
    {
        i += 1;
    }
    let after_name = i;
    i = skip_ws(bytes, i, max_newlines);
    if bytes.get(i) != Some(&b'=') {
        return Some(after_name);
    }
    i = skip_ws(bytes, i + 1, max_newlines);
    if let &quote @ (b'"' | b'\'') = bytes.get(i)? {
        i += 1;
        while let Some(&c) = bytes.get(i) {
            if c == quote {
                return Some(i + 1);
            }
            i += 1;
        }
        None
    } else {
        let start = i;
        while bytes.get(i).copied().is_some_and(|c| {
            !matches!(c, b' ' | b'\t' | b'\n' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
        }) {
            i += 1;
        }
        (i > start).then_some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Inputs start just past the tag name.
    fn open(bytes: &[u8]) -> Option<usize> {
        tag_end(bytes, 0, false, true)
    }

    #[test]
    fn open_tags() {
        assert_eq!(open(b">"), Some(1));
        assert_eq!(open(b"/>"), Some(2));
        assert_eq!(open(b" a b=1 c='x' d=\"y\" />"), Some(21));
        assert_eq!(open(b" a = 1>"), Some(7));
        assert_eq!(open(b" _a:b.c-d>"), Some(10));
        assert_eq!(open(b"a>"), None, "attributes need preceding whitespace");
        assert_eq!(open(b" 1a>"), None);
        assert_eq!(open(b" a=>"), None, "unquoted values are non-empty");
        assert_eq!(open(b" a=x`y>"), None);
        assert_eq!(open(b" a='x>"), None, "unterminated quote");
        assert_eq!(open(b" / >"), None);
    }

    #[test]
    fn closing_tags_take_only_whitespace() {
        assert_eq!(tag_end(b">", 0, true, true), Some(1));
        assert_eq!(tag_end(b" \t>", 0, true, true), Some(3));
        assert_eq!(tag_end(b" a>", 0, true, true), None);
    }

    #[test]
    fn newlines_gate() {
        assert_eq!(tag_end(b"\na>", 0, false, true), Some(3));
        assert_eq!(tag_end(b"\na>", 0, false, false), None);
        assert_eq!(tag_end(b"\n>", 0, true, true), Some(2));
        assert_eq!(tag_end(b"\n>", 0, true, false), None);
    }
}
