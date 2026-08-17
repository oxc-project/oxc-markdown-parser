//! Inline phase.
//!
//! micromark-shaped single pass: special characters are scanned left to right;
//! code spans, autolinks and raw HTML resolve immediately (they outrank everything),
//! `*`/`_` runs and brackets are only recorded,
//! then `process_emphasis` / bracket close restructure the flat node list.
//!
//! Positions are ranges into the joined logical text ([`input::Input`]);
//! every emitted span goes through its exact per-byte map back to source offsets.

mod autolink;
mod emphasis;
mod gfm_autolink;
mod html;
pub mod input;
mod link;
mod refmap;

pub use gfm_autolink::{LiteralKind, literal_kind};
pub use refmap::RefMap;

use std::ops::Range;

use oxc_allocator::{Allocator, ArenaBox, ArenaVec};

use crate::ast::*;
use crate::block::IrSeg;
use crate::options::Constructs;
use crate::pos::Span;
use crate::syntax;
use emphasis::Delim;
use input::Input;
use link::Bracket;

/// Proto-node over joined-text ranges; converted to the arena AST at the end.
pub enum PN {
    Text(Range<usize>),
    SoftBreak(usize),
    HardBreak { kind: HardBreakKind, r: Range<usize> },
    Code { r: Range<usize>, content: Range<usize> },
    Html { r: Range<usize> },
    Autolink { r: Range<usize>, email: bool },
    AutolinkLiteral(Range<usize>),
    FootnoteRef { label: Range<usize>, r: Range<usize> },
    MathSpan(Range<usize>),
    WikiLink(Range<usize>),
    Liquid(Range<usize>),
    Emph { marker: u8, strong: bool, children: Vec<PN>, r: Range<usize> },
    Link { image: bool, kind: PLinkKind, children: Vec<PN>, r: Range<usize> },
}

pub enum PLinkKind {
    Inline { dest: Range<usize>, angle_bracketed: bool, title: Option<Range<usize>> },
    Reference { kind: ReferenceKind, label: Range<usize> },
}

pub fn inlines<'a>(
    allocator: &'a Allocator,
    source: &str,
    segments: &[IrSeg],
    refs: &RefMap,
    constructs: &Constructs,
) -> ArenaVec<'a, Inline<'a>> {
    if segments.is_empty() {
        return ArenaVec::new_in(&allocator);
    }
    let input = Input::new(source, segments);
    let mut tk = Tokenizer {
        t: &input.text,
        refs,
        constructs,
        pos: 0,
        text_start: 0,
        nodes: Vec::new(),
        delims: Vec::new(),
        brackets: Vec::new(),
        first_delim_tilde: None,
        liquid_exhausted: [false; 2],
    };
    tk.run();
    let mut out = ArenaVec::new_in(&allocator);
    for pn in tk.nodes {
        push_ast(allocator, &input, pn, &mut out);
    }
    out
}

pub struct Tokenizer<'i, 'r> {
    t: &'i str,
    refs: &'r RefMap,
    constructs: &'r Constructs,
    pos: usize,
    text_start: usize,
    nodes: Vec<PN>,
    delims: Vec<Delim>,
    brackets: Vec<Bracket>,
    /// Whether the first family whose construct succeeded in this run was strikethrough
    /// (any `*`/`_` run, any valid-length `~` run, flanking irrelevant);
    /// decides which family resolves first at the end of the run.
    first_delim_tilde: Option<bool>,
    /// Per closer kind (`%}` / `}}`):
    /// a liquid scan already reached the end of input without finding this closer,
    /// so every later attempt of the same kind fails too, skip the rescans (quadratic otherwise).
    liquid_exhausted: [bool; 2],
}

impl Tokenizer<'_, '_> {
    fn run(&mut self) {
        let bytes = self.t.as_bytes();
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'\n' => self.line_ending(),
                b'\\' => match bytes.get(self.pos + 1) {
                    Some(b'\n') => {
                        self.flush_text(self.pos);
                        self.nodes.push(PN::HardBreak {
                            kind: HardBreakKind::Backslash,
                            r: self.pos..self.pos + 1,
                        });
                        self.pos += 2;
                        self.skip_line_ws();
                    }
                    _ if syntax::escapes_next(bytes, self.pos) => self.pos += 2,
                    _ => self.pos += 1,
                },
                b'`' => self.backticks(),
                b'$' if self.constructs.math_text => self.math_span(),
                b'{' if self.try_liquid() => {}
                b'<' => {
                    let handled = self.angle();
                    if !handled {
                        self.pos += 1;
                    }
                }
                // Extensions run before core constructs in micromark,
                // so a literal email may start at `_` (atext);
                // attention only gets the run when the email attempt fails.
                b'_' if self.try_autolink_literal() => {}
                b'*' | b'_' => self.delimiter_run(),
                b'~' if self.constructs.gfm_strikethrough => self.delimiter_run(),
                // GFM autolink literals start on alphanumerics and `+-.`
                // (`_` belongs to attention, which wins there);
                // the previous-character gates inside `scan` make most attempts O(1).
                // Suppressed inside an open bracket label.
                c if (c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.')) => {
                    if !self.try_autolink_literal() {
                        self.pos += 1;
                    }
                }
                // Footnote calls are extension constructs and outrank the core label start;
                // `![^…]` leaves the `!` as text.
                b'[' if self.try_footnote_call() => {}
                b'[' if self.try_wiki_link() => {}
                b'[' => self.open_bracket(false),
                b'!' if bytes.get(self.pos + 1) == Some(&b'[') => self.open_bracket(true),
                b']' => self.close_bracket(),
                _ => self.pos += 1,
            }
        }
        // Final line: trailing whitespace is stripped, never a break
        self.flush_text(self.t.trim_end_matches([' ', '\t']).len());
        self.process_emphasis_final();
    }

    /// GFM autolink literal attempt at the current byte.
    /// The previous- character gates inside the scanner keep failed attempts O(1);
    /// literals never start inside an open bracket label.
    fn try_autolink_literal(&mut self) -> bool {
        if !self.constructs.gfm_autolink_literal || !self.brackets.is_empty() {
            return false;
        }
        let prev = self.t[..self.pos].chars().next_back();
        let Some(end) = gfm_autolink::scan(self.t, self.pos, prev) else { return false };
        self.flush_text(self.pos);
        self.nodes.push(PN::AutolinkLiteral(self.pos..end));
        self.pos = end;
        self.text_start = end;
        true
    }

    /// A defined footnote label `[^…]` starting at `at`;
    /// returns (label range, position past the `]`).
    /// Skips all work when the document has no footnote definitions.
    pub(crate) fn footnote_label(&self, at: usize) -> Option<(Range<usize>, usize)> {
        if !self.constructs.gfm_footnote || self.refs.footnotes_is_empty() {
            return None;
        }
        let (label, end) = syntax::link_target::footnote_label(self.t, at)?;
        self.refs
            .contains_footnote(&syntax::label::normalize(&self.t[label.clone()]))
            .then_some((label, end))
    }

    /// GFM footnote call `[^label]` at the current `[`. Only defined labels match;
    /// anything else falls back to the ordinary bracket machinery.
    fn try_footnote_call(&mut self) -> bool {
        let Some((label, end)) = self.footnote_label(self.pos) else { return false };
        self.flush_text(self.pos);
        self.push_footnote_call(label, self.pos, end);
        true
    }

    /// Shared tail of the two footnote-call paths (plain `[^…]` and the failed-image fallback).
    pub(crate) fn push_footnote_call(&mut self, label: Range<usize>, at: usize, end: usize) {
        self.nodes.push(PN::FootnoteRef { label, r: at..end });
        self.pos = end;
        self.text_start = end;
    }

    fn open_bracket(&mut self, image: bool) {
        self.flush_text(self.pos);
        let width = 1 + usize::from(image);
        self.nodes.push(PN::Text(self.pos..self.pos + width));
        self.brackets.push(Bracket {
            node: self.nodes.len() - 1,
            delim_len: self.delims.len(),
            image,
            active: true,
            text_start: self.pos,
        });
        self.pos += width;
        self.text_start = self.pos;
    }

    /// Consumes leading whitespace of a (logical) line: stripped from text,
    /// but still present in the raw buffer for code spans.
    fn skip_line_ws(&mut self) {
        while matches!(self.t.as_bytes().get(self.pos), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
        self.text_start = self.pos;
    }

    pub(crate) fn flush_text(&mut self, upto: usize) {
        if self.text_start < upto {
            self.nodes.push(PN::Text(self.text_start..upto));
        }
        self.text_start = upto.max(self.text_start);
    }

    fn line_ending(&mut self) {
        let p = self.pos;
        let run = &self.t[self.text_start..p];
        let trimmed = run.trim_end_matches([' ', '\t']);
        let trail = run.len() - trimmed.len();
        self.flush_text(p - trail);
        // A hard break needs a trailing run of two-plus spaces;
        // a tab in the run demotes it to stripped whitespace (micromark behavior).
        if trail >= 2 && run.as_bytes()[run.len() - trail..].iter().all(|&b| b == b' ') {
            self.nodes.push(PN::HardBreak { kind: HardBreakKind::Spaces, r: p - trail..p });
        } else {
            self.nodes.push(PN::SoftBreak(p));
        }
        self.pos = p + 1;
        self.skip_line_ws();
    }

    /// `$ … $` / `$$ … $$` math (micromark-extension-math, single dollars allowed):
    /// the opening run is maximal, the closing run must match it exactly,
    /// content may span line endings but not the end of input.
    /// Resolves immediately, like code spans, failed attempt consumes the whole run
    /// (mid-run `$`s can never open, matching micromark's previous-character gate).
    fn math_span(&mut self) {
        let start = self.pos;
        let n = run_len(self.t.as_bytes(), start, b'$');
        let Some(close) = matching_run(self.t.as_bytes(), start + n, b'$', n) else {
            self.pos = start + n;
            return;
        };
        self.flush_text(start);
        self.nodes.push(PN::MathSpan(start..close + n));
        self.pos = close + n;
        self.text_start = self.pos;
    }

    /// `[[target]]` (alias-less braindb grammar): no line endings inside,
    /// the target needs at least one non-whitespace byte,
    /// and the first `]` must start the closing `]]`, anything else fails the construct.
    fn try_wiki_link(&mut self) -> bool {
        if !self.constructs.wiki_link {
            return false;
        }
        let bytes = self.t.as_bytes();
        let start = self.pos;
        if bytes.get(start + 1) != Some(&b'[') {
            return false;
        }
        let mut i = start + 2;
        let mut data = false;
        loop {
            match bytes.get(i) {
                None | Some(b'\n') => return false,
                Some(b']') => {
                    if !(data && bytes.get(i + 1) == Some(&b']')) {
                        return false;
                    }
                    self.flush_text(start);
                    self.nodes.push(PN::WikiLink(start..i + 2));
                    self.pos = i + 2;
                    self.text_start = self.pos;
                    return true;
                }
                Some(&b) => {
                    data |= !matches!(b, b' ' | b'\t');
                    i += 1;
                }
            }
        }
    }

    /// `{% … %}` / `{{ … }}` in text:
    /// the closer is the first occurrence of the matching two-byte sequence (no nesting),
    /// may span line endings, fails at the end of input.
    fn try_liquid(&mut self) -> bool {
        if !self.constructs.liquid {
            return false;
        }
        let start = self.pos;
        let Some(closer) = syntax::liquid::open(&self.t[start..]) else { return false };
        let exhausted = &mut self.liquid_exhausted[usize::from(closer == b'}')];
        if *exhausted {
            return false;
        }
        let Some(after) = syntax::liquid::close(&self.t[start + 2..], closer) else {
            // No closer of this kind exists from here on; every later attempt fails too
            *exhausted = true;
            return false;
        };
        let end = start + 2 + after;
        self.flush_text(start);
        self.nodes.push(PN::Liquid(start..end));
        self.pos = end;
        self.text_start = self.pos;
        true
    }

    fn backticks(&mut self) {
        let start = self.pos;
        let n = run_len(self.t.as_bytes(), start, b'`');
        let Some(close) = matching_run(self.t.as_bytes(), start + n, b'`', n) else {
            self.pos = start + n;
            return;
        };
        let mut content = start + n..close;
        let c = &self.t[content.clone()];
        if c.len() >= 2
            && c.starts_with([' ', '\n'])
            && c.ends_with([' ', '\n'])
            && !c.bytes().all(|b| b == b' ' || b == b'\n')
        {
            content = content.start + 1..content.end - 1;
        }
        self.flush_text(start);
        self.nodes.push(PN::Code { r: start..close + n, content });
        self.pos = close + n;
        self.text_start = self.pos;
    }

    /// `<`: autolink, then raw HTML.
    /// Also the MDX JSX toggle point (the `mdx_jsx_text` construct hooks in here).
    fn angle(&mut self) -> bool {
        if self.constructs.autolink
            && let Some((end, email)) = autolink::scan(self.t, self.pos)
        {
            self.flush_text(self.pos);
            self.nodes.push(PN::Autolink { r: self.pos..end, email });
            self.pos = end;
            self.text_start = end;
            return true;
        }
        if self.constructs.html_text
            && let Some(end) = html::scan(self.t, self.pos)
        {
            self.flush_text(self.pos);
            self.nodes.push(PN::Html { r: self.pos..end });
            self.pos = end;
            self.text_start = end;
            return true;
        }
        false
    }
}

/// Length of the run of `marker` bytes at `at`.
fn run_len(bytes: &[u8], at: usize, marker: u8) -> usize {
    bytes[at..].iter().take_while(|&&b| b == marker).count()
}

/// Start of the first run of exactly `n` `marker` bytes at or after `from`
/// (code spans and math share this closing-run rule).
fn matching_run(bytes: &[u8], from: usize, marker: u8, n: usize) -> Option<usize> {
    let mut j = from;
    loop {
        let at = j + bytes[j..].iter().position(|&b| b == marker)?;
        let m = run_len(bytes, at, marker);
        if m == n {
            return Some(at);
        }
        j = at + m;
    }
}

/// Siblings are emitted in source order
fn push<'a>(out: &mut ArenaVec<'a, Inline<'a>>, node: Inline<'a>) {
    debug_assert!(out.last().is_none_or(|prev| prev.span().end <= node.span().start));
    out.push(node);
}

fn push_ast<'a>(
    allocator: &'a Allocator,
    input: &Input,
    pn: PN,
    out: &mut ArenaVec<'a, Inline<'a>>,
) {
    match pn {
        PN::Text(r) => {
            if r.is_empty() {
                return;
            }
            let span = input.span(r.clone());
            let text = &input.text[r];
            let ascii_only = text.is_ascii();
            let contains_cjk = !ascii_only && text.chars().any(is_cjk);
            // Bracket and delimiter runs arrive as their own text pieces;
            // contiguous pieces merge into one node (as mdast does).
            if let Some(Inline::Text(prev)) = out.last_mut()
                && prev.span.end == span.start
            {
                prev.span.end = span.end;
                prev.ascii_only &= ascii_only;
                prev.contains_cjk |= contains_cjk;
                return;
            }
            push(out, Inline::Text(Text { span, ascii_only, contains_cjk }));
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
            let pieces = ArenaVec::from_iter_in(input.pieces(r.clone()), &allocator);
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
            let pieces = ArenaVec::from_iter_in(input.pieces(r.clone()), &allocator);
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
            let pieces = ArenaVec::from_iter_in(input.pieces(r.clone()), &allocator);
            push(
                out,
                Inline::Liquid(ArenaBox::new_in(
                    Liquid { pieces, span: input.span(r) },
                    &allocator,
                )),
            );
        }
        PN::Emph { marker, strong, children, r } => {
            let mut inner = ArenaVec::new_in(&allocator);
            for child in children {
                push_ast(allocator, input, child, &mut inner);
            }
            let span = input.span(r);
            if marker == b'~' {
                push(
                    out,
                    Inline::Strikethrough(ArenaBox::new_in(
                        Strikethrough { children: inner, span },
                        &allocator,
                    )),
                );
            } else if strong {
                push(
                    out,
                    Inline::Strong(ArenaBox::new_in(Strong { children: inner, span }, &allocator)),
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
        PN::Link { image, kind, children, r } => {
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
                        Image { kind, children: inner, span },
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
