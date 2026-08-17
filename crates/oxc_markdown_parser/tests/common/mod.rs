//! One walk over the AST for both integration tests:
//! the snapshot printer and the span-invariant checker are `Visitor`s
//! fed the same node names, spans and facts.
//!
//! Every node is destructured exhaustively, so a field added to `ast.rs`
//! fails to compile here until it is classified (snapshotted / checked / ignored).

// The checker never reads `Fact::Str` payloads.
#![allow(dead_code)]

use oxc_markdown_parser::{Segment, Span, ast::*};

/// A node's non-child facts: extra spans and segments it owns, and style facts.
pub enum Fact<'a> {
    Span(&'static str, Span),
    Spans(&'static str, &'a [Span]),
    Segments(&'static str, &'a [Segment]),
    Str(&'static str, String),
}

pub trait Visitor {
    fn enter(&mut self, name: &'static str, span: Span, facts: Vec<Fact<'_>>);
    fn exit(&mut self);
}

pub fn walk_root(root: &Root<'_>, v: &mut impl Visitor) {
    let Root { children, span } = root;
    v.enter("Root", *span, vec![]);
    walk_blocks(children, v);
    v.exit();
}

fn walk_blocks(blocks: &[Block<'_>], v: &mut impl Visitor) {
    for block in blocks {
        walk_block(block, v);
    }
}

fn optional(key: &'static str, span: Option<Span>) -> Option<Fact<'static>> {
    span.map(|s| Fact::Span(key, s))
}

fn walk_block(block: &Block<'_>, v: &mut impl Visitor) {
    match block {
        Block::Paragraph(n) => {
            let Paragraph { children, lazy_lines, span } = &**n;
            v.enter("Paragraph", *span, vec![Fact::Spans("lazy", lazy_lines)]);
            walk_inlines(children, v);
        }
        Block::Heading(n) => {
            let Heading { level, kind, children, span } = &**n;
            let mut facts = vec![Fact::Str("level", level.to_string())];
            match kind {
                HeadingKind::Atx => facts.push(Fact::Str("kind", "atx".into())),
                HeadingKind::Setext { underline, lazy_lines } => {
                    facts.push(Fact::Str("kind", "setext".into()));
                    facts.push(Fact::Span("underline", *underline));
                    facts.push(Fact::Spans("lazy", lazy_lines));
                }
            }
            v.enter("Heading", *span, facts);
            walk_inlines(children, v);
        }
        Block::ThematicBreak(ThematicBreak { span }) => v.enter("ThematicBreak", *span, vec![]),
        Block::CodeBlock(n) => {
            let CodeBlock { kind, lines, span } = &**n;
            let mut facts = match *kind {
                CodeBlockKind::Fenced { fence, info } => {
                    let mut facts = vec![Fact::Str("fence", (fence as char).to_string())];
                    facts.extend(optional("info", info));
                    facts
                }
                CodeBlockKind::Indented => vec![Fact::Str("kind", "indented".into())],
            };
            facts.push(Fact::Segments("lines", lines));
            v.enter("CodeBlock", *span, facts);
        }
        Block::HtmlBlock(n) => {
            let HtmlBlock { kind, lines, trailing_newline, span } = &**n;
            v.enter(
                "HtmlBlock",
                *span,
                vec![
                    Fact::Str("kind", kind.to_string()),
                    Fact::Str("trailing_newline", trailing_newline.to_string()),
                    Fact::Segments("lines", lines),
                ],
            );
        }
        Block::Blockquote(n) => {
            let Blockquote { children, span } = &**n;
            v.enter("Blockquote", *span, vec![]);
            walk_blocks(children, v);
        }
        Block::List(n) => {
            let List { marker, tight, children, span } = &**n;
            let marker = match *marker {
                ListMarker::Bullet { marker } => format!("bullet {:?}", marker as char),
                ListMarker::Ordered { delimiter } => format!("ordered {:?}", delimiter as char),
            };
            v.enter(
                "List",
                *span,
                vec![Fact::Str("marker", marker), Fact::Str("tight", tight.to_string())],
            );
            for item in children {
                let ListItem { marker, padding, checkbox, spread, children, span } = item;
                let mut facts = vec![
                    Fact::Span("marker", *marker),
                    Fact::Str("padding", padding.to_string()),
                    Fact::Str("spread", spread.to_string()),
                ];
                if let Some(TaskCheckbox { span, checked }) = checkbox {
                    facts.push(Fact::Span("checkbox", *span));
                    facts.push(Fact::Str("checked", checked.to_string()));
                }
                v.enter("ListItem", *span, facts);
                walk_blocks(children, v);
                v.exit();
            }
        }
        Block::Definition(n) => {
            let Definition { label, destination, title, span } = &**n;
            let mut facts = vec![Fact::Segments("label", label)];
            facts.extend(destination_facts(destination));
            facts.extend(title.as_ref().map(|t| Fact::Segments("title", t.as_slice())));
            v.enter("Definition", *span, facts);
        }
        Block::Table(n) => {
            let Table { align, children, span } = &**n;
            let align: Vec<_> = align.iter().collect();
            v.enter("Table", *span, vec![Fact::Str("align", format!("{align:?}"))]);
            for row in children {
                let TableRow { children, span } = row;
                v.enter("TableRow", *span, vec![]);
                for cell in children {
                    let TableCell { children, span } = cell;
                    v.enter("TableCell", *span, vec![]);
                    walk_inlines(children, v);
                    v.exit();
                }
                v.exit();
            }
        }
        Block::FootnoteDefinition(n) => {
            let FootnoteDefinition { label, children, span } = &**n;
            v.enter("FootnoteDefinition", *span, vec![Fact::Span("label", *label)]);
            walk_blocks(children, v);
        }
        Block::MathBlock(n) => {
            let MathBlock { meta, lines, span } = &**n;
            let mut facts = vec![];
            facts.extend(optional("meta", *meta));
            facts.push(Fact::Segments("lines", lines));
            v.enter("MathBlock", *span, facts);
        }
        Block::Liquid(n) => {
            let Liquid { pieces, span } = &**n;
            v.enter("Liquid", *span, vec![Fact::Segments("pieces", pieces)]);
        }
        Block::ContainerDirective(n) => {
            let ContainerDirective { opening, closing, children, span } = &**n;
            let mut facts = vec![Fact::Span("opening", *opening)];
            facts.extend(optional("closing", *closing));
            v.enter("ContainerDirective", *span, facts);
            walk_blocks(children, v);
        }
        Block::MdxEsm(MdxEsm { span }) => v.enter("MdxEsm", *span, vec![]),
        Block::MdxExpression(MdxExpression { span }) => v.enter("MdxExpression", *span, vec![]),
        Block::MdxJsx(n) => {
            let MdxJsxFlow { opening, closing, children, span } = &**n;
            let mut facts = vec![Fact::Span("opening", *opening)];
            facts.extend(optional("closing", *closing));
            v.enter("MdxJsx", *span, facts);
            walk_blocks(children, v);
        }
    }
    v.exit();
}

fn walk_inlines(inlines: &[Inline<'_>], v: &mut impl Visitor) {
    for inline in inlines {
        walk_inline(inline, v);
    }
}

fn destination_facts(destination: &Destination) -> [Fact<'static>; 2] {
    let Destination { span, angle_bracketed } = destination;
    [Fact::Span("dest", *span), Fact::Str("angle", angle_bracketed.to_string())]
}

fn link_facts<'a>(kind: &'a LinkKind<'a>) -> Vec<Fact<'a>> {
    match kind {
        LinkKind::Inline { destination, title } => {
            let mut facts = vec![Fact::Str("kind", "inline".into())];
            facts.extend(destination_facts(destination));
            facts.extend(title.as_ref().map(|t| Fact::Segments("title", t.as_slice())));
            facts
        }
        LinkKind::Reference { kind, label } => {
            let kind = match kind {
                ReferenceKind::Full => "full",
                ReferenceKind::Collapsed => "collapsed",
                ReferenceKind::Shortcut => "shortcut",
            };
            vec![Fact::Str("kind", kind.into()), Fact::Segments("label", label)]
        }
    }
}

fn walk_inline(inline: &Inline<'_>, v: &mut impl Visitor) {
    match inline {
        Inline::Text(Text { span, ascii_only, contains_cjk }) => {
            let mut facts = vec![];
            if !ascii_only {
                facts.push(Fact::Str("non_ascii", String::new()));
            }
            if *contains_cjk {
                facts.push(Fact::Str("cjk", String::new()));
            }
            v.enter("Text", *span, facts);
        }
        Inline::SoftBreak(SoftBreak { span }) => v.enter("SoftBreak", *span, vec![]),
        Inline::HardBreak(HardBreak { kind, span }) => {
            let kind = match kind {
                HardBreakKind::Spaces => "spaces",
                HardBreakKind::Backslash => "backslash",
            };
            v.enter("HardBreak", *span, vec![Fact::Str("kind", kind.into())]);
        }
        Inline::Emphasis(n) => {
            let Emphasis { marker, children, span } = &**n;
            v.enter("Emphasis", *span, vec![Fact::Str("marker", (*marker as char).to_string())]);
            walk_inlines(children, v);
        }
        Inline::Strong(n) => {
            let Strong { children, span } = &**n;
            v.enter("Strong", *span, vec![]);
            walk_inlines(children, v);
        }
        Inline::Strikethrough(n) => {
            let Strikethrough { children, span } = &**n;
            v.enter("Strikethrough", *span, vec![]);
            walk_inlines(children, v);
        }
        Inline::CodeSpan(n) => {
            let CodeSpan { pieces, span } = &**n;
            v.enter("CodeSpan", *span, vec![Fact::Segments("pieces", pieces)]);
        }
        Inline::Link(n) => {
            let Link { kind, children, span } = &**n;
            v.enter("Link", *span, link_facts(kind));
            walk_inlines(children, v);
        }
        Inline::Image(n) => {
            let Image { kind, children, span } = &**n;
            v.enter("Image", *span, link_facts(kind));
            walk_inlines(children, v);
        }
        Inline::Autolink(Autolink { span, email }) => {
            v.enter("Autolink", *span, vec![Fact::Str("email", email.to_string())]);
        }
        Inline::AutolinkLiteral(AutolinkLiteral { span }) => {
            v.enter("AutolinkLiteral", *span, vec![]);
        }
        Inline::HtmlInline(n) => {
            let HtmlInline { pieces, span } = &**n;
            v.enter("HtmlInline", *span, vec![Fact::Segments("pieces", pieces)]);
        }
        Inline::FootnoteReference(n) => {
            let FootnoteReference { label, span } = &**n;
            v.enter("FootnoteReference", *span, vec![Fact::Span("label", *label)]);
        }
        Inline::MathSpan(n) => {
            let MathSpan { pieces, span } = &**n;
            v.enter("MathSpan", *span, vec![Fact::Segments("pieces", pieces)]);
        }
        Inline::WikiLink(WikiLink { span }) => v.enter("WikiLink", *span, vec![]),
        Inline::Liquid(n) => {
            let Liquid { pieces, span } = &**n;
            v.enter("Liquid", *span, vec![Fact::Segments("pieces", pieces)]);
        }
        Inline::MdxExpression(MdxExpression { span }) => v.enter("MdxExpression", *span, vec![]),
        Inline::MdxJsx(n) => {
            let MdxJsxText { opening, closing, children, span } = &**n;
            let mut facts = vec![Fact::Span("opening", *opening)];
            facts.extend(optional("closing", *closing));
            v.enter("MdxJsx", *span, facts);
            walk_inlines(children, v);
        }
    }
    v.exit();
}
