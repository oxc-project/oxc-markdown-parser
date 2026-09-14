//! Lexical predicates: grammar facts for formatters.
//!
//! A markdown formatter constantly synthesizes new line starts
//! (joining lines for prose wrap, stripping indent, removing escapes).
//! Whether the synthesized line would open a block is a grammar fact,
//! and approximating it downstream is the classic printer failure mode
//! (Prettier's `/^>|^(?:[*+-]|#{1,6}|\d+[).])$/` misses `:::`, fences, `<`, and `|`.
//! Each a documented meaning-breaking bug).
//! This module exposes the parser's own classification instead:
//! [`line_start`] runs the exact probe table the block engine uses, so the two cannot drift.
//!
//! Policy-free: these functions state what the grammar would do with a line,
//! never what a formatter should do about it.

use crate::options::Constructs;

use super::probe::{Start, probe};
use super::scan;

/// What a line would open at flow position.
/// One variant per block construct;
/// the payload-free mirror of the engine's probe result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineStart {
    Blockquote,
    /// Only reported with `in_paragraph` (an underline needs a paragraph).
    SetextUnderline,
    ThematicBreak,
    AtxHeading,
    CodeFence,
    HtmlBlock,
    MathFence,
    Liquid,
    /// A directive opening fence (`:::name…`): opens a container, interrupts a paragraph.
    DirectiveOpener,
    /// A bare `:::` run: closes an open directive, inert otherwise.
    DirectiveCloser,
    FootnoteDefinition,
    ListItem,
    /// Only reported with `in_paragraph`
    /// (a delimiter row activates by stealing the paragraph's last line as the header).
    TableDelimiterRow,
    /// A `|`-initiated line: a row if a table is open or forming.
    TableRow,
}

/// The number of an ordered list marker (`123.` / `123)`), for a printer that renumbers.
/// CommonMark caps markers at 9 digits, so the value always fits; `None` for a bullet.
pub fn ordered_number(marker: &str) -> Option<u32> {
    marker.trim_end_matches(['.', ')']).parse().ok()
}

/// Classifies what `line` (one line, no line ending) would start as a block,
/// or `None` for paragraph text.
/// `in_paragraph` enables the constructs that only exist relative to an open paragraph.
///
/// Leading indent is handled here: 4+ columns of indent is indented code (never a block start),
/// per the default `code_indented` construct.
pub fn line_start(constructs: &Constructs, line: &str, in_paragraph: bool) -> Option<LineStart> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    // 4+ spaces, or a tab anywhere in the first ≤3 columns
    // (a tab there always advances past column 4): indented code, not a block start.
    if indent >= 4 || line.as_bytes().get(indent) == Some(&b'\t') {
        return None;
    }
    let tail = &line[indent..];
    if let Some(start) = probe(constructs, tail, in_paragraph) {
        return Some(match start {
            Start::Quote => LineStart::Blockquote,
            Start::Setext(_) => LineStart::SetextUnderline,
            Start::ThematicBreak => LineStart::ThematicBreak,
            Start::Atx { .. } => LineStart::AtxHeading,
            Start::Fence { .. } => LineStart::CodeFence,
            Start::Html { .. } => LineStart::HtmlBlock,
            Start::MathFence { .. } => LineStart::MathFence,
            Start::Liquid => LineStart::Liquid,
            Start::Directive { .. } => LineStart::DirectiveOpener,
            Start::FootnoteDef { .. } => LineStart::FootnoteDefinition,
            Start::ListItem { .. } => LineStart::ListItem,
            Start::TableDelimiter(_) => LineStart::TableDelimiterRow,
        });
    }
    // Below the probe table:
    // line shapes that are inert on their own but significant next to an open construct.
    // Exactly what a formatter must not create by joining lines.
    if constructs.container_directive && scan::fence_close(tail, b':', 3) {
        return Some(LineStart::DirectiveCloser);
    }
    if constructs.gfm_table && tail.starts_with('|') {
        return Some(LineStart::TableRow);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(line: &str, in_paragraph: bool) -> Option<LineStart> {
        line_start(&Constructs::markdown(), line, in_paragraph)
    }

    #[test]
    fn probe_table_mirrors_engine() {
        assert_eq!(md("> q", false), Some(LineStart::Blockquote));
        assert_eq!(md("---", true), Some(LineStart::SetextUnderline));
        assert_eq!(md("---", false), Some(LineStart::ThematicBreak));
        assert_eq!(md("# h", false), Some(LineStart::AtxHeading));
        assert_eq!(md("```rs", false), Some(LineStart::CodeFence));
        assert_eq!(md("<div>", false), Some(LineStart::HtmlBlock));
        assert_eq!(md("$$", false), Some(LineStart::MathFence));
        assert_eq!(md("{% t %}", false), Some(LineStart::Liquid));
        assert_eq!(md("[^1]: n", false), Some(LineStart::FootnoteDefinition));
        assert_eq!(md("- item", false), Some(LineStart::ListItem));
        assert_eq!(md("plain text", true), None);
    }

    #[test]
    fn ordered_numbers() {
        assert_eq!(ordered_number("007."), Some(7));
        assert_eq!(ordered_number("999999999)"), Some(999_999_999));
        assert_eq!(ordered_number("-"), None);
    }

    #[test]
    fn covers_prettiers_documented_misses() {
        // The regex holes behind Blume's directive corruption and #19847.
        assert_eq!(md(":::note", false), Some(LineStart::DirectiveOpener));
        assert_eq!(md(":::", false), Some(LineStart::DirectiveCloser));
        assert_eq!(md(":::)", false), None, "an emoticon is paragraph text");
        assert_eq!(md("| - | - |", true), Some(LineStart::TableDelimiterRow));
        assert_eq!(md("| a | b |", false), Some(LineStart::TableRow));
    }

    #[test]
    fn indent_is_indented_code() {
        assert_eq!(md("    # not a heading", false), None);
        assert_eq!(md("\t- not a list", false), None);
        assert_eq!(md("  \t- not a list", false), None);
        assert_eq!(md("   - list", false), Some(LineStart::ListItem));
    }

    /// The rows below the probe table carry their own construct gates
    /// (the probe rows are pinned in `probe`'s tests).
    #[test]
    fn construct_gating() {
        let mut c = Constructs::markdown();
        c.container_directive = false;
        c.gfm_table = false;
        assert_eq!(line_start(&c, ":::", false), None);
        assert_eq!(line_start(&c, "| a |", false), None);
    }
}
