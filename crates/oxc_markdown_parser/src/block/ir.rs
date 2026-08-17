//! Intermediate block tree.
//!
//! The block phase runs to completion before the inline phase
//! (the reference-definition map must be complete first),
//! so its output is this lightweight tree holding leaf content as segments.
//! The builder turns it into the arena AST and runs the inline phase per leaf.

use crate::ast::Destination;
use crate::pos::{Segment, Span};

pub struct IrSeg {
    pub seg: Segment,
    pub lazy: bool,
}

pub enum Ir {
    Paragraph {
        segments: Vec<IrSeg>,
        span: Span,
    },
    Heading {
        level: u8,
        /// `None` for ATX; the underline span for setext.
        underline: Option<Span>,
        segments: Vec<IrSeg>,
        span: Span,
    },
    ThematicBreak {
        span: Span,
    },
    Code {
        /// `None` for indented code.
        fenced: Option<(u8, Option<Span>)>,
        lines: Vec<Segment>,
        span: Span,
    },
    Html {
        kind: u8,
        lines: Vec<Segment>,
        /// See [`crate::ast::HtmlBlock::trailing_newline`].
        trailing_newline: bool,
        span: Span,
    },
    Quote {
        children: Vec<Ir>,
        span: Span,
    },
    List {
        ordered: bool,
        /// Bullet char or ordered delimiter.
        marker: u8,
        items: Vec<IrItem>,
        span: Span,
    },
    Definition {
        label: Vec<Segment>,
        destination: Destination,
        title: Option<Vec<Segment>>,
        span: Span,
    },
    Table {
        align: Vec<crate::ast::TableAlign>,
        /// Row 0 is the header; the delimiter row leaves only `align`.
        rows: Vec<IrRow>,
        span: Span,
    },
    FootnoteDefinition {
        label: Span,
        children: Vec<Ir>,
        span: Span,
    },
    MathBlock {
        meta: Option<Span>,
        lines: Vec<Segment>,
        span: Span,
    },
    Liquid {
        pieces: Vec<Segment>,
        span: Span,
    },
    ContainerDirective {
        opening: Span,
        closing: Option<Span>,
        children: Vec<Ir>,
        span: Span,
    },
}

pub struct IrRow {
    pub span: Span,
    pub cells: Vec<Span>,
}

pub struct IrItem {
    pub marker: Span,
    pub padding: u8,
    pub children: Vec<Ir>,
    pub span: Span,
}

impl Ir {
    pub fn span(&self) -> Span {
        match self {
            Ir::Paragraph { span, .. }
            | Ir::Heading { span, .. }
            | Ir::ThematicBreak { span }
            | Ir::Code { span, .. }
            | Ir::Html { span, .. }
            | Ir::Quote { span, .. }
            | Ir::List { span, .. }
            | Ir::Definition { span, .. }
            | Ir::Table { span, .. }
            | Ir::FootnoteDefinition { span, .. }
            | Ir::MathBlock { span, .. }
            | Ir::Liquid { span, .. }
            | Ir::ContainerDirective { span, .. } => *span,
        }
    }
}
