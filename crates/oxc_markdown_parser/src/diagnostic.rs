//! Parse diagnostics.
//!
//! CommonMark has no syntax errors, so markdown-mode parses always return an empty diagnostics list;
//! the list exists for MDX mode (unclosed JSX tags, stray `<`/`{`, malformed expressions, …)
//! and for input-level limits.
//! Diagnostics never abort the parse, an AST is always produced.

use crate::pos::Span;
use std::fmt::Display;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub span: Span,
}

impl Diagnostic {
    pub(crate) fn new(kind: DiagnosticKind, span: Span) -> Self {
        Self { kind, span }
    }
}

// MDX mode will add variants (unclosed JSX, malformed expressions, …);
// non_exhaustive keeps those additions from being breaking changes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// The source exceeds the maximum supported size (4 GiB).
    SourceTooLong,
}

impl Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match &self.kind {
            DiagnosticKind::SourceTooLong => "source exceeds the maximum supported size (4 GiB)",
        };
        write!(f, "{message} at {}..{}", self.span.start, self.span.end)
    }
}
