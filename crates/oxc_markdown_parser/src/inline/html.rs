//! Inline raw HTML scanning (CommonMark 6.4).
//! Whitespace inside tags may include line endings
//! (the joined text has no blank lines to worry about).

use crate::syntax::html::tag_end;

/// Scans a raw HTML construct at `pos` (which holds `<`).
/// Returns the position just past it.
pub fn scan(t: &str, pos: usize) -> Option<usize> {
    let bytes = t.as_bytes();
    debug_assert_eq!(bytes[pos], b'<');
    let rest = &t[pos + 1..];

    if let Some(after) = rest.strip_prefix("!--") {
        // `<!-->` and `<!--->` are valid (empty) comments
        if after.starts_with('>') {
            return Some(pos + 1 + 3 + 1);
        }
        if after.starts_with("->") {
            return Some(pos + 1 + 3 + 2);
        }
        let close = after.find("-->")?;
        return Some(pos + 1 + 3 + close + 3);
    }
    if let Some(after) = rest.strip_prefix("![CDATA[") {
        let close = after.find("]]>")?;
        return Some(pos + 1 + 8 + close + 3);
    }
    if let Some(after) = rest.strip_prefix('?') {
        let close = after.find("?>")?;
        return Some(pos + 1 + 1 + close + 2);
    }
    if let Some(after) = rest.strip_prefix('!') {
        if !after.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return None;
        }
        let close = after.find('>')?;
        return Some(pos + 1 + 1 + close + 1);
    }

    // Open or closing tag
    let closing = rest.starts_with('/');
    let mut i = pos + 1 + usize::from(closing);
    let first = *bytes.get(i)?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    i += 1;
    while bytes.get(i).copied().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'-') {
        i += 1;
    }
    tag_end(bytes, i, closing, true)
}

#[cfg(test)]
mod tests {
    use super::scan;

    #[test]
    fn tags() {
        // Past the name, `syntax::html::tag_end` owns the grammar (and its tests)
        assert_eq!(scan("<a>", 0), Some(3));
        assert_eq!(scan("</a-b x=1>", 0), None, "closing tags take no attributes");
        assert_eq!(scan("<1>", 0), None);
        assert_eq!(scan("< a>", 0), None);
        assert_eq!(scan("<a", 0), None);
    }

    #[test]
    fn comments_and_friends() {
        assert_eq!(scan("<!-->", 0), Some(5));
        assert_eq!(scan("<!--->", 0), Some(6));
        assert_eq!(scan("<!-- c -->x", 0), Some(10));
        assert_eq!(scan("<!-- c", 0), None);
        assert_eq!(scan("<![CDATA[x]]>", 0), Some(13));
        assert_eq!(scan("<?php ?>", 0), Some(8));
        assert_eq!(scan("<!DOCTYPE a>", 0), Some(12));
        assert_eq!(scan("<!1>", 0), None);
    }
}
