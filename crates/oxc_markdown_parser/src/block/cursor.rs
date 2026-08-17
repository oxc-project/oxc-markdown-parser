//! A cursor over one physical line.
//!
//! Tracks byte position and logical column together.
//! Tabs advance to the next 4-column stop measured from the physical line start (container prefixes included),
//! so consuming a prefix can stop inside a tab;
//! the leftover columns live in `partial` as virtual spaces in front of `pos`.

use crate::pos::{Segment, Span};

#[derive(Clone, Copy)]
pub struct Cursor<'s> {
    line: &'s str,
    line_start: u32,
    /// Byte position within the line.
    pub pos: usize,
    /// Logical column (tabs expanded).
    pub col: usize,
    /// Virtual spaces pending in front of `pos` from a partially consumed tab.
    pub partial: u8,
}

impl<'s> Cursor<'s> {
    pub fn new(line: &'s str, line_start: u32) -> Self {
        Self { line, line_start, pos: 0, col: 0, partial: 0 }
    }

    pub fn tail(&self) -> &'s str {
        &self.line[self.pos..]
    }

    /// Byte offset in the source.
    #[expect(clippy::cast_possible_truncation)] // sources are bounded to u32
    pub fn offset(&self) -> u32 {
        self.line_start + self.pos as u32
    }

    /// The rest of the line as a leaf-content segment.
    /// The span from here to the end of the line, trailing spaces/tabs excluded
    /// (the extent of a heading, thematic break or setext underline).
    pub fn trimmed_span(&self) -> Span {
        Span::at(self.offset(), 0..self.tail().trim_end_matches([' ', '\t']).len())
    }

    pub fn segment(&self, line_content_end: u32) -> Segment {
        Segment::new(Span::new(self.offset().min(line_content_end), line_content_end), self.partial)
    }

    /// Columns of whitespace available at the cursor (without consuming).
    pub fn indent_cols(&self) -> usize {
        let mut cols = usize::from(self.partial);
        let mut col = self.col + cols;
        for b in self.line[self.pos..].bytes() {
            match b {
                b' ' => {
                    cols += 1;
                    col += 1;
                }
                b'\t' => {
                    let width = 4 - col % 4;
                    cols += width;
                    col += width;
                }
                _ => break,
            }
        }
        cols
    }

    /// Consumes exactly `n` columns of whitespace
    /// (caller checked availability via [`indent_cols`](Self::indent_cols)).
    #[expect(clippy::cast_possible_truncation)] // partial-tab widths are < 4
    pub fn advance_cols(&mut self, mut n: usize) {
        while n > 0 {
            if self.partial > 0 {
                let take = usize::from(self.partial).min(n);
                self.partial -= take as u8;
                self.col += take;
                n -= take;
                continue;
            }
            match self.line.as_bytes().get(self.pos) {
                Some(b' ') => {
                    self.pos += 1;
                    self.col += 1;
                    n -= 1;
                }
                Some(b'\t') => {
                    let width = 4 - self.col % 4;
                    self.pos += 1;
                    if width > n {
                        self.partial = (width - n) as u8;
                        self.col += n;
                        n = 0;
                    } else {
                        self.col += width;
                        n -= width;
                    }
                }
                _ => break,
            }
        }
    }

    /// Consumes up to `max` columns of leading whitespace.
    pub fn skip_indent_up_to(&mut self, max: usize) {
        let available = self.indent_cols().min(max);
        self.advance_cols(available);
    }

    /// The position where a flow construct could start on this line:
    /// past up-to-3 columns of indent, with no partial tab straddling the boundary.
    /// `None` when the line is indented-code-deep.
    pub fn flow_start(mut self) -> Option<Self> {
        if self.indent_cols() >= 4 {
            return None;
        }
        self.skip_indent_up_to(3);
        (self.partial == 0).then_some(self)
    }

    /// Consumes all leading whitespace (dropping any partial-tab padding).
    pub fn skip_all_ws(&mut self) {
        let available = self.indent_cols();
        self.advance_cols(available);
        self.partial = 0;
    }

    /// Advances over `n` bytes of known-ASCII content.
    pub fn bump(&mut self, n: usize) {
        debug_assert_eq!(self.partial, 0);
        self.pos += n;
        self.col += n;
    }

    /// Consumes one column of whitespace if present (the optional space after `>`).
    pub fn eat_one_space(&mut self) {
        if self.partial > 0 || matches!(self.line.as_bytes().get(self.pos), Some(b' ' | b'\t')) {
            self.advance_cols(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabs expand to 4-column stops from the physical line start.
    #[test]
    fn tab_columns() {
        let cur = Cursor::new("\tx", 0);
        assert_eq!(cur.indent_cols(), 4);
        let cur = Cursor::new("  \tx", 0);
        assert_eq!(cur.indent_cols(), 4);
        let cur = Cursor::new("    \tx", 0);
        assert_eq!(cur.indent_cols(), 8);
    }

    /// Stopping inside a tab leaves the remaining columns as `partial`,
    /// and later consumption drains `partial` before touching bytes.
    #[test]
    fn partial_tab() {
        let mut cur = Cursor::new("\tx", 0);
        cur.advance_cols(1);
        assert_eq!((cur.pos, cur.col, cur.partial), (1, 1, 3));
        assert_eq!(cur.indent_cols(), 3);
        assert_eq!(cur.segment(2), Segment::new(Span::new(1, 2), 3));

        cur.advance_cols(3);
        assert_eq!((cur.pos, cur.col, cur.partial), (1, 4, 0));
        assert_eq!(cur.tail(), "x");
    }

    /// Up to 3 columns is a flow position; 4 or more (tabs included) is indented code.
    #[test]
    fn flow_start() {
        assert_eq!(Cursor::new("   x", 0).flow_start().map(|c| c.pos), Some(3));
        assert!(Cursor::new("    x", 0).flow_start().is_none());
        assert!(Cursor::new("\tx", 0).flow_start().is_none());
    }

    #[test]
    fn spans_and_offsets() {
        let mut cur = Cursor::new("  # h  ", 100);
        cur.skip_indent_up_to(3);
        assert_eq!(cur.offset(), 102);
        assert_eq!(cur.trimmed_span(), Span::new(102, 105));
        cur.eat_one_space();
        assert_eq!(cur.pos, 2, "eat_one_space only eats whitespace");
        cur.bump(1);
        cur.eat_one_space();
        assert_eq!(cur.tail(), "h  ");
    }
}
