//! oxc-markdown-parser is a CommonMark + GFM (and eventually MDX v3) parser
//! that produces a span-faithful, style-preserving typed AST, designed for building formatters.
//!
//! Parse behavior targets micromark (= CommonMark + GFM as GitHub renders it).
//! Style facts are first-class AST fields;
//! consumers slice the original source through spans instead of reading cooked values.
//!
//! ## Basic Usage
//!
//! ```rust
//! use oxc_markdown_parser::{Allocator, Parser};
//!
//! let allocator = Allocator::default();
//! let parser = Parser::new(&allocator, "# Hello\n");
//! let ret = parser.parse();
//! // Markdown mode is structurally diagnostic-free.
//! assert!(ret.diagnostics.is_empty());
//! ```

pub mod ast;
mod block;
mod diagnostic;
mod inline;
mod options;
mod parser;
mod pos;
mod syntax;

pub use block::lexical;
pub use diagnostic::{Diagnostic, DiagnosticKind};
pub use inline::{LiteralKind, literal_kind};
pub use options::{Constructs, ParserOptions};
pub use oxc_allocator::Allocator;
pub use parser::{Parser, ParserReturn};
pub use pos::{Segment, Span};
pub use syntax::{decode, label, unicode};

/// Size regression guards, in the spirit of oxc_ast's generated assertions:
/// enums stay pointer-sized-plus-tag, and hot node types stay small.
#[cfg(all(test, target_pointer_width = "64"))]
mod size_asserts {
    use crate::ast;

    #[test]
    fn sizes() {
        // Both content enums:
        // tag + arena Box (or an inline Copy payload of at most the same footprint).
        assert_eq!(size_of::<ast::Block>(), 16);
        assert_eq!(size_of::<ast::Inline>(), 16);
        // The hottest inline node stays inline in the enum.
        assert_eq!(size_of::<ast::Text>(), 12);
    }
}
