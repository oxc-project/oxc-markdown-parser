//! Flow liquid tags (`{% … %}` / `{{ … }}`) in the line loop:
//! multi-line validation against the container stack, and the dead-position cache
//! (`Engine::liquid_dead`) that keeps failed attempts from rescanning quadratically.

use crate::block::cursor::Cursor;
use crate::block::ir::Ir;
use crate::block::scan;
use crate::pos::{Segment, Span};
use crate::syntax::liquid;

use super::state::StackMatch;
use super::{Engine, line_content_end, next_line_start};

/// A validated flow liquid tag, ready to attach.
pub(super) struct LiquidTag {
    /// One verbatim piece per line.
    pub pieces: Vec<Segment>,
    pub span: Span,
    /// Content end of the closing line; lines up to it are consumed.
    pub last_line_end: u32,
}

impl<'s> Engine<'s> {
    /// Validates a flow liquid tag starting at `cur` (at the `{`),
    /// scanning for the first closer across lines, micromark-style:
    /// continuation lines must match this container stack's prefixes (a lazy or unprefixed line fails),
    /// EOF before the closer fails, and only whitespace may follow the closer on its line.
    /// (Any failure rejects the whole construct retroactively.)
    /// The closer never spans a line ending.
    #[expect(clippy::cast_possible_truncation)] // in-line lengths
    pub(super) fn liquid_scan(
        &mut self,
        mut cur: Cursor<'s>,
        content_end: u32,
    ) -> Option<LiquidTag> {
        let closer = liquid::open(cur.tail()).expect("probed as a liquid opener");
        let kind = usize::from(closer == b'}');
        let start = cur.offset();
        // See `Engine::liquid_dead`
        if start.saturating_add(2) <= self.liquid_dead[kind] {
            return None;
        }
        let mut pieces = Vec::new();
        let mut line_end = content_end;
        // The first line searches past the two-byte opener
        let mut search_from = 2usize;
        loop {
            let tail = cur.tail();
            if let Some(after) = liquid::close(&tail[search_from..], closer) {
                let close = search_from + after;
                if !scan::is_blank(&tail[close..]) {
                    // Line-local failure: cacheable (see `Engine::liquid_dead`)
                    self.liquid_dead[kind] = cur.offset() + close as u32 - 2;
                    return None;
                }
                let end = cur.offset() + close as u32;
                pieces.push(cur.segment(end));
                return Some(LiquidTag {
                    pieces,
                    span: Span::new(start, end),
                    last_line_end: line_end,
                });
            }
            pieces.push(cur.segment(line_end));
            let Some(next_start) = next_line_start(self.source, line_end) else {
                // No closer of this kind exists from here to EOF,
                // so no later attempt can succeed either.
                self.liquid_dead[kind] = u32::MAX;
                return None;
            };
            let next_end = line_content_end(self.source, next_start);
            let mut next = self.line_cursor(next_start, next_end);
            // A directive-close line never reaches the liquid's content
            if !matches!(self.match_stack(&mut next), StackMatch::Prefixes(m) if m == self.stack.len())
            {
                return None;
            }
            // Prettier's liquid is not `concrete`:
            // a container starting on a continuation line interrupts the attempt.
            if next
                .flow_start()
                .is_some_and(|c| self.probe(c.tail(), false).is_some_and(|s| s.is_container()))
            {
                return None;
            }
            cur = next;
            line_end = next_end;
            search_from = 0;
        }
    }

    /// Opens (attaches) a validated flow liquid tag and skips its lines.
    pub(super) fn open_liquid(&mut self, tag: LiquidTag) {
        self.close_list_top();
        self.attach(Ir::Liquid { pieces: tag.pieces, span: tag.span });
        self.skip_to = tag.last_line_end;
    }
}
