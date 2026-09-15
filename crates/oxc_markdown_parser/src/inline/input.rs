//! The inline phase's view of a leaf's content:
//! the logical lines joined with `\n`, plus a per-line map back to source offsets.
//!
//! Within a line the joined bytes are the source bytes;
//! across lines (container prefixes, padding) only the map is exact,
//! so node spans always go through it.

use std::ops::Range;

use oxc_allocator::{Allocator, ArenaVec};

use crate::block::IrSeg;
use crate::pos::{Segment, Span};

pub struct Input {
    pub text: String,
    lines: Vec<Line>,
}

#[derive(Clone, Copy)]
struct Line {
    /// Joined offset of its first byte (padding first, then the content).
    joined: usize,
    /// Virtual padding bytes at its head (a partially consumed tab).
    padding: u8,
    /// Source offset of its content.
    source: u32,
}

impl Input {
    pub fn new(source: &str, segments: &[IrSeg]) -> Self {
        let len = segments
            .iter()
            .map(|s| (s.seg.span.end - s.seg.span.start) as usize + usize::from(s.seg.padding) + 1)
            .sum();
        let mut text = String::with_capacity(len);
        let mut lines = Vec::with_capacity(segments.len());
        for (i, s) in segments.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            lines.push(Line {
                joined: text.len(),
                padding: s.seg.padding,
                source: s.seg.span.start,
            });
            for _ in 0..s.seg.padding {
                text.push(' ');
            }
            text.push_str(s.seg.span.slice(source));
        }
        Self { text, lines }
    }

    fn line_of(&self, joined: usize) -> usize {
        self.lines.partition_point(|line| line.joined <= joined).saturating_sub(1)
    }

    /// Joined offset of line `i`'s separator (or the text's end).
    fn line_end(&self, i: usize) -> usize {
        self.lines.get(i + 1).map_or(self.text.len(), |next| next.joined - 1)
    }

    /// Source offset of a joined byte; a separator `\n` maps to the next line's start,
    /// the end of the text to the end of the last line.
    #[expect(clippy::cast_possible_truncation)] // in-line lengths
    pub fn offset(&self, joined: usize) -> u32 {
        let i = self.line_of(joined);
        if let Some(next) = self.lines.get(i + 1)
            && joined + 1 >= next.joined
        {
            return next.source;
        }
        let line = self.lines[i];
        let content = line.joined + usize::from(line.padding);
        line.source + joined.saturating_sub(content) as u32
    }

    /// Source span of a joined range.
    /// Exact for ranges that do not end on a separator (which node ranges never do).
    pub fn span(&self, r: Range<usize>) -> Span {
        if r.is_empty() {
            return Span::empty(self.offset(r.start));
        }
        Span::new(self.offset(r.start), self.offset(r.end - 1) + 1)
    }

    /// Content segments of a joined range, one per line piece, straight into the arena
    /// (multi-line constructs cross container prefixes, so a single span can't cover them faithfully;
    /// virtual padding from partially consumed tabs is preserved as [`Segment::padding`]).
    pub fn pieces_in<'a>(
        &self,
        r: Range<usize>,
        allocator: &'a Allocator,
    ) -> ArenaVec<'a, Segment> {
        ArenaVec::from_iter_in(self.pieces_iter(r), &allocator)
    }

    /// Std-vector form of [`pieces_in`](Self::pieces_in), for the block-phase IR (definitions).
    pub fn pieces(&self, r: Range<usize>) -> Vec<Segment> {
        self.pieces_iter(r).collect()
    }

    /// One piece per line the range touches
    /// (a range ending right after a separator has an empty piece on the next line).
    fn pieces_iter(&self, r: Range<usize>) -> impl Iterator<Item = Segment> {
        (self.line_of(r.start)..=self.line_of(r.end)).map(move |i| self.piece(i, r.clone()))
    }

    /// How many joined lines start before `joined`.
    pub fn lines_before(&self, joined: usize) -> usize {
        self.lines.partition_point(|line| line.joined < joined)
    }

    /// The part of `r` on line `i`:
    /// joined padding bytes inside it become [`Segment::padding`] instead of span content.
    #[expect(clippy::cast_possible_truncation)] // in-line lengths, padding clamped to a u8
    fn piece(&self, i: usize, r: Range<usize>) -> Segment {
        let line = self.lines[i];
        let start = r.start.max(line.joined);
        let end = r.end.min(self.line_end(i)).max(start);
        let content = line.joined + usize::from(line.padding);
        let padding = content.saturating_sub(start).min(end - start);
        let source = self.offset(start + padding);
        Segment::new(Span::new(source, source + (end - start - padding) as u32), padding as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: u32, end: u32, padding: u8) -> IrSeg {
        IrSeg { seg: Segment::new(Span::new(start, end), padding), lazy: false }
    }

    /// `> ab\n>\tcd\n`: the `>\t` prefix leaves 2 virtual spaces of padding before `cd`.
    fn sample() -> Input {
        Input::new("> ab\n>\tcd\n", &[seg(2, 4, 0), seg(7, 9, 2)])
    }

    /// Joined positions map back through container prefixes and virtual padding
    /// to original-source offsets.
    #[test]
    fn maps_back_to_source() {
        let input = sample();
        assert_eq!(input.text, "ab\n  cd");

        assert_eq!(input.span(0..2), Span::new(2, 4));
        assert_eq!(input.span(5..7), Span::new(7, 9));
        // The separator and padding bytes point at the next line's start
        assert_eq!(input.offset(2), 7);
        assert_eq!(input.offset(3), 7);
        assert_eq!(input.span(1..1), Span::empty(3));
        assert_eq!(input.lines_before(0), 0);
        assert_eq!(input.lines_before(3), 1);
    }

    #[test]
    fn pieces_split_on_separators_and_keep_padding() {
        let input = sample();
        assert_eq!(
            input.pieces(0..7),
            vec![Segment::new(Span::new(2, 4), 0), Segment::new(Span::new(7, 9), 2)]
        );
        // A range starting inside the padding keeps only the remaining padding
        assert_eq!(input.pieces(4..7), vec![Segment::new(Span::new(7, 9), 1)]);
        assert_eq!(input.pieces(1..2), vec![Segment::new(Span::new(3, 4), 0)]);
        // A range ending right after the separator has an empty piece on the next line;
        // one starting at the separator, an empty piece before it (`[\nfoo\n]` labels)
        assert_eq!(
            input.pieces(0..3),
            vec![Segment::new(Span::new(2, 4), 0), Segment::new(Span::empty(7), 0)]
        );
        assert_eq!(
            input.pieces(2..7),
            vec![Segment::new(Span::empty(7), 0), Segment::new(Span::new(7, 9), 2)]
        );
    }
}
