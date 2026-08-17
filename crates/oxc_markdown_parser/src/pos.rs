use std::{borrow::Cow, ops::Range};

/// Span represents a range of a piece of source code.
/// It counts by byte offset, so it's 0-based.
///
/// Offsets are `u32` (matching oxc convention);
/// sources larger than 4 GiB are rejected by the parser up front.
///
/// Every AST node carries spans into the original source,
/// including inline nodes, whose offsets are never derived from a rebuilt buffer.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Span {
    /// Start offset. (Inclusive)
    pub start: u32,
    /// End offset. (Exclusive)
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// A zero-width span at the given offset. Covers no characters, but its
    /// position may still be meaningful (e.g. a marker between two tokens).
    pub fn empty(at: u32) -> Self {
        Self { start: at, end: at }
    }

    /// A span for an in-line byte range relative to `base`.
    #[expect(clippy::cast_possible_truncation)] // in-line ranges
    pub fn at(base: u32, range: Range<usize>) -> Self {
        Self::new(base + range.start as u32, base + range.end as u32)
    }

    /// The source text this span covers.
    pub fn slice(self, source: &str) -> &str {
        &source[self.start as usize..self.end as usize]
    }

    /// Whether the span covers no characters.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// One logical line of a leaf block:
/// the source line with container prefixes (`>`, list indents) already stripped.
///
/// `padding` is the number of virtual spaces in front of `span` left over
/// when prefix consumption stopped inside a tab
/// (tabs advance to 4-column stops, so a prefix can end mid-tab).
/// Consumers render `padding` spaces, then the slice.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Segment {
    pub span: Span,
    pub padding: u8,
}

impl Segment {
    pub fn new(span: Span, padding: u8) -> Self {
        Self { span, padding }
    }

    /// The logical text of a multi-line construct:
    /// pieces joined with `\n`, padding rendered as spaces.
    /// Borrows for the common single-line case.
    /// This is what labels are normalized from and what a multi-line title decodes from;
    /// a raw source slice across the pieces would include the container prefixes between them.
    pub fn join<'s>(source: &'s str, pieces: &[Segment]) -> Cow<'s, str> {
        match pieces {
            [single] if single.padding == 0 => Cow::Borrowed(single.span.slice(source)),
            _ => {
                let mut out = String::new();
                for (i, piece) in pieces.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    out.extend(std::iter::repeat_n(' ', usize::from(piece.padding)));
                    out.push_str(piece.span.slice(source));
                }
                Cow::Owned(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_basics() {
        let span = Span::at(10, 2..5);
        assert_eq!(span, Span::new(12, 15));
        assert!(!span.is_empty());
        assert!(Span::empty(3).is_empty());
        assert_eq!(Span::new(2, 5).slice("abcdefg"), "cde");
    }

    /// A single unpadded piece borrows; anything else renders padding and joins with `\n`.
    #[test]
    fn segment_join() {
        let source = "> a\n>   b\n";
        let a = Segment::new(Span::new(2, 3), 0);
        let b = Segment::new(Span::new(6, 9), 2);
        assert!(matches!(Segment::join(source, &[a]), Cow::Borrowed("a")));
        assert_eq!(Segment::join(source, &[b]), "    b");
        assert_eq!(Segment::join(source, &[a, b]), "a\n    b");
    }
}
