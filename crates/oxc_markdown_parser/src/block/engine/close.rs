//! Closing blocks and attaching them to their container (the mirror of `open.rs`).
//! Paragraph close strips reference definitions (`refdef`) and registers their labels.

use crate::pos::{Segment, Span};

use super::super::ir::{Ir, IrItem, IrSeg};
use super::super::refdef;
use super::Engine;
use super::state::{Leaf, OpenContainer};

impl Engine<'_> {
    /// Closes the leaf into its container.
    pub(super) fn close_leaf(&mut self) {
        let Some(leaf) = self.leaf.take() else { return };
        let ir = match leaf {
            Leaf::Paragraph { segments, .. } => {
                let rest = self.finish_paragraph(segments);
                let (Some(first), Some(last)) = (rest.first(), rest.last()) else { return };
                Ir::Paragraph {
                    span: Span::new(first.seg.span.start, last.seg.span.end),
                    segments: rest,
                }
            }
            Leaf::IndentedCode { lines, start, end, .. } => {
                Ir::Code { fenced: None, lines, span: Span::new(start, end) }
            }
            Leaf::FencedCode { fence: b'$', info, lines, start, end, .. } => {
                Ir::MathBlock { meta: info, lines, span: Span::new(start, end) }
            }
            Leaf::FencedCode { fence, info, lines, start, end, .. } => {
                Ir::Code { fenced: Some((fence, info)), lines, span: Span::new(start, end) }
            }
            Leaf::Html { kind, lines, trailing_newline, start, end } => {
                Ir::Html { kind, lines, trailing_newline, span: Span::new(start, end) }
            }
            Leaf::Table { align, rows, start, end } => {
                Ir::Table { align, rows, span: Span::new(start, end) }
            }
        };
        self.attach(ir);
    }

    /// Attaches stripped definitions and registers their labels,
    /// the inline phase's reference map fills here, with no extra tree walk.
    pub(super) fn attach_definitions(&mut self, defs: Vec<Ir>) {
        for def in defs {
            if let Ir::Definition { label, .. } = &def {
                let text = Segment::join(self.source, label);
                self.refs.push(crate::syntax::label::normalize(&text).into_owned());
            }
            self.attach(def);
        }
    }

    /// The paragraph-close prologue shared by paragraphs and setext headings:
    /// leading definitions are stripped and attached, and the last line loses its trailing whitespace.
    /// Returns the remaining paragraph lines, possibly none.
    pub(super) fn finish_paragraph(&mut self, segments: Vec<IrSeg>) -> Vec<IrSeg> {
        let (defs, mut rest) = refdef::strip(self.source, segments);
        self.attach_definitions(defs);
        trim_last(self.source, &mut rest);
        rest
    }

    /// The prologue of every block start that is not a list item:
    /// it ends the open leaf, and a list on top of the stack (only a compatible item continues one).
    pub(super) fn close_leaf_and_list(&mut self) {
        self.close_leaf();
        self.close_list_top();
    }

    /// Closes open containers so that `keep` remain.
    pub(super) fn close_from(&mut self, keep: usize) {
        self.close_leaf();
        while self.stack.len() > keep {
            self.close_container();
        }
    }

    /// Closes an open `List` sitting on top of the stack
    /// (any new block other than a compatible item ends the list).
    pub(super) fn close_list_top(&mut self) {
        debug_assert!(self.leaf.is_none());
        if matches!(self.stack.last(), Some(OpenContainer::List { .. })) {
            self.close_container();
        }
    }

    pub(super) fn close_container(&mut self) {
        debug_assert!(self.leaf.is_none());
        match self.stack.pop().expect("close_container on empty stack") {
            OpenContainer::Quote { children, start, end } => {
                self.attach(Ir::Quote { children, span: Span::new(start, end) });
            }
            OpenContainer::List { items, ordered, marker, start } => {
                // Items arrive in source order; the last one ends the list
                let end = items.last().map_or(start, |item| item.span.end);
                self.attach(Ir::List { ordered, marker, items, span: Span::new(start, end) });
            }
            OpenContainer::Footnote { children, label, start, end, .. } => {
                self.attach(Ir::FootnoteDefinition {
                    label,
                    children,
                    span: Span::new(start, end),
                });
            }
            OpenContainer::Directive { children, opening, closing, end, .. } => {
                self.attach(Ir::ContainerDirective {
                    opening,
                    closing,
                    children,
                    span: Span::new(opening.start, end),
                });
            }
            OpenContainer::Item { children, marker, padding, start, end, .. } => {
                let Some(OpenContainer::List { items, .. }) = self.stack.last_mut() else {
                    unreachable!("list items always sit in a list");
                };
                items.push(IrItem { marker, padding, children, span: Span::new(start, end) });
            }
        }
    }

    /// Attaches a finished block to the current innermost container.
    pub(super) fn attach(&mut self, ir: Ir) {
        let end = ir.span().end;
        let children = match self.stack.last_mut() {
            Some(
                OpenContainer::Quote { children, end: e, .. }
                | OpenContainer::Item { children, end: e, .. }
                | OpenContainer::Footnote { children, end: e, .. }
                | OpenContainer::Directive { children, end: e, .. },
            ) => {
                *e = (*e).max(end);
                children
            }
            Some(OpenContainer::List { .. }) => {
                // Lists never receive non-item children directly
                self.close_container();
                return self.attach(ir);
            }
            None => &mut self.root,
        };
        // Siblings are attached in source order
        debug_assert!(children.last().is_none_or(|prev| prev.span().end <= ir.span().start));
        children.push(ir);
    }
}

/// Drops the trailing whitespace of a closed paragraph's last line from its segment:
/// it is never content (a hard break needs a following line),
/// so the paragraph or setext heading span ends before it, like an ATX heading's.
fn trim_last(source: &str, segments: &mut [IrSeg]) {
    if let Some(last) = segments.last_mut() {
        last.seg.span.end = last.seg.span.start + trimmed_len(last.seg.span.slice(source));
    }
}

/// Byte length of `tail` with trailing whitespace removed.
#[expect(clippy::cast_possible_truncation)] // in-line lengths
fn trimmed_len(tail: &str) -> u32 {
    tail.trim_end_matches([' ', '\t']).len() as u32
}
