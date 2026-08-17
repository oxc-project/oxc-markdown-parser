//! GFM autolink literals (`www.…`, `http[s]://…`, bare emails),
//! a port of micromark-extension-gfm-autolink-literal's state machine:
//! previous-character gates, the underscore rule for the domain's last two segments,
//! balanced-paren paths, and trailing-punctuation trimming via lookahead.

// Non-boundary breaks below use [`is_whitespace`]:
// micromark checks `markdownLineEndingOrSpace(code) || unicodeWhitespace(code)`,
// which is exactly JS `\s`, not Rust's `char::is_whitespace`.
use crate::syntax::unicode::{is_punctuation, is_whitespace};

/// Attempts a literal at `pos`, `prev` is the character before it.
/// Returns the end position.
pub fn scan(t: &str, pos: usize, prev: Option<char>) -> Option<usize> {
    match t.as_bytes()[pos] {
        b'h' | b'H' => email(t, pos, prev).or_else(|| http(t, pos, prev)),
        b'w' | b'W' => email(t, pos, prev).or_else(|| www(t, pos, prev)),
        _ => email(t, pos, prev),
    }
}

/// The kind of an autolink literal.
///
/// Derived with the same disambiguation the parser used
/// (an `@`-run that scans as a full email wins over `www.`);
/// public so renderers/printers derive the href scheme identically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralKind {
    /// `http://` / `https://`: the text already carries its scheme.
    Http,
    /// `www.…`: prepend `http://`.
    Www,
    /// Bare email: prepend `mailto:`.
    Email,
}

/// Classifies a matched autolink literal.
pub fn literal_kind(text: &str) -> LiteralKind {
    let b = text.as_bytes();
    // Byte-wise compare: domains accept non-ASCII,
    // so a `str` slice at 7/8 could split a character (`www.aa日x`) and panic.
    if b.len() >= 7 && b[..7].eq_ignore_ascii_case(b"http://")
        || b.len() >= 8 && b[..8].eq_ignore_ascii_case(b"https://")
    {
        return LiteralKind::Http;
    }
    if b.contains(&b'@') && email(text, 0, None) == Some(text.len()) {
        return LiteralKind::Email;
    }
    LiteralKind::Www
}

fn is_atext(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.' | '_')
}

fn char_at(t: &str, i: usize) -> Option<char> {
    t.get(i..).and_then(|s| s.chars().next())
}

fn email(t: &str, pos: usize, prev: Option<char>) -> Option<usize> {
    if prev.is_some_and(|c| c == '/' || is_atext(c)) {
        return None;
    }
    let bytes = t.as_bytes();
    let mut i = pos;
    while bytes.get(i).copied().is_some_and(|b| is_atext(b as char) && b.is_ascii()) {
        i += 1;
    }
    if i == pos || bytes.get(i) != Some(&b'@') {
        return None;
    }
    i += 1;
    let (mut data, mut dot) = (false, false);
    loop {
        match bytes.get(i) {
            Some(b'.') => {
                // The dot joins the domain only when followed by more of it
                if !bytes.get(i + 1).copied().is_some_and(|b| b.is_ascii_alphanumeric()) {
                    break;
                }
                dot = true;
                i += 1;
            }
            Some(b'-' | b'_') => {
                data = true;
                i += 1;
            }
            Some(&b) if b.is_ascii_alphanumeric() => {
                data = true;
                i += 1;
            }
            _ => break,
        }
    }
    // The domain must end with a letter
    (data && dot && bytes.get(i - 1).copied().is_some_and(|b| b.is_ascii_alphabetic())).then_some(i)
}

fn www(t: &str, pos: usize, prev: Option<char>) -> Option<usize> {
    // micromark's `previousWww`: a few ASCII punctuation marks
    // or `markdownLineEndingOrSpace` (ASCII space/tab/line ending only, so NBSP blocks the link).
    if !matches!(prev, None | Some('(' | '*' | '_' | '[' | ']' | '~' | ' ' | '\t' | '\n')) {
        return None;
    }
    let bytes = t.as_bytes();
    let prefix = bytes.get(pos..pos + 4)?;
    if !prefix[..3].iter().all(|b| matches!(b, b'w' | b'W')) || prefix[3] != b'.' {
        return None;
    }
    let mut i = pos + 4;
    bytes.get(i)?;
    domain(t, &mut i)?;
    path(t, &mut i);
    Some(i)
}

fn http(t: &str, pos: usize, prev: Option<char>) -> Option<usize> {
    if prev.is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let bytes = t.as_bytes();
    let mut i = pos;
    while bytes.get(i).copied().is_some_and(|b| b.is_ascii_alphabetic()) && i - pos < 5 {
        i += 1;
    }
    if !t[pos..i].eq_ignore_ascii_case("http") && !t[pos..i].eq_ignore_ascii_case("https") {
        return None;
    }
    if t.get(i..i + 3) != Some("://") {
        return None;
    }
    i += 3;
    let next = char_at(t, i)?;
    // micromark's `afterProtocol`: ASCII controls only (`asciiControl`)
    if is_whitespace(next) || next.is_ascii_control() || is_punctuation(next) {
        return None;
    }
    domain(t, &mut i)?;
    path(t, &mut i);
    Some(i)
}

/// Domain run;
/// fails on an underscore in the last two dot-separated segments
/// or when nothing substantial was consumed.
fn domain(t: &str, i: &mut usize) -> Option<()> {
    let (mut underscore_last, mut underscore_prev, mut seen) = (false, false, false);
    let mut not_trail_until = 0;
    while let Some(c) = char_at(t, *i) {
        match c {
            '.' | '_' => {
                if *i >= not_trail_until {
                    match trail_check(t, *i) {
                        Ok(()) => break,
                        Err(scanned_to) => not_trail_until = scanned_to,
                    }
                }
                if c == '_' {
                    underscore_last = true;
                } else {
                    underscore_prev = underscore_last;
                    underscore_last = false;
                }
                *i += 1;
            }
            c if is_whitespace(c) => break,
            c if c != '-' && is_punctuation(c) => break,
            c => {
                seen = true;
                *i += c.len_utf8();
            }
        }
    }
    (!underscore_prev && !underscore_last && seen).then_some(())
}

/// Path run: parens balance;
/// a trail-class character ends the path when everything after it (to whitespace/EOF/`<`) is trail.
fn path(t: &str, i: &mut usize) {
    let (mut open, mut close) = (0usize, 0usize);
    let mut not_trail_until = 0;
    while let Some(c) = char_at(t, *i) {
        match c {
            '(' => {
                open += 1;
                *i += 1;
            }
            ')' if close < open => {
                close += 1;
                *i += 1;
            }
            '!' | '"' | '&' | '\'' | ')' | '*' | ',' | '.' | ':' | ';' | '<' | '?' | ']' | '_'
            | '~' => {
                if *i >= not_trail_until {
                    match trail_check(t, *i) {
                        Ok(()) => break,
                        Err(scanned_to) => not_trail_until = scanned_to,
                    }
                }
                if c == ')' {
                    close += 1;
                }
                *i += 1;
            }
            c if is_whitespace(c) => break,
            c => *i += c.len_utf8(),
        }
    }
}

/// Whether everything from `i` to whitespace/EOF/`<` is trailing punctuation (and thus not part of the link).
/// `Err` carries how far the scan reached.
/// Failed check fails identically for every trail-class position before that point,
/// so callers memoize it to stay linear on long punctuation runs.
/// (The `&…;` shape trims regardless of whether it names a real entity — deliberately unlike `decode::entity`.)
fn trail_check(t: &str, mut i: usize) -> Result<(), usize> {
    let bytes = t.as_bytes();
    loop {
        let Some(c) = char_at(t, i) else { return Ok(()) };
        match c {
            '<' => return Ok(()),
            c if is_whitespace(c) => return Ok(()),
            '!' | '"' | '\'' | ')' | '*' | ',' | '.' | ':' | ';' | '?' | '_' | '~' => i += 1,
            '&' => {
                i += 1;
                let run = bytes[i..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
                if run == 0 || bytes.get(i + run) != Some(&b';') {
                    return Err(i + run);
                }
                i += run + 1;
            }
            ']' => {
                i += 1;
                match char_at(t, i) {
                    None | Some('(' | '[') => return Ok(()),
                    Some(c) if is_whitespace(c) => return Ok(()),
                    _ => {}
                }
            }
            _ => return Err(i),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_kinds() {
        assert_eq!(literal_kind("http://x.y"), LiteralKind::Http);
        assert_eq!(literal_kind("HTTPS://x.y"), LiteralKind::Http);
        assert_eq!(literal_kind("www.x.y"), LiteralKind::Www);
        assert_eq!(literal_kind("a@b.cd"), LiteralKind::Email);
        assert_eq!(literal_kind("www.aa日x"), LiteralKind::Www, "non-ASCII must not panic");
    }

    #[test]
    fn scans_with_trailing_punctuation_trimmed() {
        assert_eq!(scan("www.x.y.", 0, None), Some(7));
        assert_eq!(scan("http://x.y/p(q)", 0, None), Some(15));
        assert_eq!(scan("http://x.y/p)", 0, None), Some(12), "unbalanced paren excluded");
        assert_eq!(scan("a@b.cd,", 0, None), Some(6));
    }

    #[test]
    fn previous_character_gates() {
        assert_eq!(scan("www.x.y", 0, Some(' ')), Some(7));
        assert_eq!(scan("www.x.y", 0, Some('_')), Some(7), "micromark's `previousWww` admits `_`");
        assert_eq!(scan("www.x.y", 0, Some('a')), None);
        assert_eq!(scan("www.x.y", 0, Some('\u{A0}')), None, "NBSP is not a markdown space");
        assert_eq!(scan("http://x.y", 0, Some('a')), None);
        assert_eq!(scan("http://x.y", 0, Some('1')), Some(10));
        assert_eq!(scan("a@b.cd", 0, Some('/')), None);
        assert_eq!(scan("a@b.cd", 0, Some('.')), None, "atext before an email is part of it");
    }
}
