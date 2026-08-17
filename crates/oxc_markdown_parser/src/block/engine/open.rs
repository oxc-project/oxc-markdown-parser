//! Step 5 of the line loop: opening new blocks at a flow position.
//!
//! `open_blocks` walks the probe table once per flow position.
//! Containers (`>`, list items, footnote definitions) re-enter the loop for the rest of the line;
//! leaves consume it.
//! One [`Start`] variant, one arm, one opener.

use std::ops::Range;

use crate::block::cursor::Cursor;
use crate::block::ir::{Ir, IrSeg};
use crate::block::probe::{Start, item_interrupts};
use crate::block::scan;
use crate::block::scan::ListMarkerScan;
use crate::pos::{Segment, Span};

use super::Engine;
use super::state::{Leaf, OpenContainer};

impl<'s> Engine<'s> {
    pub(super) fn open_blocks(&mut self, mut cur: Cursor<'s>, content_end: u32) {
        loop {
            if scan::is_blank(cur.tail()) {
                // Empty quote (`>`) or empty list item (`- `) opened this line
                return;
            }
            let level_col = cur.col;
            let indent = cur.indent_cols();
            let para_open = matches!(self.leaf, Some(Leaf::Paragraph { .. }));

            if indent >= 4 {
                if !para_open && self.constructs.code_indented {
                    cur.advance_cols(4);
                    self.open_indented_code(cur, content_end);
                } else {
                    // Paragraph continuation (indented code cannot interrupt),
                    // or in MDX mode an over-indented paragraph line.
                    self.paragraph_text(cur, content_end, true);
                }
                return;
            }

            let save = cur;
            cur.skip_indent_up_to(3);
            if cur.partial != 0 {
                // A tab straddles the indent boundary;
                // only whitespace-sensitive constructs (paragraph text) make sense here.
                self.paragraph_text(cur, content_end, false);
                return;
            }

            match self.probe(cur.tail(), para_open) {
                Some(Start::Quote) => {
                    self.open_quote(&mut cur);
                    continue;
                }
                // An item interrupting the open paragraph must be non-empty, and if ordered, start at 1
                // (`para_open` is per iteration: a container opened on this line closed that paragraph,
                // unlike micromark's line-level flag, see `DIVERGENCES.md`).
                // A restricted marker falls through to paragraph text.
                Some(Start::ListItem { m, empty }) if !para_open || item_interrupts(&m, empty) => {
                    self.open_list_item(&mut cur, &m, empty, level_col);
                    continue;
                }
                Some(Start::FootnoteDef { label, content }) => {
                    self.open_footnote_definition(&mut cur, label, content);
                    continue;
                }
                Some(Start::Setext(level)) => {
                    if !self.open_setext_heading(cur, level) {
                        // The whole paragraph was definitions, so there is no heading text.
                        // commonmark.js falls through to the remaining starts under the interrupt restriction:
                        // `---` is a thematic break, `===` and `-` text.
                        if scan::thematic_break(cur.tail()) {
                            self.open_thematic_break(cur);
                        } else {
                            self.paragraph_text(cur, content_end, false);
                        }
                    }
                }
                Some(Start::ThematicBreak) => self.open_thematic_break(cur),
                Some(Start::Atx { level, content }) => self.open_atx_heading(cur, level, content),
                Some(Start::Fence { fence, len, info }) => {
                    self.open_fence(cur, indent, content_end, fence, len, info);
                }
                Some(Start::MathFence { len, meta }) => {
                    self.open_fence(cur, indent, content_end, b'$', len, meta);
                }
                Some(Start::Directive { len }) => {
                    self.open_directive(cur, indent, content_end, len);
                }
                Some(Start::Liquid) => {
                    // micromark's interrupt check passes on the two-byte opener alone,
                    // so an open paragraph closes even when the full construct fails
                    // and this line restarts as a new paragraph
                    // (whose inline phase may still find a liquid text tag).
                    self.close_leaf();
                    match self.liquid_scan(cur, content_end) {
                        Some(liquid) => self.open_liquid(liquid),
                        None => self.paragraph_text(cur, content_end, false),
                    }
                }
                Some(Start::Html { kind }) => self.open_html(save, content_end, kind),
                Some(Start::TableDelimiter(align)) => {
                    if !self.try_start_table(align, save.segment(content_end)) {
                        // Cell counts didn't line up:
                        // the delimiter row is ordinary paragraph text
                        // (`para_open` is guaranteed by the probe gate, so this appends).
                        self.paragraph_text(save, content_end, false);
                    }
                }
                Some(Start::ListItem { .. }) | None => {
                    self.paragraph_text(if para_open { save } else { cur }, content_end, false);
                }
            }
            return;
        }
    }

    /// `cur` sits past the 4 columns of indent.
    fn open_indented_code(&mut self, cur: Cursor<'s>, content_end: u32) {
        self.close_list_top();
        self.leaf = Some(Leaf::IndentedCode {
            lines: vec![cur.segment(content_end)],
            pending: Vec::new(),
            start: cur.offset(),
            end: content_end,
        });
    }

    fn open_quote(&mut self, cur: &mut Cursor<'s>) {
        self.close_leaf_and_list();
        let start = cur.offset();
        cur.bump(1);
        cur.eat_one_space();
        // An empty quote still spans its `>` marker
        // (as an empty item spans its marker)
        self.stack.push(OpenContainer::Quote { children: Vec::new(), start, end: start + 1 });
    }

    /// Opens an item, and a new list first unless the top of the stack is a compatible one.
    /// `level_col` is the column the flow position's indent was measured from.
    fn open_list_item(
        &mut self,
        cur: &mut Cursor<'s>,
        m: &ListMarkerScan,
        empty: bool,
        level_col: usize,
    ) {
        self.close_leaf();
        let compatible = matches!(
            self.stack.last(),
            Some(OpenContainer::List { ordered, marker, .. })
                if *ordered == m.ordered && *marker == m.marker
        );
        if !compatible {
            self.close_list_top();
            self.stack.push(OpenContainer::List {
                items: Vec::new(),
                ordered: m.ordered,
                marker: m.marker,
                start: cur.offset(),
            });
        }
        let marker_start = cur.offset();
        #[expect(clippy::cast_possible_truncation)] // marker is short ASCII
        let marker_span = Span::new(marker_start, marker_start + m.len as u32);
        cur.bump(m.len);
        let w = cur.indent_cols();
        let (padding, content_indent) = if empty {
            (0u8, cur.col - level_col + 1)
        } else if (1..=4).contains(&w) {
            cur.advance_cols(w);
            #[expect(clippy::cast_possible_truncation)] // w <= 4
            (w as u8, cur.col - level_col)
        } else {
            // 5+ columns of spacing:
            // content starts one space in, the rest is content (e.g. indented code).
            cur.advance_cols(1);
            (1u8, cur.col - level_col)
        };
        self.stack.push(OpenContainer::Item {
            children: Vec::new(),
            marker: marker_span,
            padding,
            content_indent,
            start: marker_start,
            end: marker_span.end,
        });
    }

    /// `label` is the label's byte range within the tail; `content` the position past the `:`.
    fn open_footnote_definition(
        &mut self,
        cur: &mut Cursor<'s>,
        label: Range<usize>,
        content: usize,
    ) {
        self.close_leaf_and_list();
        let base = cur.offset();
        let label = Span::at(base, label);
        // Registered immediately, micromark-style;
        // the inline phase runs after the block phase either way.
        self.refs.insert_footnote(
            crate::syntax::label::normalize(label.slice(self.source)).into_owned(),
        );
        self.stack.push(OpenContainer::Footnote {
            children: Vec::new(),
            label,
            content_indent: 4,
            start: base,
            end: base,
        });
        cur.bump(content);
        // Optional spaces before same-line content
        cur.skip_all_ws();
    }

    /// The open paragraph becomes the heading; definitions at its head are stripped first.
    /// Returns `false` when the paragraph held only definitions:
    /// they are attached, but no heading exists and the underline line is not consumed.
    fn open_setext_heading(&mut self, cur: Cursor<'s>, level: u8) -> bool {
        let underline = cur.trimmed_span();
        let Some(Leaf::Paragraph { segments, .. }) = self.leaf.take() else {
            unreachable!("setext is only probed with a paragraph open")
        };
        let rest = self.finish_paragraph(segments);
        let Some(first) = rest.first() else { return false };
        let start = first.seg.span.start;
        self.attach(Ir::Heading {
            level,
            underline: Some(underline),
            segments: rest,
            span: Span::new(start, underline.end),
        });
        true
    }

    fn open_thematic_break(&mut self, cur: Cursor<'s>) {
        self.close_leaf_and_list();
        self.attach(Ir::ThematicBreak { span: cur.trimmed_span() });
    }

    /// `content` is the heading text's byte range within the tail (closing `#` run stripped).
    fn open_atx_heading(&mut self, cur: Cursor<'s>, level: u8, content: Range<usize>) {
        self.close_leaf_and_list();
        let base = cur.offset();
        let segments = if content.is_empty() {
            Vec::new()
        } else {
            let seg = Segment::new(Span::at(base, content), 0);
            vec![IrSeg { seg, lazy: false }]
        };
        self.attach(Ir::Heading { level, underline: None, segments, span: cur.trimmed_span() });
    }

    /// Code (`` ` ``/`~`) and math (`$`) fences share one leaf;
    /// `indent` is the fence line's indent, `info` the info-string / meta byte range within the tail
    /// (trimmed; empty means none).
    fn open_fence(
        &mut self,
        cur: Cursor<'s>,
        indent: usize,
        content_end: u32,
        fence: u8,
        len: u32,
        info: Range<usize>,
    ) {
        let info = (!info.is_empty()).then_some(info);
        self.close_leaf_and_list();
        let base = cur.offset();
        self.leaf = Some(Leaf::FencedCode {
            fence,
            len,
            indent,
            info: info.map(|r| Span::at(base, r)),
            lines: Vec::new(),
            start: base,
            end: content_end,
        });
    }

    /// `indent` is the fence line's indent.
    fn open_directive(&mut self, cur: Cursor<'s>, indent: usize, content_end: u32, len: u32) {
        self.close_leaf_and_list();
        self.stack.push(OpenContainer::Directive {
            children: Vec::new(),
            fence_len: len,
            content_indent: indent,
            opening: Span::new(cur.offset(), content_end),
            closing: None,
            end: content_end,
        });
    }

    /// `line` is the cursor at the physical line start (the block's first line keeps its indent).
    fn open_html(&mut self, line: Cursor<'s>, content_end: u32, kind: u8) {
        self.close_leaf_and_list();
        // End conditions are position-independent, so the indented line serves
        let done = kind <= 5 && scan::html_block_end(line.tail(), kind);
        self.leaf = Some(Leaf::Html {
            kind,
            lines: vec![line.segment(content_end)],
            trailing_newline: false,
            start: line.offset(),
            end: content_end,
        });
        if done {
            self.close_leaf();
        }
    }
}
