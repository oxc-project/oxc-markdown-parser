//! Bracket handling: links and images (cmark's bracket stack).
//!
//! `[` / `![` only records a bracket;
//! `]` tries, in order, the inline suffix `(dest "title")`, a full reference label,
//! then collapsed / shortcut resolution against the [`super::RefMap`].
//! On success the inner delimiters are processed
//! and the nodes since the opener become the link's children;
//! a successful link deactivates every earlier `[` (links don't nest in links; images do).

use std::ops::Range;

use crate::ast::ReferenceKind;
use crate::syntax::{label, link_target, skip_ws};

use super::{PLinkKind, PN, Tokenizer};

#[derive(Clone, Copy)]
pub struct Bracket {
    pub node: usize,
    pub delim_len: usize,
    pub image: bool,
    pub active: bool,
    /// Joined start of the `[` / `![` text.
    pub text_start: usize,
}

impl Bracket {
    /// Joined position just past the `[`.
    fn after_open(self) -> usize {
        self.text_start + 1 + usize::from(self.image)
    }
}

impl Tokenizer<'_, '_> {
    pub(crate) fn close_bracket(&mut self) {
        let p = self.pos;
        self.flush_text(p);
        let Some(b) = self.brackets.last().copied() else {
            self.literal_close(p);
            return;
        };
        if !b.active {
            return self.fail_bracket(p);
        }

        // Inline: `](dest "title")`
        if self.t.as_bytes().get(p + 1) == Some(&b'(')
            && let Some((dest, angle_bracketed, title, end)) = scan_inline_suffix(self.t, p + 1)
        {
            self.make_link(b, PLinkKind::Inline { dest, angle_bracketed, title }, end);
            return;
        }

        // `][…`: only full (`][label]`) or collapsed (`][]`) resolution.
        // micromark never falls back to a shortcut when a `[` follows,
        // whether the second label is undefined or fails to parse at all.
        if self.t.as_bytes().get(p + 1) == Some(&b'[') {
            // Collapsed is literally `][]` (micromark's own construct);
            // `[a][ ]` is a full reference whose label fails to parse.
            if self.t.as_bytes().get(p + 2) == Some(&b']') {
                let inner = b.after_open()..p;
                if self.lookup(&inner) {
                    self.make_link(
                        b,
                        PLinkKind::Reference { kind: ReferenceKind::Collapsed, label: inner },
                        p + 3,
                    );
                    return;
                }
                return self.fail_bracket(p);
            }
            let Some(label_close) = link_target::label_end(self.t, p + 1) else {
                return self.fail_bracket(p);
            };
            let label = p + 2..label_close - 1;
            if self.lookup(&label) {
                self.make_link(
                    b,
                    PLinkKind::Reference { kind: ReferenceKind::Full, label },
                    label_close,
                );
                return;
            }
            // A present-but-undefined label fails the bracket;
            // the label re-scans on its own.
            return self.fail_bracket(p);
        }

        // Shortcut: `]`.
        // No emptiness guard: a whitespace-only label normalizes to `""`,
        // which no definition can ever register.
        let inner = b.after_open()..p;
        if self.lookup(&inner) {
            self.make_link(
                b,
                PLinkKind::Reference { kind: ReferenceKind::Shortcut, label: inner },
                p + 1,
            );
            return;
        }
        // GFM: `![^label]` that failed to become an image
        // resolves to a literal `!` plus a footnote call (micromark's "potential call").
        if b.image
            && let Some((label, end)) = self.footnote_label(b.text_start + 1)
            && end == p + 1
        {
            self.nodes.truncate(b.node + 1);
            self.delims.truncate(b.delim_len);
            self.nodes[b.node] = PN::Text(b.text_start..b.text_start + 1);
            self.push_footnote_call(label, b.text_start + 1, end);
            self.brackets.pop();
            return;
        }
        self.fail_bracket(p);
    }

    /// Definition-map probe;
    /// skips normalization entirely when the document has no definitions.
    fn lookup(&self, label: &Range<usize>) -> bool {
        !self.refs.is_empty() && self.refs.contains(&label::normalize(&self.t[label.clone()]))
    }

    /// The shared failure tail: drop the bracket, keep the `]` as text.
    fn fail_bracket(&mut self, p: usize) {
        self.brackets.pop();
        self.literal_close(p);
    }

    fn literal_close(&mut self, p: usize) {
        self.nodes.push(PN::Text(p..p + 1));
        self.pos = p + 1;
        self.text_start = self.pos;
    }

    /// Drains `nodes[from..]` (dropping delimiter text nodes emptied by emphasis processing)
    /// as a new node's children.
    pub(crate) fn take_children(&mut self, from: usize) -> Vec<PN> {
        self.nodes.drain(from..).filter(|n| !matches!(n, PN::Text(r) if r.is_empty())).collect()
    }

    fn make_link(&mut self, b: Bracket, kind: PLinkKind, end: usize) {
        self.process_emphasis(b.delim_len);
        let inner = self.take_children(b.node + 1);
        self.delims.truncate(b.delim_len);
        self.nodes[b.node] =
            PN::Link { image: b.image, kind, children: inner, r: b.text_start..end };
        self.brackets.pop();
        if !b.image {
            for bracket in &mut self.brackets {
                if !bracket.image {
                    bracket.active = false;
                }
            }
        }
        self.pos = end;
        self.text_start = end;
    }
}

/// Scans `(ws dest ws "title" ws)` starting at the `(`.
/// Returns (destination range (empty when absent), angle-bracketed flag,
/// title range including its quotes, position past `)`).
type InlineSuffix = (Range<usize>, bool, Option<Range<usize>>, usize);

fn scan_inline_suffix(t: &str, open: usize) -> Option<InlineSuffix> {
    let bytes = t.as_bytes();
    debug_assert_eq!(bytes[open], b'(');
    let mut p = skip_ws(bytes, open + 1, usize::MAX);

    // An empty bare destination keeps its (zero-width) position inside the parens
    let (dest, angle_bracketed) = link_target::destination(t, p)?;
    p = dest.end;

    // Optional title, requiring whitespace after the destination
    let mut title = None;
    let after_dest = p;
    p = skip_ws(bytes, p, usize::MAX);
    if p > after_dest
        && let Some(title_range) = link_target::title(t, p)
    {
        p = skip_ws(bytes, title_range.end, usize::MAX);
        title = Some(title_range);
    }

    (bytes.get(p) == Some(&b')')).then(|| (dest, angle_bracketed, title, p + 1))
}
