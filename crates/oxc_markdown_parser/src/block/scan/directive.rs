//! Container-directive (`:::name[label]{attrs}`) fence start (micromark-extension-directive).

use crate::syntax::skip_ws;
use crate::syntax::unicode::{is_punctuation, is_whitespace};

use super::is_blank;

/// Opening container-directive fence (micromark-extension-directive):
/// 3+ `:`s, a name, optional `[label]` and `{attributes}`, then only whitespace.
/// A malformed label or attributes rejects the whole fence
/// (the failed attempt leaves `[`/`{` in place, which the whitespace-to-EOL rule then refuses).
/// Returns the fence length.
#[expect(clippy::cast_possible_truncation)] // fence runs fit in-line lengths
pub fn directive_fence_open(tail: &str) -> Option<u32> {
    let bytes = tail.as_bytes();
    let run = bytes.iter().take_while(|&&b| b == b':').count();
    if run < 3 {
        return None;
    }
    let mut i = run + directive_name(&tail[run..])?;
    if bytes.get(i) == Some(&b'[') {
        i += directive_label(&tail[i..])?;
    }
    if bytes.get(i) == Some(&b'{') {
        i += directive_attributes(&tail[i..])?;
    }
    is_blank(&tail[i..]).then_some(run as u32)
}

/// A run of name characters: anything that is not Unicode whitespace or punctuation,
/// except the punctuation `allowed_punct` admits (by position).
/// Returns the byte length.
fn name_run(tail: &str, allowed_punct: impl Fn(usize, char) -> bool) -> usize {
    let mut end = 0;
    for (i, c) in tail.char_indices() {
        if is_whitespace(c) || (is_punctuation(c) && !allowed_punct(i, c)) {
            break;
        }
        end = i + c.len_utf8();
    }
    end
}

/// Directive name: one or more name characters,
/// with `-`/`_` allowed mid-name only (not first, not last).
fn directive_name(tail: &str) -> Option<usize> {
    let end = name_run(tail, |i, c| i > 0 && matches!(c, '-' | '_'));
    (end > 0 && !tail[..end].ends_with(['-', '_'])).then_some(end)
}

/// `[label]` on a fence line (micromark's factory-label with `disallowEol`):
/// empty is fine, `\` escapes `[`/`]`/`\` (capped at 999 escapes),
/// unescaped brackets balance up to 32 deep.
/// Returns the byte length including both brackets; `None` runs into the end of the line.
fn directive_label(tail: &str) -> Option<usize> {
    let bytes = tail.as_bytes();
    debug_assert_eq!(bytes[0], b'[');
    let mut i = 1;
    let mut balance = 0u32;
    let mut escapes = 0u32;
    loop {
        match bytes.get(i)? {
            b']' if balance == 0 => return Some(i + 1),
            b']' => balance -= 1,
            b'[' => {
                balance += 1;
                if balance > 32 {
                    return None;
                }
            }
            b'\\' => {
                if matches!(bytes.get(i + 1), Some(b'[' | b']' | b'\\')) {
                    escapes += 1;
                    if escapes > 999 {
                        return None;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// `{attributes}` on a fence line (micromark's factory-attributes with `disallowEol`):
/// `#id` / `.class` shortcuts, bare names, and `name=value` with unquoted or `"`/`'`-quoted values.
/// Returns the byte length including both braces.
fn directive_attributes(tail: &str) -> Option<usize> {
    let bytes = tail.as_bytes();
    debug_assert_eq!(bytes[0], b'{');
    let mut i = 1;
    loop {
        i = skip_ws(bytes, i, 0);
        match bytes.get(i)? {
            b'}' => return Some(i + 1),
            b'#' | b'.' => {
                // Shortcut value: at least one char;
                // only the first also refuses the chars that end a value.
                i += 1;
                if matches!(bytes.get(i)?, b'#' | b'.' | b'}' | b' ' | b'\t') {
                    return None;
                }
                i = attr_value_end(bytes, i, true)?;
            }
            _ => {
                // Attribute name: like a directive name,
                // but `-`/`_` may also start it and `.`/`:` may continue it, with no trailing restriction.
                let end = i + name_run(&tail[i..], |j, c| {
                    if j == 0 { matches!(c, '-' | '_') } else { matches!(c, '-' | '.' | ':' | '_') }
                });
                if end == i {
                    return None;
                }
                i = end;
                let j = skip_ws(bytes, i, 0);
                if bytes.get(j) != Some(&b'=') {
                    continue; // attribute without value
                }
                i = skip_ws(bytes, j + 1, 0);
                match *bytes.get(i)? {
                    b'}' => return None,
                    q @ (b'"' | b'\'') => {
                        i += 1;
                        while *bytes.get(i)? != q {
                            i += 1;
                        }
                        i += 1;
                        // The quote must be followed by `}` or whitespace
                        if !matches!(bytes.get(i)?, b'}' | b' ' | b'\t') {
                            return None;
                        }
                    }
                    _ => i = attr_value_end(bytes, i, false)?,
                }
            }
        }
    }
}

/// An unquoted attribute (or shortcut) value: scans until `}`/whitespace
/// (shortcuts also stop at `#`/`.`);
/// the chars `"` `'` `<` `=` `>` `` ` `` reject the whole attributes block.
/// Returns the position past the value.
fn attr_value_end(bytes: &[u8], mut i: usize, shortcut: bool) -> Option<usize> {
    loop {
        match bytes.get(i)? {
            b'"' | b'\'' | b'<' | b'=' | b'>' | b'`' => return None,
            b'#' | b'.' if shortcut => return Some(i),
            b'}' | b' ' | b'\t' => return Some(i),
            _ => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_open() {
        assert_eq!(directive_fence_open(":::note"), Some(3));
        assert_eq!(directive_fence_open(":::日本"), Some(3));
        assert_eq!(
            directive_fence_open("::::my-note[Label]{#id .cls key=val k2=\"v 2\"}  "),
            Some(4)
        );
        assert_eq!(directive_fence_open("::x"), None);
        assert_eq!(directive_fence_open(":::"), None, "a closing fence is not an opener");
        assert_eq!(directive_fence_open("::: a"), None);
        assert_eq!(directive_fence_open(":::a b"), None);
    }

    #[test]
    fn name_allows_dash_and_underscore_mid_name_only() {
        assert_eq!(directive_fence_open(":::a_b-c"), Some(3));
        assert_eq!(directive_fence_open(":::-x"), None);
        assert_eq!(directive_fence_open(":::x-"), None);
    }

    #[test]
    fn label_balances_and_escapes() {
        assert_eq!(directive_fence_open(":::a[]"), Some(3));
        assert_eq!(directive_fence_open(":::a[[x]]"), Some(3));
        assert_eq!(directive_fence_open(r":::a[\]]"), Some(3));
        assert_eq!(directive_fence_open(":::a[unclosed"), None);
        assert_eq!(directive_fence_open(":::a[x]]"), None);
    }

    #[test]
    fn attributes() {
        assert_eq!(directive_fence_open(":::a{}"), Some(3));
        assert_eq!(directive_fence_open(":::a{k}"), Some(3), "valueless attribute");
        assert_eq!(directive_fence_open(":::a{ k = v }"), Some(3));
        assert_eq!(directive_fence_open(":::a{k='v'}"), Some(3));
        assert_eq!(directive_fence_open(":::a{key=}"), None, "empty value");
        assert_eq!(directive_fence_open(":::a{.}"), None, "empty shortcut");
        assert_eq!(directive_fence_open(":::a{k=\"v\"x}"), None, "quote must end the value");
        assert_eq!(directive_fence_open(":::a{k=v<}"), None);
        assert_eq!(directive_fence_open(":::a{k=v"), None);
    }
}
