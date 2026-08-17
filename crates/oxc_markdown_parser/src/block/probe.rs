//! The table of block starts.
//!
//! [`probe`] classifies what begins at a flow position (past the ≤3-column indent),
//! in CommonMark's priority order, with construct toggles gating their rows here and nowhere else.
//! The engine's open loop, its lazy-continuation test, the liquid interrupt check
//! and the HTML line-ending quirk all consume it;
//! [`super::lexical`] exposes the same classification to formatters, so engine and public API cannot drift.
//!
//! A new block construct is one [`Start`] variant, one row here, and one opener in the engine.

use std::ops::Range;

use crate::ast::TableAlign;
use crate::options::Constructs;
use crate::syntax;

use super::scan;

/// What starts at a flow position.
pub enum Start {
    Quote,
    /// Only probed while a paragraph is open in the current container.
    Setext(u8),
    ThematicBreak,
    Atx {
        level: u8,
        content: Range<usize>,
    },
    Fence {
        fence: u8,
        len: u32,
        info: Range<usize>,
    },
    Html {
        kind: u8,
    },
    ListItem {
        m: scan::ListMarkerScan,
        empty: bool,
    },
    /// Probed only while a paragraph is open:
    /// the header row is the paragraph's last line, validated at the open site.
    TableDelimiter(Vec<TableAlign>),
    FootnoteDef {
        label: Range<usize>,
        content: usize,
    },
    MathFence {
        len: u32,
        meta: Range<usize>,
    },
    /// The probe row checks only the two-byte opener, mirroring micromark's cheap interrupt check;
    /// the open site validates (and consumes) the full construct.
    Liquid,
    /// Fully validated fence line (name + optional label/attributes);
    /// interrupts like fenced code (micromark: `concrete`).
    Directive {
        len: u32,
    },
}

impl Start {
    /// micromark's `document` constructs, which interrupt a non-`concrete` flow attempt (liquid)
    /// mid-construct: blockquote, a list item under the paragraph-interrupt restrictions,
    /// and a GFM footnote definition. Directives are flow constructs and don't count.
    pub(super) fn is_container(&self) -> bool {
        match self {
            Start::Quote | Start::FootnoteDef { .. } => true,
            Start::ListItem { m, empty } => item_interrupts(m, *empty),
            _ => false,
        }
    }
}

/// Classifies the block construct starting at `tail` (a flow position).
/// Priority order is CommonMark's (setext before thematic break before list marker).
/// `para_open` folds in the constructs that only exist relative to an open paragraph (setext)
/// or are barred by one (HTML type 7).
pub fn probe(constructs: &Constructs, tail: &str, para_open: bool) -> Option<Start> {
    if tail.starts_with('>') {
        return Some(Start::Quote);
    }
    if para_open && let Some(level) = scan::setext_underline(tail) {
        return Some(Start::Setext(level));
    }
    if scan::thematic_break(tail) {
        return Some(Start::ThematicBreak);
    }
    if let Some((level, content)) = scan::atx_heading(tail) {
        return Some(Start::Atx { level, content });
    }
    if let Some((fence, len, info)) = scan::fence_open(tail) {
        return Some(Start::Fence { fence, len, info });
    }
    if constructs.html_flow
        && let Some(kind) = scan::html_block_start(tail, para_open)
    {
        return Some(Start::Html { kind });
    }
    if constructs.math_flow
        && let Some((len, meta)) = scan::math_fence_open(tail)
    {
        return Some(Start::MathFence { len, meta });
    }
    if constructs.liquid && syntax::liquid::open(tail).is_some() {
        return Some(Start::Liquid);
    }
    if constructs.container_directive
        && let Some(len) = scan::directive_fence_open(tail)
    {
        return Some(Start::Directive { len });
    }
    if constructs.gfm_footnote
        && let Some((label, content)) = scan::footnote_definition_start(tail)
    {
        return Some(Start::FootnoteDef { label, content });
    }
    if let Some(m) = scan::list_marker(tail) {
        let empty = scan::is_blank(&tail[m.len..]);
        return Some(Start::ListItem { m, empty });
    }
    if para_open
        && constructs.gfm_table
        && let Some(align) = scan::table_delimiter_row(tail)
    {
        return Some(Start::TableDelimiter(align));
    }
    None
}

/// The paragraph-interrupt restriction on list items (micromark):
/// an interrupting item must be non-empty and, if ordered, start at 1.
pub(super) fn item_interrupts(m: &scan::ListMarkerScan, empty: bool) -> bool {
    !empty && (!m.ordered || m.starts_at_one)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_gate_their_rows() {
        let on = Constructs::markdown();
        let off = Constructs {
            html_flow: false,
            math_flow: false,
            liquid: false,
            container_directive: false,
            gfm_footnote: false,
            gfm_table: false,
            ..on
        };
        for (tail, para_open) in [
            ("<div>", false),
            ("$$", false),
            ("{% t %}", false),
            (":::a", false),
            ("[^1]: n", false),
            ("| - |", true),
        ] {
            assert!(probe(&on, tail, para_open).is_some(), "{tail}");
            assert!(probe(&off, tail, para_open).is_none(), "{tail}");
        }
        assert!(matches!(probe(&off, "> q", false), Some(Start::Quote)), "core stays on");
    }

    /// A delimiter row exists only relative to an open paragraph.
    #[test]
    fn table_delimiter_needs_a_paragraph() {
        assert!(probe(&Constructs::markdown(), "| - |", false).is_none());
    }
}
