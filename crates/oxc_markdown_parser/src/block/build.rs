//! IR → arena AST.
//!
//! Runs after the block phase completes:
//! builds the tree, running the inline phase per leaf against the reference-definition map the engine collected,
//! and computes span-derived facts the AST carries as first-class fields, currently list tightness and item spread.

use oxc_allocator::{Allocator, ArenaBox, ArenaVec};

use crate::ast::*;
use crate::inline::RefMap;
use crate::options::Constructs;
use crate::pos::Span;

use super::ir::{Ir, IrItem};

/// `blanks` is [`super::engine::BlankLines::loosening`], the list-tightness table.
pub fn build<'a>(
    allocator: &'a Allocator,
    source: &str,
    blanks: &[Span],
    irs: Vec<Ir>,
    span: Span,
    constructs: &Constructs,
    refs: &RefMap,
) -> Root<'a> {
    let cx = Cx { allocator, source, blanks, refs, constructs };
    Root { children: cx.blocks(irs), span }
}

struct Cx<'x, 'a> {
    allocator: &'a Allocator,
    source: &'x str,
    blanks: &'x [Span],
    refs: &'x RefMap,
    constructs: &'x Constructs,
}

impl<'a> Cx<'_, 'a> {
    fn blocks(&self, irs: Vec<Ir>) -> ArenaVec<'a, Block<'a>> {
        let mut out = ArenaVec::with_capacity_in(irs.len(), &self.allocator);
        for ir in irs {
            out.push(self.block(ir));
        }
        out
    }

    /// `new_in` + `extend`: no arena reservation for the common no-lazy-lines case
    /// (`from_iter_in` would reserve the filter's upper size hint).
    fn lazy_lines(&self, segments: &[super::IrSeg]) -> ArenaVec<'a, Span> {
        let mut out = ArenaVec::new_in(&self.allocator);
        out.extend(segments.iter().filter(|s| s.lazy).map(|s| s.seg.span));
        out
    }

    fn inlines(&self, segments: &[super::IrSeg]) -> ArenaVec<'a, Inline<'a>> {
        crate::inline::inlines(self.allocator, self.source, segments, self.refs, self.constructs)
    }

    fn block(&self, ir: Ir) -> Block<'a> {
        match ir {
            Ir::Paragraph { segments, span } => {
                let lazy_lines = self.lazy_lines(&segments);
                let children = self.inlines(&segments);
                Block::Paragraph(ArenaBox::new_in(
                    Paragraph { children, lazy_lines, span },
                    &self.allocator,
                ))
            }
            Ir::Heading { level, underline, segments, span } => {
                let kind = match underline {
                    Some(underline) => {
                        HeadingKind::Setext { underline, lazy_lines: self.lazy_lines(&segments) }
                    }
                    None => HeadingKind::Atx,
                };
                let children = self.inlines(&segments);
                Block::Heading(ArenaBox::new_in(
                    Heading { level, kind, children, span },
                    &self.allocator,
                ))
            }
            Ir::ThematicBreak { span } => Block::ThematicBreak(ThematicBreak { span }),
            Ir::Code { fenced, lines, span } => {
                let kind = match fenced {
                    Some((fence, info)) => CodeBlockKind::Fenced { fence, info },
                    None => CodeBlockKind::Indented,
                };
                let lines = ArenaVec::from_iter_in(lines, &self.allocator);
                Block::CodeBlock(ArenaBox::new_in(CodeBlock { kind, lines, span }, &self.allocator))
            }
            Ir::Html { kind, lines, trailing_newline, span } => {
                let lines = ArenaVec::from_iter_in(lines, &self.allocator);
                Block::HtmlBlock(ArenaBox::new_in(
                    HtmlBlock { kind, lines, trailing_newline, span },
                    &self.allocator,
                ))
            }
            Ir::Quote { children, span } => {
                let children = self.blocks(children);
                Block::Blockquote(ArenaBox::new_in(Blockquote { children, span }, &self.allocator))
            }
            Ir::Definition { label, destination, title, span } => {
                let label = ArenaVec::from_iter_in(label, &self.allocator);
                let title = title.map(|t| ArenaVec::from_iter_in(t, &self.allocator));
                Block::Definition(ArenaBox::new_in(
                    Definition { label, destination, title, span },
                    &self.allocator,
                ))
            }
            Ir::FootnoteDefinition { label, children, span } => {
                let children = self.blocks(children);
                Block::FootnoteDefinition(ArenaBox::new_in(
                    FootnoteDefinition { label, children, span },
                    &self.allocator,
                ))
            }
            Ir::Table { align, rows, span } => {
                let align = ArenaVec::from_iter_in(align, &self.allocator);
                let mut children = ArenaVec::with_capacity_in(rows.len(), &self.allocator);
                for row in rows {
                    let mut cells = ArenaVec::with_capacity_in(row.cells.len(), &self.allocator);
                    for cell in row.cells {
                        let seg =
                            super::IrSeg { seg: crate::pos::Segment::new(cell, 0), lazy: false };
                        cells.push(TableCell { children: self.inlines(&[seg]), span: cell });
                    }
                    children.push(TableRow { children: cells, span: row.span });
                }
                Block::Table(ArenaBox::new_in(Table { align, children, span }, &self.allocator))
            }
            Ir::MathBlock { meta, lines, span } => {
                let lines = ArenaVec::from_iter_in(lines, &self.allocator);
                Block::MathBlock(ArenaBox::new_in(MathBlock { meta, lines, span }, &self.allocator))
            }
            Ir::Liquid { pieces, span } => {
                let pieces = ArenaVec::from_iter_in(pieces, &self.allocator);
                Block::Liquid(ArenaBox::new_in(Liquid { pieces, span }, &self.allocator))
            }
            Ir::ContainerDirective { opening, closing, children, span } => {
                let children = self.blocks(children);
                Block::ContainerDirective(ArenaBox::new_in(
                    ContainerDirective { opening, closing, children, span },
                    &self.allocator,
                ))
            }
            Ir::List { ordered, marker, items, span } => {
                // cmark's list-wide looseness: a blank line between two items,
                // or inside any item between its children (`ListItem::spread`).
                // Blank lines inside a child (a code block, a nested list) don't count.
                let gap = items.windows(2).any(|pair| {
                    has_blank_between(self.blanks, pair[0].span.end, pair[1].span.start)
                });
                let marker = if ordered {
                    ListMarker::Ordered { delimiter: marker }
                } else {
                    ListMarker::Bullet { marker }
                };
                let mut children = ArenaVec::with_capacity_in(items.len(), &self.allocator);
                for item in items {
                    children.push(self.item(item));
                }
                let tight = !gap && !children.iter().any(|item| item.spread);
                Block::List(ArenaBox::new_in(
                    List { marker, tight, children, span },
                    &self.allocator,
                ))
            }
        }
    }

    fn item(&self, item: IrItem) -> ListItem<'a> {
        let IrItem { marker, padding, mut children, span } = item;
        let checkbox = if self.constructs.gfm_task_list_item {
            self.take_checkbox(&mut children)
        } else {
            None
        };
        let spread = item_is_spread(self.blanks, &children);
        let children = self.blocks(children);
        ListItem { marker, padding, checkbox, spread, children, span }
    }

    /// GFM task list: `[ ]` / `[\t]` / `[x]` / `[X]` at the very start of the item's first content
    /// (micromark's `_gfmTasklistFirstContentOfListItem`): the first paragraph,
    /// which may sit behind definitions stripped from the same paragraph chunk (`- [a]: /u\n  [ ] b`),
    /// but not behind a blank line or another block.
    /// The paragraph must continue after the checkbox:
    /// non-whitespace on the same line, or a following line.
    /// The checkbox and the whitespace before same-line content leave the paragraph
    /// (mdast strips that whitespace too).
    /// When only whitespace follows on the line, the paragraph starts right after `]`:
    /// that whitespace is a hard or soft break (micromark: `- [x]   \n:-:` renders `<br />`).
    fn take_checkbox(&self, children: &mut [Ir]) -> Option<TaskCheckbox> {
        let mut at = 0;
        while let Some(Ir::Definition { span, .. }) = children.get(at) {
            // Same chunk as the next child iff no blank line separates them
            let next_start = children.get(at + 1)?.span().start;
            if has_blank_between(self.blanks, span.end, next_start) {
                return None;
            }
            at += 1;
        }
        let Some(Ir::Paragraph { segments, span }) = children.get_mut(at) else { return None };
        let multiline = segments.len() > 1;
        let first = segments.first_mut()?;
        let raw = first.seg.span.slice(self.source);
        let checked = match raw.as_bytes() {
            [b'[', b' ' | b'\t', b']', ..] => false,
            [b'[', b'x' | b'X', b']', ..] => true,
            _ => return None,
        };
        // The checkbox needs the paragraph to continue:
        // whitespace (not content) may follow only when something comes after it.
        let rest = &raw[3..];
        if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
            return None;
        }
        let after_ws = rest.trim_start_matches([' ', '\t']);
        if !multiline && after_ws.is_empty() {
            return None;
        }
        let checkbox = TaskCheckbox { span: Span::at(first.seg.span.start, 0..3), checked };
        // Paragraph starts at same-line content, or right after `]` when only whitespace follows
        let skip = if after_ws.is_empty() { 3 } else { raw.len() - after_ws.len() };
        first.seg.span.start += u32::try_from(skip).unwrap_or(0);
        span.start = first.seg.span.start;
        Some(checkbox)
    }
}

/// A blank line between two of an item's block children.
fn item_is_spread(blanks: &[Span], children: &[Ir]) -> bool {
    children
        .windows(2)
        .any(|pair| has_blank_between(blanks, pair[0].span().end, pair[1].span().start))
}

fn has_blank_between(blanks: &[Span], a: u32, b: u32) -> bool {
    let from = blanks.partition_point(|s| s.start < a);
    blanks[from..].iter().take_while(|s| s.start < b).any(|s| s.end <= b)
}
