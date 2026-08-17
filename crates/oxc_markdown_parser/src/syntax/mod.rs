//! Grammar shared by the block and inline phases.
//!
//! Anything both phases (or the parser and its consumers) must agree on lives here exactly once:
//! escape and entity decoding, label normalization, link-target productions,
//! liquid delimiters, HTML tag grammar, Unicode character classes.
//!
//! `decode` and `label` are re-exported at the crate root as public API.

pub mod decode;
mod entities;
pub mod html;
pub mod label;
pub mod link_target;
pub mod liquid;
pub mod unicode;

/// Whether the backslash at `p` escapes the next byte
/// (ASCII punctuation only; never a line ending).
pub fn escapes_next(bytes: &[u8], p: usize) -> bool {
    bytes.get(p + 1).is_some_and(u8::is_ascii_punctuation)
}

/// Skips spaces/tabs and up to `max_newlines` line endings.
pub fn skip_ws(bytes: &[u8], mut p: usize, max_newlines: usize) -> usize {
    let mut newlines = 0;
    while let Some(&c) = bytes.get(p) {
        match c {
            b' ' | b'\t' => p += 1,
            b'\n' if newlines < max_newlines => {
                newlines += 1;
                p += 1;
            }
            _ => break,
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_next_only_ascii_punctuation() {
        assert!(escapes_next(br"\*", 0));
        assert!(!escapes_next(br"\a", 0));
        assert!(!escapes_next(b"\\\n", 0));
        assert!(!escapes_next(br"\", 0), "nothing to escape at the end");
    }

    #[test]
    fn skip_ws_caps_newlines() {
        assert_eq!(skip_ws(b"  \t x", 0, 0), 4);
        assert_eq!(skip_ws(b" \n x", 0, 0), 1);
        assert_eq!(skip_ws(b" \n x", 0, 1), 3);
        assert_eq!(skip_ws(b"\n\n x", 0, 1), 1);
        assert_eq!(skip_ws(b"x", 0, 1), 0);
    }
}
