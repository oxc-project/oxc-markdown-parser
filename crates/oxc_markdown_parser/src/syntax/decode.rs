//! Backslash-escape and character-reference decoding.
//!
//! The AST carries raw source spans only;
//! this is the shared decoder that turns a raw slice into its cooked text.
//! It is public because a printer needs the very same recognition to judge escape removal safely
//! (an escape whose removal forms an entity changes meaning).

use std::borrow::Cow;

use super::entities;
use crate::syntax::escapes_next;
use crate::{ast::Destination, pos::Segment};

/// Decodes backslash escapes and entities.
/// `escapes: false` decodes only entities
/// (autolink contents; backslash escapes don't work there).
pub fn decode(raw: &str, escapes: bool) -> Cow<'_, str> {
    if !raw.bytes().any(|b| b == b'\\' || b == b'&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if escapes && escapes_next(bytes, i) => {
                out.push(bytes[i + 1] as char);
                i += 2;
            }
            b'&' => {
                if let Some((text, end)) = entity(raw, i) {
                    out.push_str(&text);
                    i = end;
                } else {
                    out.push('&');
                    i += 1;
                }
            }
            _ => {
                let Some(c) = raw[i..].chars().next() else { break };
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    Cow::Owned(out)
}

/// Recognizes a character reference at `at` (which holds `&`).
/// Returns the replacement text and the position just past the `;`.
pub fn entity(raw: &str, at: usize) -> Option<(Cow<'static, str>, usize)> {
    let rest = &raw.as_bytes()[at + 1..];
    if rest.first() == Some(&b'#') {
        let (digits, radix, skip) = if matches!(rest.get(1), Some(b'x' | b'X')) {
            (&rest[2..], 16u32, 2)
        } else {
            (&rest[1..], 10u32, 1)
        };
        let max_len = if radix == 16 { 6 } else { 7 };
        let len = digits
            .iter()
            .take_while(|b| b.is_ascii_digit() || (radix == 16 && b.is_ascii_hexdigit()))
            .count();
        if len == 0 || len > max_len || digits.get(len) != Some(&b';') {
            return None;
        }
        let text = std::str::from_utf8(&digits[..len]).ok()?;
        let value = u32::from_str_radix(text, radix).ok()?;
        let c = match char::from_u32(value) {
            Some(c) if value != 0 => c,
            _ => '\u{FFFD}',
        };
        return Some((Cow::Owned(c.to_string()), at + 1 + skip + len + 1));
    }
    // Named: letters/digits then `;`. micromark caps the scan at 31 chars;
    // any larger bound behaves the same because the lookup rejects unknown
    // names and the longest real entity is 31 chars. 48 leaves slack.
    let len = rest.iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    if len == 0 || len > 48 || rest.get(len) != Some(&b';') {
        return None;
    }
    let name = std::str::from_utf8(&rest[..len]).ok()?;
    let value = entities::lookup(name)?;
    Some((Cow::Borrowed(value), at + 1 + len + 1))
}

/// The cooked value of a link or definition title held as line pieces:
/// quotes stripped, continuation lines' leading whitespace removed (micromark strips all of it),
/// escapes and entities decoded.
pub fn title(source: &str, pieces: &[Segment]) -> String {
    let joined = join_stripped(source, pieces, usize::MAX);
    decode(&joined[1..joined.len() - 1], true).into_owned()
}

/// Inline HTML as micromark's HTML compiler emits it.
///
/// The pieces joined with `\n`, continuation lines minus up to 3 columns of leading whitespace.
/// For the mdast `value` (what a formatter prints) use [`Segment::join`]: it keeps that whitespace.
pub fn html_inline(source: &str, pieces: &[Segment]) -> String {
    join_stripped(source, pieces, 3)
}

/// Pieces joined with `\n`, each continuation piece minus up to `max` leading whitespace columns
/// (padding counts as spaces).
/// Tabs are counted from the piece start, not from the source column micromark uses,
/// and a partially consumed tab is not re-emitted as spaces:
/// `<b\n\t\tc=1>` gives `\tc=1` here and ` \tc=1` in micromark.
/// Exact tab columns would need the segment's start column, which `Segment` does not carry;
/// the shape (a tab-indented continuation line of an inline tag) is rare.
fn join_stripped(source: &str, pieces: &[Segment], max: usize) -> String {
    let mut out = String::new();
    for (i, piece) in pieces.iter().enumerate() {
        let mut text = piece.span.slice(source);
        let mut padding = usize::from(piece.padding);
        if i > 0 {
            out.push('\n');
            let mut cols = padding.min(max);
            padding -= cols;
            while cols < max {
                match text.as_bytes().first() {
                    Some(b' ') => cols += 1,
                    Some(b'\t') => cols += 4 - cols % 4,
                    _ => break,
                }
                text = &text[1..];
            }
        }
        out.extend(std::iter::repeat_n(' ', padding));
        out.push_str(text);
    }
    out
}

/// The cooked value of a link, image or definition destination (mdast's `url`):
/// angle brackets stripped, escapes and entities decoded.
pub fn destination<'s>(source: &'s str, destination: &Destination) -> Cow<'s, str> {
    let raw = destination.span.slice(source);
    let raw = if destination.angle_bracketed { &raw[1..raw.len() - 1] } else { raw };
    decode(raw, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;

    #[test]
    fn decodes_escapes_and_entities() {
        assert!(matches!(decode("plain", true), Cow::Borrowed(_)));
        assert_eq!(decode(r"\*a\\b\c", true), r"*a\b\c");
        assert_eq!(decode("&amp; &#35; &#x41; &unknown; &", true), "& # A &unknown; &");
        // `escapes: false` keeps backslashes but still decodes entities
        assert_eq!(decode(r"\&amp;", false), r"\&");
        assert_eq!(decode(r"\&amp;", true), "&amp;");
    }

    #[test]
    fn entity_recognition() {
        assert_eq!(entity("&amp;x", 0), Some((Cow::Borrowed("&"), 5)));
        assert_eq!(entity("&#0;", 0), Some((Cow::Borrowed("\u{FFFD}"), 4)));
        assert_eq!(entity("&#xD800;", 0), Some((Cow::Borrowed("\u{FFFD}"), 8)));
        assert_eq!(entity("&amp", 0), None);
        assert_eq!(entity("&#;", 0), None);
        assert_eq!(entity("&#12345678;", 0), None, "more than 7 decimal digits");
    }

    #[test]
    fn title_and_html_inline() {
        let source = "\"a\n    b\"";
        let pieces = [Segment::new(Span::new(0, 2), 0), Segment::new(Span::new(3, 9), 0)];
        assert_eq!(title(source, &pieces), "a\nb");

        // Continuation lines lose up to 3 columns; padding counts as columns
        let source = "<b\n     c>";
        let pieces = [Segment::new(Span::new(0, 2), 0), Segment::new(Span::new(3, 10), 0)];
        assert_eq!(html_inline(source, &pieces), "<b\n  c>");
        let pieces = [Segment::new(Span::new(0, 2), 0), Segment::new(Span::new(8, 10), 2)];
        assert_eq!(html_inline(source, &pieces), "<b\nc>");
    }
}
