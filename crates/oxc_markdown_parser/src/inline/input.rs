//! The inline phase's view of a leaf's content:
//! the logical lines joined with `\n`, plus an exact per-byte map back to source offsets.
//!
//! Node spans always come from this map (never from arithmetic over the joined buffer),
//! so container prefixes between lines can't skew them.

use std::ops::Range;

use oxc_allocator::{Allocator, ArenaVec};

use crate::block::IrSeg;
use crate::pos::{Segment, Span};

pub struct Input {
    pub text: String,
    /// Source offset of each joined byte; separator `\n`s map to the next line's start.
    /// One trailing sentinel (end of the last line).
    offsets: Vec<u32>,
    /// Per joined line: (joined start, virtual padding bytes at its head).
    lines: Vec<(usize, u8)>,
}

impl Input {
    pub fn new(source: &str, segments: &[IrSeg]) -> Self {
        let mut text = String::new();
        let mut offsets = Vec::new();
        let mut lines = Vec::with_capacity(segments.len());
        for (i, s) in segments.iter().enumerate() {
            if i > 0 {
                text.push('\n');
                offsets.push(s.seg.span.start);
            }
            lines.push((text.len(), s.seg.padding));
            for _ in 0..s.seg.padding {
                text.push(' ');
                offsets.push(s.seg.span.start);
            }
            text.push_str(s.seg.span.slice(source));
            offsets.extend(s.seg.span.start..s.seg.span.end);
        }
        offsets.push(segments.last().map_or(0, |s| s.seg.span.end));
        Self { text, offsets, lines }
    }

    pub fn offset(&self, joined: usize) -> u32 {
        self.offsets[joined.min(self.offsets.len() - 1)]
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
    /// Single-line ranges (the common case) take no scan.
    pub fn pieces_in<'a>(
        &self,
        r: Range<usize>,
        allocator: &'a Allocator,
    ) -> ArenaVec<'a, Segment> {
        let text = &self.text[r.clone()];
        if !text.contains('\n') {
            return ArenaVec::from_array_in([self.piece(r)], &allocator);
        }
        let mut out = ArenaVec::with_capacity_in(text.matches('\n').count() + 1, &allocator);
        let mut start = r.start;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                out.push(self.piece(start..r.start + i));
                start = r.start + i + 1;
            }
        }
        out.push(self.piece(start..r.end));
        out
    }

    /// Std-vector form of [`pieces_in`](Self::pieces_in), for the block-phase IR.
    pub fn pieces(&self, r: Range<usize>) -> Vec<Segment> {
        let mut out = Vec::new();
        let mut start = r.start;
        for (i, b) in self.text[r.clone()].bytes().enumerate() {
            if b == b'\n' {
                out.push(self.piece(start..r.start + i));
                start = r.start + i + 1;
            }
        }
        out.push(self.piece(start..r.end));
        out
    }

    /// How many joined lines start before `joined`.
    pub fn lines_before(&self, joined: usize) -> usize {
        self.lines.partition_point(|&(start, _)| start < joined)
    }

    /// One single-line piece:
    /// joined padding bytes inside it become [`Segment::padding`] instead of span content.
    #[expect(clippy::cast_possible_truncation)] // clamped to a u8 padding
    fn piece(&self, r: Range<usize>) -> Segment {
        let line = self.lines.partition_point(|&(start, _)| start <= r.start).saturating_sub(1);
        let padding_end =
            self.lines.get(line).map_or(0, |&(start, padding)| start + usize::from(padding));
        let padding = padding_end.saturating_sub(r.start).min(r.len());
        Segment::new(self.span(r.start + padding..r.end), padding as u8)
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
    }
}
