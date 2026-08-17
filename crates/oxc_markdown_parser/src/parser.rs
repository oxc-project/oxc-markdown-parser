//! Parser entry point.
//!
//! Two-phase design:
//! a block phase resolves the container structure and hands each leaf's logical content
//! (container prefixes (`>`, list indents) already stripped, as segments over the original source)
//! to an inline phase, together with the reference-definition map collected while blocks were finalized.
//! Inline spans always point into the original source.
//!
//! Construct toggles hook into three fixed decision points of those loops:
//!
//! - `<` at flow start / in text: HTML block / inline HTML vs MDX JSX
//! - `{` in flow / text: literal text (or liquid) vs MDX expression
//! - 4-space indent at flow start: indented code vs paragraph continuation
//!
//! Everything else (GFM, math, wiki-link, liquid, directives) is an ordinary construct
//! added to the block or inline scan under its flag.

use oxc_allocator::{Allocator, ArenaVec};

use crate::ast::Root;
use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::options::ParserOptions;
use crate::pos::Span;

/// The outcome of a parse: an AST plus diagnostics.
///
/// Parsing never fails.
/// CommonMark has no syntax errors,
/// and MDX-mode errors are reported here while the parse recovers.
#[derive(Debug)]
pub struct ParserReturn<'a> {
    pub root: Root<'a>,
    pub diagnostics: Vec<Diagnostic>,
    /// Every logical blank line, in source order:
    /// the span after the container prefixes (empty for a bare `>` line inside a blockquote),
    /// up to the line ending.
    /// Lines inside fenced blocks are content, never listed;
    /// blank lines inside indented code are listed (they are only content if more code follows),
    /// consumers compare against node spans.
    /// Blank runs between siblings carry meaning (HTML verbatim boundaries, loose lists);
    /// exposing the table the block phase already built saves consumers a container-aware rescan.
    /// (List tightness itself is already on [`crate::ast::List::tight`],
    /// computed from the subset that can loosen a list.)
    pub blanks: Vec<Span>,
}

pub struct Parser<'a> {
    allocator: &'a Allocator,
    source: &'a str,
    options: ParserOptions,
}

impl<'a> Parser<'a> {
    /// Markdown mode with the default constructs.
    pub fn new(allocator: &'a Allocator, source: &'a str) -> Self {
        Self::with_options(allocator, source, ParserOptions::default())
    }

    pub fn with_options(allocator: &'a Allocator, source: &'a str, options: ParserOptions) -> Self {
        Self { allocator, source, options }
    }

    /// Parse the source into a [`Root`].
    ///
    /// Front matter is not handled here: callers strip it before parsing
    /// (offsets in the AST are relative to what was passed in).
    pub fn parse(self) -> ParserReturn<'a> {
        let mut diagnostics = vec![];
        if u32::try_from(self.source.len()).is_err() {
            diagnostics.push(Diagnostic::new(DiagnosticKind::SourceTooLong, Span::empty(0)));
            return ParserReturn {
                root: Root { children: ArenaVec::new_in(&self.allocator), span: Span::empty(0) },
                diagnostics,
                blanks: vec![],
            };
        }

        #[expect(clippy::cast_possible_truncation)]
        let source_len = self.source.len() as u32;
        let engine = crate::block::Engine::new(self.source, self.options.constructs);
        let (irs, blanks, refs) = engine.run();
        let root = crate::block::build(
            self.allocator,
            self.source,
            &blanks.loosening,
            irs,
            Span::new(0, source_len),
            &self.options.constructs,
            &refs,
        );
        ParserReturn { root, diagnostics, blanks: blanks.all }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Block, HeadingKind};

    /// Every blank line is listed; only some loosen the list:
    /// a bare `>` line inside a blockquote does not (it still ends an HTML block for consumers).
    #[test]
    fn blank_lines_listed_vs_loosening() {
        for (source, blank_at, tight) in
            [("- > a\n  >\n  > b\n", 9, true), ("- a\n\n- b\n", 4, false)]
        {
            let allocator = Allocator::default();
            let ret = Parser::new(&allocator, source).parse();
            assert_eq!(ret.blanks, vec![Span::empty(blank_at)], "{source:?}");
            let Some(Block::List(list)) = ret.root.children.first() else { panic!("list") };
            assert_eq!(list.tight, tight, "{source:?}");
        }
    }

    /// A lazy line absorbed into a setext heading stays recorded.
    #[test]
    fn setext_heading_keeps_lazy_lines() {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, "> a\nb\n> ===\n").parse();
        let Some(Block::Blockquote(quote)) = ret.root.children.first() else { panic!("quote") };
        let Some(Block::Heading(heading)) = quote.children.first() else { panic!("heading") };
        let HeadingKind::Setext { lazy_lines, .. } = &heading.kind else { panic!("setext") };
        assert_eq!(lazy_lines.as_slice(), &[Span::new(4, 5)]);
    }
}
