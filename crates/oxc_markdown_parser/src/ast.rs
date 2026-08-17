//! AST node definitions.
//!
//! Node taxonomy (heading / paragraph / list / html / emphasis / …),
//! sibling order and spans follow CommonMark's grammar;
//! a printer's dispatch is written per this unit.
//!
//! Beyond that the shapes deliberately diverge from mdast:
//!
//! - No cooked values: consumers slice the original source through [`Span`]s.
//!   Inline node spans are original-source offsets, never rebuilt-buffer ones.
//! - Style facts a printer would otherwise re-derive by peeking at the source
//!   (markers, fence chars, reference kinds, break kinds, …) are first-class fields.
//! - Formatter policy (sentence splitting, CJK classification, alignment judgment) stays out;
//!   the AST records lexical facts only.
//!
//! Blank-line facts (loose lists, verbatim/markdown flips around HTML blocks) are not stored:
//! sibling spans always allow counting the blank lines between them.

use oxc_allocator::{ArenaBox, ArenaVec};

use crate::pos::{Segment, Span};

/// The document.
#[derive(Debug)]
pub struct Root<'a> {
    pub children: ArenaVec<'a, Block<'a>>,
    pub span: Span,
}

/// Flow (block-level) content.
// Variant names mirror the node struct names;
// the Block/Inline suffixes disambiguate the flow/phrasing pairs (CodeBlock vs CodeSpan, …).
#[expect(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum Block<'a> {
    Paragraph(ArenaBox<'a, Paragraph<'a>>),
    Heading(ArenaBox<'a, Heading<'a>>),
    ThematicBreak(ThematicBreak),
    CodeBlock(ArenaBox<'a, CodeBlock<'a>>),
    HtmlBlock(ArenaBox<'a, HtmlBlock<'a>>),
    Blockquote(ArenaBox<'a, Blockquote<'a>>),
    List(ArenaBox<'a, List<'a>>),
    Definition(ArenaBox<'a, Definition<'a>>),
    Table(ArenaBox<'a, Table<'a>>),
    FootnoteDefinition(ArenaBox<'a, FootnoteDefinition<'a>>),
    MathBlock(ArenaBox<'a, MathBlock<'a>>),
    /// `{% … %}` / `{{ … }}` standing alone at flow level;
    /// kept verbatim so it can't be absorbed into neighboring blocks.
    Liquid(ArenaBox<'a, Liquid<'a>>),
    ContainerDirective(ArenaBox<'a, ContainerDirective<'a>>),
    MdxEsm(MdxEsm),
    MdxExpression(MdxExpression),
    MdxJsx(ArenaBox<'a, MdxJsxFlow<'a>>),
}

impl Block<'_> {
    pub fn span(&self) -> Span {
        match self {
            Block::Paragraph(n) => n.span,
            Block::Heading(n) => n.span,
            Block::ThematicBreak(n) => n.span,
            Block::CodeBlock(n) => n.span,
            Block::HtmlBlock(n) => n.span,
            Block::Blockquote(n) => n.span,
            Block::List(n) => n.span,
            Block::Definition(n) => n.span,
            Block::Table(n) => n.span,
            Block::FootnoteDefinition(n) => n.span,
            Block::MathBlock(n) => n.span,
            Block::Liquid(n) => n.span,
            Block::ContainerDirective(n) => n.span,
            Block::MdxEsm(n) => n.span,
            Block::MdxExpression(n) => n.span,
            Block::MdxJsx(n) => n.span,
        }
    }
}

/// Phrasing (inline) content.
// Same policy as [`Block`]: HtmlInline pairs with HtmlBlock.
#[expect(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum Inline<'a> {
    Text(Text),
    /// A soft line break (newline inside a paragraph).
    /// Rendered as `\n`; proseWrap policy decides its fate downstream.
    SoftBreak(SoftBreak),
    Emphasis(ArenaBox<'a, Emphasis<'a>>),
    Strong(ArenaBox<'a, Strong<'a>>),
    Strikethrough(ArenaBox<'a, Strikethrough<'a>>),
    CodeSpan(ArenaBox<'a, CodeSpan<'a>>),
    Link(ArenaBox<'a, Link<'a>>),
    Image(ArenaBox<'a, Image<'a>>),
    Autolink(Autolink),
    /// GFM bare URL / email, printed verbatim.
    AutolinkLiteral(AutolinkLiteral),
    HtmlInline(ArenaBox<'a, HtmlInline<'a>>),
    HardBreak(HardBreak),
    FootnoteReference(ArenaBox<'a, FootnoteReference>),
    /// `$ … $`, printed verbatim.
    MathSpan(ArenaBox<'a, MathSpan<'a>>),
    /// `[[target]]`, printed verbatim.
    WikiLink(WikiLink),
    /// `{% … %}` / `{{ … }}` inside text, kept verbatim.
    Liquid(ArenaBox<'a, Liquid<'a>>),
    MdxExpression(MdxExpression),
    MdxJsx(ArenaBox<'a, MdxJsxText<'a>>),
}

impl Inline<'_> {
    pub fn span(&self) -> Span {
        match self {
            Inline::Text(n) => n.span,
            Inline::SoftBreak(n) => n.span,
            Inline::Emphasis(n) => n.span,
            Inline::Strong(n) => n.span,
            Inline::Strikethrough(n) => n.span,
            Inline::CodeSpan(n) => n.span,
            Inline::Link(n) => n.span,
            Inline::Image(n) => n.span,
            Inline::Autolink(n) => n.span,
            Inline::AutolinkLiteral(n) => n.span,
            Inline::HtmlInline(n) => n.span,
            Inline::HardBreak(n) => n.span,
            Inline::FootnoteReference(n) => n.span,
            Inline::MathSpan(n) => n.span,
            Inline::WikiLink(n) => n.span,
            Inline::Liquid(n) => n.span,
            Inline::MdxExpression(n) => n.span,
            Inline::MdxJsx(n) => n.span,
        }
    }
}

#[derive(Debug)]
pub struct Paragraph<'a> {
    pub children: ArenaVec<'a, Inline<'a>>,
    /// Source lines that entered this paragraph as lazy continuations
    /// (unprefixed lines inside a blockquote/list).
    /// A printer must not merge or re-wrap across these: doing so changes meaning.
    pub lazy_lines: ArenaVec<'a, Span>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Heading<'a> {
    /// 1–6 for ATX, 1–2 for setext.
    pub level: u8,
    pub kind: HeadingKind<'a>,
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

#[derive(Debug)]
pub enum HeadingKind<'a> {
    Atx,
    /// The underline is printed verbatim (its length is preserved).
    Setext {
        underline: Span,
        /// Source lines that entered the underlying paragraph as lazy continuations,
        /// as on [`Paragraph::lazy_lines`].
        /// A printer keeping the setext form must not merge or re-wrap across these.
        lazy_lines: ArenaVec<'a, Span>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ThematicBreak {
    pub span: Span,
}

#[derive(Debug)]
pub struct CodeBlock<'a> {
    pub kind: CodeBlockKind,
    /// Logical content lines (container prefixes and fence/code indent stripped).
    /// The printer recomputes the minimum fence length from them.
    pub lines: ArenaVec<'a, Segment>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub enum CodeBlockKind {
    Fenced {
        /// `` ` `` or `~`.
        fence: u8,
        /// Raw info string, if any.
        /// Used both for language dispatch and verbatim output.
        info: Option<Span>,
    },
    /// Preserved as indented (never converted to fenced).
    Indented,
}

/// A raw HTML block. Contents are never parsed or formatted.
#[derive(Debug)]
pub struct HtmlBlock<'a> {
    /// CommonMark HTML block type 1–7.
    /// Type 1 (`<pre>`/`<script>`/`<style>`/ `<textarea>`) runs to its closing tag;
    /// the rest end at a blank line,
    /// which is how markdown content interleaves between unbalanced tags.
    pub kind: u8,
    /// Logical content lines, verbatim.
    pub lines: ArenaVec<'a, Segment>,
    /// Whether the block's final line ending belongs to its content
    /// (micromark: `html.value` ends with a line ending).
    /// Only types 1–5, in two cases.
    /// The block was still open at EOF (or at a directive's closing fence), outside any blockquote.
    /// The line that closed its container opened a new one (list item, blockquote, footnote definition).
    /// Types 6–7 end at their last content line, so never.
    pub trailing_newline: bool,
    pub span: Span,
}

#[derive(Debug)]
pub struct Blockquote<'a> {
    pub children: ArenaVec<'a, Block<'a>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct List<'a> {
    pub marker: ListMarker,
    /// Loose lists render children as paragraphs;
    /// blank-line placement around items decides this at parse time.
    pub tight: bool,
    pub children: ArenaVec<'a, ListItem<'a>>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub enum ListMarker {
    /// `-` / `*` / `+`.
    Bullet { marker: u8 },
    /// `.` / `)` after the number.
    Ordered { delimiter: u8 },
}

#[derive(Debug)]
pub struct ListItem<'a> {
    /// The marker itself: `-`, or the full `123.`.
    /// Ordered numbers stay raw
    /// (CommonMark caps them at 999,999,999; arithmetic on them overflows).
    pub marker: Span,
    /// Spaces between the marker and the content, for aligned-list judgment.
    pub padding: u8,
    /// GFM task list checkbox, when the item starts with one.
    /// The first paragraph begins after the checkbox and the whitespace before same-line content;
    /// when only whitespace follows on that line it stays in the paragraph,
    /// as the line's soft or hard break.
    pub checkbox: Option<TaskCheckbox>,
    /// A blank line separates two of this item's children (mdast's `listItem.spread`).
    /// Looseness is judged per item downstream (Prettier: this, or a blank line before the next item),
    /// while [`List::tight`] is the whole list's cmark verdict.
    pub spread: bool,
    pub children: ArenaVec<'a, Block<'a>>,
    pub span: Span,
}

/// `[ ]`, `[x]` or `[X]` (the raw span keeps the case).
#[derive(Clone, Copy, Debug)]
pub struct TaskCheckbox {
    pub span: Span,
    pub checked: bool,
}

/// A link reference definition: `[label]: dest "title"`.
///
/// Label and title may span lines,
/// and a multi-line span would include the container prefixes between them,
/// so both are logical-line pieces (see [`Segment::join`]).
/// Normalize the label from the joined pieces;
/// a title's continuation pieces keep their leading whitespace raw (micromark strips all of it).
#[derive(Debug)]
pub struct Definition<'a> {
    /// Between the brackets.
    pub label: ArenaVec<'a, Segment>,
    pub destination: Destination,
    /// Including the quotes.
    pub title: Option<ArenaVec<'a, Segment>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Table<'a> {
    /// One entry per column of the delimiter row.
    /// Alignments beyond the header row's cell count are dropped (GFM).
    pub align: ArenaVec<'a, TableAlign>,
    /// The first row is the header.
    pub children: ArenaVec<'a, TableRow<'a>>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlign {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug)]
pub struct TableRow<'a> {
    pub children: ArenaVec<'a, TableCell<'a>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct TableCell<'a> {
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct FootnoteDefinition<'a> {
    pub label: Span,
    pub children: ArenaVec<'a, Block<'a>>,
    pub span: Span,
}

/// `$$ … $$` display math, content is kept raw.
/// As with code fences, the fence length is not stored
/// (printers normalize to `$$`; the `$` run at `span.start` gives it if ever needed).
#[derive(Debug)]
pub struct MathBlock<'a> {
    /// Raw text after the opening fence (`$$meta`), leading whitespace stripped.
    /// Never contains `$`.
    pub meta: Option<Span>,
    /// Logical content lines (container prefixes and the opening fence's indent stripped),
    /// fence lines excluded.
    pub lines: ArenaVec<'a, Segment>,
    pub span: Span,
}

/// A container directive:
///
/// ```markdown
/// :::note[label]{.class}
/// children
/// :::
/// ```
#[derive(Debug)]
pub struct ContainerDirective<'a> {
    /// The whole opening fence line, kept verbatim.
    /// Dialects disagree on its grammar (remark-directive vs markdown-it-container),
    /// so it is never normalized.
    pub opening: Span,
    /// The closing fence line, if the directive was explicitly closed.
    /// Nesting uses longer fences; as with code fences, the length is not stored
    /// (the `:` run at `opening.start` gives it).
    pub closing: Option<Span>,
    pub children: ArenaVec<'a, Block<'a>>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub struct Text {
    pub span: Span,
    /// Every byte is ASCII.
    /// Fast path for escape/wrap analysis.
    pub ascii_only: bool,
    /// Contains CJK characters.
    /// Classification (CJ vs K vs punctuation) is formatter policy and happens downstream.
    pub contains_cjk: bool,
}

#[derive(Debug)]
pub struct Emphasis<'a> {
    /// `*` or `_`. Normalization is context-dependent,
    /// so the source marker is a required input, not a derivable one.
    pub marker: u8,
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Strong<'a> {
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

/// GFM `~~x~~`.
#[derive(Debug)]
pub struct Strikethrough<'a> {
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct CodeSpan<'a> {
    /// Content between the backtick runs, one segment per line
    /// (a multi-line code span crosses container prefixes, so a single span can't cover it;
    /// a partially consumed tab leaves virtual spaces in `padding`).
    /// The printer recomputes the minimum backtick run length from these,
    /// and escapes `|` only inside table cells.
    pub pieces: ArenaVec<'a, Segment>,
    pub span: Span,
}

/// `[text](dest "title")` or `[text][label]` / `[text][]` / `[text]`.
#[derive(Debug)]
pub struct Link<'a> {
    pub kind: LinkKind<'a>,
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

/// `![alt](dest "title")` and the reference forms.
#[derive(Debug)]
pub struct Image<'a> {
    pub kind: LinkKind<'a>,
    /// The alt text.
    /// Nested link/image syntax is preserved here, not cooked.
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}

/// Titles and labels are logical-line pieces for the same reason as on [`Definition`].
#[derive(Debug)]
pub enum LinkKind<'a> {
    Inline {
        destination: Destination,
        /// Including the quotes.
        title: Option<ArenaVec<'a, Segment>>,
    },
    Reference {
        kind: ReferenceKind,
        /// The text the reference resolves through:
        /// the second bracket's interior for `Full`, the link text for `Collapsed`/`Shortcut`.
        label: ArenaVec<'a, Segment>,
    },
}

/// Reference-link form.
/// Only `Full` is reformatted; the others are printed verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    /// `[text][label]`
    Full,
    /// `[text][]`
    Collapsed,
    /// `[text]`
    Shortcut,
}

/// A link/image destination, excluding the title.
#[derive(Clone, Copy, Debug)]
pub struct Destination {
    /// Covers the `<`/`>` when angle-bracketed.
    pub span: Span,
    /// `<dest>` form. Whether brackets are needed on output is a lexical judgment
    /// (spaces, `(`, `<`, `\` …), computed from the raw span.
    pub angle_bracketed: bool,
}

/// `<https://example.com>` / `<user@example.com>`.
#[derive(Clone, Copy, Debug)]
pub struct Autolink {
    pub span: Span,
    /// Email autolinks render with an implied `mailto:` that must not leak into output.
    pub email: bool,
}

/// GFM autolink literal (`www.example.com`, bare `https://…` / email).
#[derive(Clone, Copy, Debug)]
pub struct AutolinkLiteral {
    pub span: Span,
}

/// A raw inline HTML tag or comment.
/// The tag is a verbatim token; text around it is markdown.
#[derive(Debug)]
pub struct HtmlInline<'a> {
    /// Verbatim content, one segment per line.
    /// Continuation pieces keep their leading whitespace raw
    /// (micromark strips up to 3 columns).
    pub pieces: ArenaVec<'a, Segment>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub struct SoftBreak {
    /// Zero-width, at the first content byte of the following line
    /// (past the line ending and any container prefix).
    /// The line ending itself is not covered: it lies between the two sibling spans.
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub struct HardBreak {
    pub kind: HardBreakKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardBreakKind {
    /// Two-plus trailing spaces before the newline.
    Spaces,
    /// `\` before the newline.
    Backslash,
}

/// GFM `[^label]`.
#[derive(Clone, Copy, Debug)]
pub struct FootnoteReference {
    pub label: Span,
    pub span: Span,
}

/// `$ … $` inline math.
/// The whole construct, `$` sequences included, is kept raw;
/// content is never trimmed or decoded.
#[derive(Debug)]
pub struct MathSpan<'a> {
    /// Verbatim construct, one segment per line
    /// (a multi-line span crosses container prefixes, so a single span can't cover it).
    pub pieces: ArenaVec<'a, Segment>,
    pub span: Span,
}

/// `[[target]]`, alias-less.
/// Always single-line; the target is `span + 2 .. span - 2`, kept raw.
#[derive(Clone, Copy, Debug)]
pub struct WikiLink {
    pub span: Span,
}

/// A `{% … %}` / `{{ … }}` template tag, block-level or inline.
/// Never interpreted: the whole construct, delimiters included, is kept verbatim.
#[derive(Debug)]
pub struct Liquid<'a> {
    /// Verbatim construct, one segment per line (multi-line tags cross container prefixes).
    pub pieces: ArenaVec<'a, Segment>,
    pub span: Span,
}

/// An MDX `import`/`export` statement.
/// The parser only establishes the boundary (via the host's JS parser callback);
/// the content stays raw for embedded formatting.
#[derive(Clone, Copy, Debug)]
pub struct MdxEsm {
    pub span: Span,
}

/// An MDX `{expr}`.
/// Boundary only; the expression stays raw.
#[derive(Clone, Copy, Debug)]
pub struct MdxExpression {
    pub span: Span,
}

/// An MDX JSX element at flow level.
/// Tags stay raw for embedded formatting; children are markdown blocks.
#[derive(Debug)]
pub struct MdxJsxFlow<'a> {
    /// The opening (or self-closing) tag, raw.
    pub opening: Span,
    /// The closing tag, raw. `None` when self-closing.
    pub closing: Option<Span>,
    pub children: ArenaVec<'a, Block<'a>>,
    pub span: Span,
}

/// An MDX JSX element in text.
/// Children are inline markdown.
#[derive(Debug)]
pub struct MdxJsxText<'a> {
    pub opening: Span,
    pub closing: Option<Span>,
    pub children: ArenaVec<'a, Inline<'a>>,
    pub span: Span,
}
