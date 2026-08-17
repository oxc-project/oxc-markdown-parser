//! The engine's open state: the container stack entries, the current leaf,
//! and the blank-line tables it accumulates.

use crate::ast::TableAlign;
use crate::block::ir::{Ir, IrItem, IrRow, IrSeg};
use crate::pos::{Segment, Span};

pub(super) enum OpenContainer {
    Quote {
        children: Vec<Ir>,
        start: u32,
        end: u32,
    },
    List {
        items: Vec<IrItem>,
        ordered: bool,
        marker: u8,
        start: u32,
    },
    Item {
        children: Vec<Ir>,
        marker: Span,
        padding: u8,
        /// Columns from the parent's content start to this item's content.
        content_indent: usize,
        start: u32,
        end: u32,
    },
    /// GFM footnote definition: continuation is `content_indent` (always 4) columns of indent
    /// or a blank line; it stays open across blanks, with no empty-close rule.
    Footnote {
        children: Vec<Ir>,
        label: Span,
        content_indent: usize,
        start: u32,
        end: u32,
    },
    /// A container directive (`:::name` … `:::`).
    /// Content lines need no prefix (blank lines included);
    /// up to `content_indent` columns (the opening fence's indent) are stripped.
    /// A closing-fence line (checked outermost-first, before deeper containers) or a lazy line ends it.
    Directive {
        children: Vec<Ir>,
        fence_len: u32,
        content_indent: usize,
        opening: Span,
        closing: Option<Span>,
        end: u32,
    },
}

pub(super) enum Leaf {
    /// The span end is always the last segment's end
    /// (segments end at their line's content end; the last one is trimmed at close),
    /// so no `end` field is carried.
    Paragraph {
        segments: Vec<IrSeg>,
        /// The last (non-lazy) line carried 4+ columns of indent;
        /// such a line is never stolen as a table header row.
        last_deep: bool,
    },
    IndentedCode {
        lines: Vec<Segment>,
        /// Blank lines buffered until more content arrives (trailing blanks are not part of the block).
        pending: Vec<Segment>,
        start: u32,
        end: u32,
    },
    /// A fenced verbatim leaf:
    /// code (`` ` ``/`~`) and math (`$`) share the continuation rule (indent stripping, ≥3-column close, lazy ends it);
    /// they diverge only at `close_leaf`.
    /// `info` is the info string for code, the meta text for math.
    FencedCode {
        fence: u8,
        len: u32,
        /// Indent of the opening fence; up to this many columns are stripped from content lines.
        indent: usize,
        info: Option<Span>,
        lines: Vec<Segment>,
        start: u32,
        end: u32,
    },
    Html {
        kind: u8,
        lines: Vec<Segment>,
        /// The block's final line ending belongs to its content
        /// (micromark quirk; set when a container mismatch closes the block).
        trailing_newline: bool,
        start: u32,
        end: u32,
    },
    Table {
        align: Vec<TableAlign>,
        rows: Vec<IrRow>,
        start: u32,
        end: u32,
    },
}

/// Outcome of matching a line against the open container stack.
pub(super) enum StackMatch {
    /// How many containers matched (prefixes consumed on the cursor).
    Prefixes(usize),
    /// The line is the closing fence of the open directive at `index`;
    /// it consumes the whole line.
    DirectiveClose { index: usize, fence: Span },
}

/// Outcome of offering a line to the open non-verbatim leaf (step 4b).
pub(super) enum Continuation {
    /// The leaf took the line.
    Consumed,
    /// The line goes on to open blocks; a paragraph that refused it stays open to be interrupted.
    Open,
}

/// The blank-line tables the line loop builds, both in source order.
#[derive(Default)]
pub struct BlankLines {
    /// Every logical blank line; documented on `ParserReturn::blanks`.
    pub all: Vec<Span>,
    /// The blank lines that can make a list loose:
    /// `all` minus the lines whose innermost open container is a blockquote
    /// (cmark's `last_line_blank` exception: `> a\n>\n> b` stays tight inside an item).
    pub loosening: Vec<Span>,
}
