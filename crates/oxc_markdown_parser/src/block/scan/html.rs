//! HTML block starts and end conditions (CommonMark 4.6).

use crate::syntax::html::tag_end;

use super::is_blank;

const HTML_BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

const HTML_VERBATIM_TAGS: &[&str] = &["pre", "script", "style", "textarea"];

/// HTML block start (CommonMark 4.6).
/// Returns the block type 1–7.
/// Type 7 never interrupts a paragraph, so it is skipped when `paragraph_open`.
pub fn html_block_start(tail: &str, paragraph_open: bool) -> Option<u8> {
    let bytes = tail.as_bytes();
    if *bytes.first()? != b'<' {
        return None;
    }
    let rest = &tail[1..];

    // Type 2–5: comment / processing instruction / declaration / CDATA
    if rest.starts_with("!--") {
        return Some(2);
    }
    if rest.starts_with('?') {
        return Some(3);
    }
    if rest.starts_with("![CDATA[") {
        return Some(5);
    }
    if rest.starts_with('!') && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Some(4);
    }

    // Tag name (types 1, 6, 7), optionally a closing tag.
    let closing = rest.starts_with('/');
    let name_start = usize::from(closing);
    let name_len =
        rest[name_start..].bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'-').count();
    if name_len == 0 {
        return None;
    }
    let name = &rest[name_start..name_start + name_len];
    let after = &rest[name_start + name_len..];
    let is_tag_in = |tags: &[&str]| tag_in(tags, name);

    if !closing
        && is_tag_in(HTML_VERBATIM_TAGS)
        && (after.is_empty() || after.starts_with([' ', '\t', '>']))
    {
        return Some(1);
    }
    if is_tag_in(HTML_BLOCK_TAGS)
        && (after.is_empty()
            || after.starts_with([' ', '\t', '>'])
            || (!closing && after.starts_with("/>")))
    {
        return Some(6);
    }

    // Type 7: a single complete tag, then only whitespace.
    // No verbatim-tag exclusion here: micromark bars raw names only as plain open tags
    // (`!slash && !closingTag` in `tagName`), which type 1 above already took
    // (`</pre>` and `<pre/>` reach this point and are type 7).
    if !paragraph_open
        && tag_end(after.as_bytes(), 0, closing, false).is_some_and(|end| is_blank(&after[end..]))
    {
        return Some(7);
    }
    None
}

/// Case-insensitive tag-name membership (HTML tag names are ASCII).
fn tag_in(tags: &[&str], name: &str) -> bool {
    tags.iter().any(|tag| tag.eq_ignore_ascii_case(name))
}

/// Whether `tail` contains the end condition of an HTML block of `kind`
/// (types 1–5 end mid-line; 6 and 7 end at blank lines, handled by the engine).
pub fn html_block_end(tail: &str, kind: u8) -> bool {
    match kind {
        1 => {
            // micromark's `continuationRawEndTag`:
            // `</` + a whole verbatim tag name with `>` immediately after,
            // anywhere in the line, case-insensitively.
            // A bare `</pre`, a longer name (`</prefoo>`)
            // or an interposed space (`</pre >`) do not end the block.
            let bytes = tail.as_bytes();
            (0..bytes.len()).any(|i| {
                if bytes[i] != b'<' || bytes.get(i + 1) != Some(&b'/') {
                    return false;
                }
                // Cap the name scan at 9: the longest raw name (`textarea`) is 8 bytes,
                // and at 9 the next byte is another letter, so both checks below fail the same way.
                let name =
                    bytes[i + 2..].iter().take(9).take_while(|b| b.is_ascii_alphabetic()).count();
                bytes.get(i + 2 + name) == Some(&b'>')
                    && tag_in(HTML_VERBATIM_TAGS, &tail[i + 2..i + 2 + name])
            })
        }
        2 => tail.contains("-->"),
        3 => tail.contains("?>"),
        4 => tail.contains('>'),
        5 => tail.contains("]]>"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_starts_by_type() {
        assert_eq!(html_block_start("<script>", false), Some(1));
        assert_eq!(html_block_start("<PRE lang=x>", false), Some(1));
        assert_eq!(html_block_start("<textarea", false), Some(1));
        assert_eq!(html_block_start("<!-- c", false), Some(2));
        assert_eq!(html_block_start("<?php", false), Some(3));
        assert_eq!(html_block_start("<!DOCTYPE html>", false), Some(4));
        assert_eq!(html_block_start("<![CDATA[x", false), Some(5));
        assert_eq!(html_block_start("<div", false), Some(6));
        assert_eq!(html_block_start("</DIV>", false), Some(6));
        assert_eq!(html_block_start("<div/>", false), Some(6));
        assert_eq!(html_block_start("<div>text", false), Some(6));
        assert_eq!(html_block_start("<x-tag a=\"1\">  ", false), Some(7));
        assert_eq!(html_block_start("<x-tag a=\"1\">", true), None, "type 7 never interrupts");
        assert_eq!(html_block_start("<x>y", false), None, "type 7 needs only whitespace after");
        assert_eq!(html_block_start("<div/ >", false), None);
        assert_eq!(html_block_start("<!1", false), None);
        assert_eq!(html_block_start("<>", false), None);
        assert_eq!(html_block_start("text", false), None);
    }

    /// Verbatim names are only type 1 as plain open tags; otherwise they fall through to type 7.
    #[test]
    fn verbatim_tags_closing_or_self_closing_are_type_7() {
        assert_eq!(html_block_start("</pre>", false), Some(7));
        assert_eq!(html_block_start("<pre/>", false), Some(7));
    }

    #[test]
    fn block_ends() {
        assert!(html_block_end("x</PRE>", 1));
        assert!(html_block_end("</textarea>y", 1));
        assert!(!html_block_end("</pre", 1));
        assert!(!html_block_end("</prefoo>", 1));
        assert!(!html_block_end("</pre >", 1));
        assert!(!html_block_end("</div>", 1));
        assert!(html_block_end("a --> b", 2));
        assert!(html_block_end("?>", 3));
        assert!(html_block_end("x>", 4));
        assert!(html_block_end("]]>", 5));
        assert!(!html_block_end("</div>", 6), "types 6/7 end on blank lines only");
    }
}
