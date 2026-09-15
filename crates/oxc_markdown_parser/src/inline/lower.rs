//! Lowering: proto-nodes over the joined text to the arena AST, spans mapped back to the source.

use oxc_allocator::{Allocator, ArenaBox, ArenaVec};

use crate::ast::*;
use crate::pos::Span;

use super::input::Input;
use super::{PLinkKind, PN};

/// Siblings are emitted in source order
fn push<'a>(out: &mut ArenaVec<'a, Inline<'a>>, node: Inline<'a>) {
    debug_assert!(out.last().is_none_or(|prev| prev.span().end <= node.span().start));
    out.push(node);
}

pub(super) fn push_ast<'a>(
    allocator: &'a Allocator,
    input: &Input,
    pn: PN,
    out: &mut ArenaVec<'a, Inline<'a>>,
) {
    match pn {
        PN::Text(r) => {
            let span = input.span(r.clone());
            let text = &input.text[r];
            let contains_cjk = !text.is_ascii() && text.chars().any(is_cjk);
            // Bracket and delimiter runs arrive as their own text pieces;
            // contiguous pieces merge into one node (as mdast does).
            if let Some(Inline::Text(prev)) = out.last_mut()
                && prev.span.end == span.start
            {
                prev.span.end = span.end;
                prev.contains_cjk |= contains_cjk;
                return;
            }
            push(out, Inline::Text(Text { span, contains_cjk }));
        }
        PN::SoftBreak(p) => {
            push(out, Inline::SoftBreak(SoftBreak { span: Span::empty(input.offset(p)) }));
        }
        PN::HardBreak { kind, r } => {
            push(out, Inline::HardBreak(HardBreak { kind, span: input.span(r) }));
        }
        PN::Code { r, content } => {
            let pieces = input.pieces_in(content, allocator);
            push(
                out,
                Inline::CodeSpan(ArenaBox::new_in(
                    CodeSpan { pieces, span: input.span(r) },
                    &allocator,
                )),
            );
        }
        PN::Html { r } => {
            let pieces = input.pieces_in(r.clone(), allocator);
            push(
                out,
                Inline::HtmlInline(ArenaBox::new_in(
                    HtmlInline { pieces, span: input.span(r) },
                    &allocator,
                )),
            );
        }
        PN::Autolink { r, email } => {
            push(out, Inline::Autolink(Autolink { span: input.span(r), email }));
        }
        PN::AutolinkLiteral(r) => {
            push(out, Inline::AutolinkLiteral(AutolinkLiteral { span: input.span(r) }));
        }
        PN::FootnoteRef { label, r } => {
            push(
                out,
                Inline::FootnoteReference(ArenaBox::new_in(
                    FootnoteReference { label: input.span(label), span: input.span(r) },
                    &allocator,
                )),
            );
        }
        PN::MathSpan(r) => {
            let pieces = input.pieces_in(r.clone(), allocator);
            push(
                out,
                Inline::MathSpan(ArenaBox::new_in(
                    MathSpan { pieces, span: input.span(r) },
                    &allocator,
                )),
            );
        }
        PN::WikiLink(r) => {
            push(out, Inline::WikiLink(WikiLink { span: input.span(r) }));
        }
        PN::Liquid(r) => {
            let pieces = input.pieces_in(r.clone(), allocator);
            push(
                out,
                Inline::Liquid(ArenaBox::new_in(
                    Liquid { pieces, span: input.span(r) },
                    &allocator,
                )),
            );
        }
        PN::Emph { marker, len, children, r } => {
            let mut inner = ArenaVec::new_in(&allocator);
            for child in children {
                push_ast(allocator, input, child, &mut inner);
            }
            let span = input.span(r);
            if marker == b'~' {
                push(
                    out,
                    Inline::Strikethrough(ArenaBox::new_in(
                        Strikethrough { tildes: len, children: inner, span },
                        &allocator,
                    )),
                );
            } else if len == 2 {
                push(
                    out,
                    Inline::Strong(ArenaBox::new_in(
                        Strong { marker, children: inner, span },
                        &allocator,
                    )),
                );
            } else {
                push(
                    out,
                    Inline::Emphasis(ArenaBox::new_in(
                        Emphasis { marker, children: inner, span },
                        &allocator,
                    )),
                );
            }
        }
        PN::Link { image, kind, children, text, r } => {
            let mut inner = ArenaVec::new_in(&allocator);
            for child in children {
                push_ast(allocator, input, child, &mut inner);
            }
            let span = input.span(r);
            let kind = match kind {
                PLinkKind::Inline { dest, angle_bracketed, title } => LinkKind::Inline {
                    destination: Destination { span: input.span(dest), angle_bracketed },
                    title: title.map(|tr| input.pieces_in(tr, allocator)),
                },
                PLinkKind::Reference { kind, label } => {
                    LinkKind::Reference { kind, label: input.pieces_in(label, allocator) }
                }
            };
            if image {
                push(
                    out,
                    Inline::Image(ArenaBox::new_in(
                        Image {
                            kind,
                            alt: input.pieces_in(text, allocator),
                            children: inner,
                            span,
                        },
                        &allocator,
                    )),
                );
            } else {
                push(
                    out,
                    Inline::Link(ArenaBox::new_in(
                        Link { kind, children: inner, span },
                        &allocator,
                    )),
                );
            }
        }
    }
}

/// Coarse CJK detection: the lexical fact only.
/// Classification (CJ vs K vs CJK punctuation) is formatter policy.
fn is_cjk(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x11FF     // Hangul Jamo
        | 0x2E80..=0x303F   // CJK radicals, Kangxi, CJK punctuation
        | 0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3130..=0x318F   // Hangul compatibility Jamo
        | 0x31F0..=0x4DBF   // Katakana ext, CJK ext A
        | 0x4E00..=0x9FFF   // CJK unified
        | 0xAC00..=0xD7AF   // Hangul syllables
        | 0xF900..=0xFAFF   // CJK compatibility
        | 0xFE30..=0xFE4F   // CJK compatibility forms
        | 0xFF00..=0xFFEF   // Full/half-width forms
        | 0x20000..=0x3FFFD // CJK ext B+
    )
}
