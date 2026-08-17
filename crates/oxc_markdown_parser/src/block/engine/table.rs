//! GFM tables in the line loop:
//! the delimiter row that steals a paragraph's last line as the header, and row continuation.

use crate::ast::TableAlign;
use crate::block::cursor::Cursor;
use crate::block::ir::IrRow;
use crate::block::probe::Start;
use crate::block::scan;
use crate::pos::{Segment, Span};

use super::Engine;
use super::state::Leaf;

impl<'s> Engine<'s> {
    /// Any non-blank line continues the open table as a data row unless another block construct starts there.
    /// micromark disables the cheap interrupt shortcut during table continuation (`_gfmTableDynamicInterruptHack`),
    /// so a liquid opener is only a maybe-start:
    /// it ends the table when the full construct validates, and stays a row otherwise.
    /// Returns whether the line was consumed.
    pub(super) fn continue_table(&mut self, cur: Cursor<'s>, content_end: u32) -> bool {
        let row_at = match cur.flow_start().map(|c| (c, self.probe(c.tail(), false))) {
            Some((c, None)) => Some(c),
            Some((c, Some(Start::Liquid))) => {
                if let Some(liquid) = self.liquid_scan(c, content_end) {
                    self.close_leaf();
                    self.open_liquid(liquid);
                    return true;
                }
                Some(c)
            }
            _ => None,
        };
        let Some(c) = row_at else { return false };
        let span = Span::new(c.offset(), content_end);
        let cells = row_cell_spans(self.source, span);
        if let Some(Leaf::Table { rows, end, .. }) = &mut self.leaf {
            rows.push(IrRow { span, cells });
            *end = content_end;
        }
        true
    }

    /// A delimiter row arrived while a paragraph is open.
    /// Its last line is the header candidate;
    /// on a cell-count match it is stolen and the rest of the paragraph closes.
    /// micromark then retries the flow constructs on the stolen line (the paragraph is gone),
    /// and `htmlFlow` outranks the table: a type 7 HTML start (paragraph text only because type 7 can't interrupt)
    /// opens an HTML block holding the delimiter row as its second line.
    /// Type 7 is the only construct that can flip this way
    /// (1–6 would have interrupted; setext and the delimiter row itself can't be a header).
    /// `row` is the delimiter row's line (indent included).
    pub(super) fn try_start_table(&mut self, align: Vec<TableAlign>, row: Segment) -> bool {
        let Some(Leaf::Paragraph { segments, last_deep }) = &self.leaf else { return false };
        let Some(last) = segments.last() else { return false };
        if last.lazy || *last_deep {
            return false;
        }
        let header = last.seg;
        let cells = row_cell_spans(self.source, header.span);
        if cells.len() != align.len() {
            return false;
        }
        // Not deep, so the ≤3-column indent is exactly the leading spaces/tabs
        let tail = header.span.slice(self.source).trim_start_matches([' ', '\t']);
        let html7 = matches!(self.probe(tail, false), Some(Start::Html { kind: 7 }));
        if let Some(Leaf::Paragraph { segments, .. }) = &mut self.leaf {
            segments.pop();
        }
        // close_leaf drops an emptied paragraph on its own
        self.close_leaf_and_list();
        self.leaf = Some(if html7 {
            Leaf::Html {
                kind: 7,
                lines: vec![header, row],
                trailing_newline: false,
                start: header.span.start,
                end: row.span.end,
            }
        } else {
            Leaf::Table {
                align,
                rows: vec![IrRow { span: header.span, cells }],
                start: header.span.start,
                end: row.span.end,
            }
        });
        true
    }
}

/// Table row cells as source spans within `line_span`.
fn row_cell_spans(source: &str, line_span: Span) -> Vec<Span> {
    scan::table_row_cells(line_span.slice(source))
        .into_iter()
        .map(|r| Span::at(line_span.start, r))
        .collect()
}
