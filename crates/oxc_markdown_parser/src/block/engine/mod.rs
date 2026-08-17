//! The container-stack line loop.
//!
//! Follows cmark's model: per line,
//! - (1) match the open container stack
//! - (2) continue verbatim leaves (fenced code, HTML types 1–5)
//! - (3) handle blank lines
//! - (4) decide lazy continuation vs closing unmatched blocks
//! - (4b) continue the non-verbatim leaf (HTML types 6–7, table rows, indented code)
//! - (5) loop over new block starts (`open.rs`)
//!
//! Each step is one method of [`Engine`]; `line` only sequences them.
//! Extension constructs with their own continuation rules live beside it:
//! `table.rs` (header stealing, row continuation) and `liquid.rs` (multi-line validation).

mod liquid;
mod open;
mod state;
mod table;

pub use state::BlankLines;

use crate::inline::RefMap;
use crate::options::Constructs;
use crate::pos::{Segment, Span};
use state::{Continuation, Leaf, OpenContainer, StackMatch};

use super::cursor::Cursor;
use super::ir::{Ir, IrItem, IrSeg};
use super::probe::{Start, probe};
use super::{refdef, scan};

pub struct Engine<'s> {
    source: &'s str,
    constructs: Constructs,
    stack: Vec<OpenContainer>,
    root: Vec<Ir>,
    leaf: Option<Leaf>,
    blanks: BlankLines,
    /// Normalized labels of the definitions seen so far,
    /// filled as paragraphs close (no separate collection pass).
    refs: RefMap,
    /// Lines ending at or before this offset were already consumed
    /// by a validated multi-line liquid tag (attached at its open site).
    skip_to: u32,
    /// Per closer kind (`%}` / `}}`):
    /// no flow liquid attempt whose search starts at or before this offset can succeed.
    /// A scan reached EOF without the closer (`u32::MAX`),
    /// or the first closer from there was rejected for trailing content
    /// (a line-local fact; container prefixes hold no closer bytes,
    /// so any earlier attempt finds the same closer first and fails the same way).
    /// Skips the rescans (they are quadratic otherwise).
    /// Stack-dependent rejections (container mismatch, interrupt) are not cacheable
    /// by position and stay unrecorded.
    liquid_dead: [u32; 2],
}

impl<'s> Engine<'s> {
    pub fn new(source: &'s str, constructs: Constructs) -> Self {
        Self {
            source,
            constructs,
            stack: Vec::new(),
            root: Vec::new(),
            leaf: None,
            blanks: BlankLines::default(),
            refs: RefMap::default(),
            skip_to: 0,
            liquid_dead: [0; 2],
        }
    }

    pub fn run(mut self) -> (Vec<Ir>, BlankLines, RefMap) {
        let mut start = 0u32;
        while (start as usize) < self.source.len() {
            let content_end = line_content_end(self.source, start);
            self.line(start, content_end);
            match next_line_start(self.source, content_end) {
                Some(next) => start = next,
                None => break,
            }
        }
        self.mark_html_eof(self.source.ends_with(['\n', '\r']));
        self.close_leaf();
        self.close_from(0);
        (self.root, self.blanks, self.refs)
    }

    fn line(&mut self, line_start: u32, content_end: u32) {
        // Lines already consumed by a validated multi-line liquid tag
        // (attached at its open site) are pure replay.
        if line_start < self.skip_to {
            return;
        }
        let mut cur = self.line_cursor(line_start, content_end);

        // 1. Match the open container stack.
        let matched = match self.match_stack(&mut cur) {
            StackMatch::DirectiveClose { index, fence } => {
                return self.close_directive(index, fence, content_end);
            }
            StackMatch::Prefixes(matched) => matched,
        };
        let all_matched = matched == self.stack.len();

        // 2. Verbatim leaf continuations consume the whole line.
        if all_matched && self.continue_verbatim_leaf(cur, content_end) {
            return;
        }

        // 3. Blank lines.
        if scan::is_blank(cur.tail()) {
            return self.blank_line(cur, matched, all_matched, content_end);
        }

        // 4. Lazy continuation vs closing unmatched containers.
        if !all_matched && self.lazy_continuation(cur, matched, content_end) {
            return;
        }

        // 4b. Non-verbatim leaf continuations.
        if matches!(self.continue_leaf(&mut cur, content_end), Continuation::Consumed) {
            return;
        }

        // 5. New block starts.
        self.open_blocks(cur, content_end);
    }

    /// A cursor over the physical line `start..end` (content only, no terminator).
    fn line_cursor(&self, start: u32, end: u32) -> Cursor<'s> {
        Cursor::new(&self.source[start as usize..end as usize], start)
    }

    /// Step 1's directive-close outcome: the line is an open directive's closing fence.
    /// It consumes the whole line, force-closing everything deeper first.
    fn close_directive(&mut self, index: usize, fence: Span, content_end: u32) {
        // The directive's content is a sub-document to micromark: it ends like EOF
        // (the fence sits on its own line, so the line ending before it always exists).
        self.mark_html_eof(true);
        self.close_from(index + 1);
        let Some(OpenContainer::Directive { closing, end, .. }) = self.stack.last_mut() else {
            unreachable!("the close signal always points at a directive");
        };
        *closing = Some(fence);
        *end = content_end;
        self.close_from(index);
    }

    /// Step 2: fenced code / math and HTML types 1–5 take every line until their end condition.
    /// Returns whether the line was consumed.
    fn continue_verbatim_leaf(&mut self, mut cur: Cursor<'s>, content_end: u32) -> bool {
        match &mut self.leaf {
            Some(Leaf::FencedCode { fence, len, indent, lines, end, .. }) => {
                let save = cur;
                cur.skip_indent_up_to(3);
                if cur.partial == 0 && scan::fence_close(cur.tail(), *fence, *len) {
                    *end = content_end;
                    self.close_leaf();
                } else {
                    cur = save;
                    let strip = *indent;
                    cur.skip_indent_up_to(strip);
                    lines.push(cur.segment(content_end));
                    *end = content_end;
                }
                true
            }
            Some(Leaf::Html { kind: kind @ 1..=5, lines, end, .. }) => {
                let kind = *kind;
                lines.push(cur.segment(content_end));
                *end = content_end;
                if scan::html_block_end(cur.tail(), kind) {
                    self.close_leaf();
                }
                true
            }
            _ => false,
        }
    }

    /// Step 3: a blank line closes non-verbatim leaves and unmatched containers,
    /// and is recorded in the blank-line tables.
    fn blank_line(&mut self, cur: Cursor<'s>, matched: usize, all_matched: bool, content_end: u32) {
        let deepest_is_quote =
            all_matched && matches!(self.stack.last(), Some(OpenContainer::Quote { .. }));
        match &mut self.leaf {
            Some(Leaf::Paragraph { .. } | Leaf::Html { .. } | Leaf::Table { .. }) => {
                self.close_leaf();
            }
            Some(Leaf::IndentedCode { pending, .. }) => {
                let mut c = cur;
                c.skip_indent_up_to(4);
                pending.push(c.segment(content_end));
            }
            // Only reachable when containers went unmatched
            Some(Leaf::FencedCode { .. }) => debug_assert!(!all_matched),
            None => {}
        }
        if !all_matched {
            self.hand_back_line_ending(false);
            self.close_from(matched);
        }
        let blank = Span::new(cur.offset(), content_end);
        self.blanks.all.push(blank);
        // A blank line whose innermost container is a blockquote (`>` alone)
        // never loosens a surrounding list (cmark's last_line_blank exception).
        if !deepest_is_quote {
            self.blanks.loosening.push(blank);
        }
        // A list item can begin with at most one blank line:
        // an item that is still empty when a blank line arrives closes here.
        if self.leaf.is_none()
            && matches!(self.stack.last(), Some(OpenContainer::Item { children, .. }) if children.is_empty())
        {
            self.close_from(self.stack.len() - 1);
        }
    }

    /// Step 4, for a line that failed to match every container:
    /// the open paragraph absorbs it lazily (returns `true`),
    /// or the unmatched containers close and the line goes on to steps 4b–5.
    /// A directive in the unmatched suffix blocks laziness:
    /// its content never accepts lazy lines (micromark's nonLazyLine),
    /// so the directive (and the paragraph inside it) end instead.
    ///
    /// The probe runs with the paragraph-interrupt restrictions off (empty items, ordered markers ≠ 1, HTML type 7):
    /// the paragraph being lazily continued sits in an unmatched deeper container,
    /// so a new block would attach beside it, not interrupt it.
    /// Setext underlines are deliberately absent, a lazy line is never an underline.
    fn lazy_continuation(&mut self, cur: Cursor<'s>, matched: usize, content_end: u32) -> bool {
        let start = cur.flow_start().and_then(|c| self.probe(c.tail(), false));
        let directive_between =
            self.stack[matched..].iter().any(|c| matches!(c, OpenContainer::Directive { .. }));
        if !directive_between && let Some(Leaf::Paragraph { segments, .. }) = &mut self.leaf {
            if start.is_none() {
                segments.push(IrSeg { seg: cur.segment(content_end), lazy: true });
                return true;
            }
            // micromark quirk: on a lazy line a complete type 7 tag interrupts the paragraph after all
            // (`html-flow.js` lifts the restriction when `parser.lazy[line]`),
            // and with a line ending after it the block opens inside the still-open container:
            // `document.js` sees no events from the in-flight attempt, so nothing closes.
            // At EOF without a line ending the container closes and the block opens outside
            // (micromark's own test pins that case).
            // cmark ≥ 0.30 reads the line as paragraph text instead; Prettier prints micromark's tree.
            // Only the paragraph closes here; step 5 opens the block from the raw line, indent included.
            let has_line_ending = (content_end as usize) < self.source.len();
            if has_line_ending && matches!(start, Some(Start::Html { kind: 7 })) {
                self.close_leaf();
                return false;
            }
        }
        // micromark's document tokenizer feeds the line ending before a new container
        // (list item, blockquote, footnote definition) to the still-open flow,
        // and cuts before it for a flow start (heading, fence, paragraph, …).
        let opens_container = matches!(
            start,
            Some(Start::Quote | Start::ListItem { .. } | Start::FootnoteDef { .. })
        );
        self.hand_back_line_ending(opens_container);
        self.close_from(matched);
        false
    }

    /// The line ending before a container-mismatch close, per `lazy_continuation`'s note:
    /// a raw HTML block (types 1–5) keeps it in `html.value` only when the line opens a container;
    /// an unclosed fence loses its last, empty line to it otherwise, a blank-line close included.
    /// HTML types 6–7 end at their last content line either way.
    fn hand_back_line_ending(&mut self, opens_container: bool) {
        match &mut self.leaf {
            Some(Leaf::Html { kind: 1..=5, trailing_newline, .. }) if opens_container => {
                *trailing_newline = true;
            }
            Some(Leaf::FencedCode { lines, end, .. }) if !opens_container => {
                if let Some(blank) = lines.pop_if(|l| l.span.is_empty() && l.padding == 0) {
                    *end = blank.span.start;
                }
            }
            _ => {}
        }
    }

    /// EOF (real, or a directive's closing fence) with a raw HTML block (types 1–5) still open:
    /// the block only ends at its closing marker, so every line ending up to here is content,
    /// the last one included (micromark's `html.value` ends with it).
    /// Not under a blockquote at any depth: its prefix match owns the line ending there.
    /// Types 6–7 end at the last content line.
    fn mark_html_eof(&mut self, has_line_ending: bool) {
        if has_line_ending
            && !self.stack.iter().any(|c| matches!(c, OpenContainer::Quote { .. }))
            && let Some(Leaf::Html { kind: 1..=5, trailing_newline, .. }) = &mut self.leaf
        {
            *trailing_newline = true;
        }
    }

    /// Step 4b: offers the line to the open non-verbatim leaf.
    /// HTML types 6–7 take every line, a table any line that is not another block start,
    /// indented code any line with 4+ columns of indent (buffered blank lines become content first).
    /// A leaf that refuses the line closes here.
    fn continue_leaf(&mut self, cur: &mut Cursor<'s>, content_end: u32) -> Continuation {
        match &mut self.leaf {
            Some(Leaf::Html { kind: 6 | 7, lines, end, .. }) => {
                lines.push(cur.segment(content_end));
                *end = content_end;
                Continuation::Consumed
            }
            Some(Leaf::IndentedCode { lines, pending, end, .. }) if cur.indent_cols() >= 4 => {
                cur.advance_cols(4);
                lines.append(pending);
                lines.push(cur.segment(content_end));
                *end = content_end;
                Continuation::Consumed
            }
            Some(Leaf::IndentedCode { .. }) => {
                // Any non-indented line ends indented code; nothing is interrupted
                // (micromark applies the paragraph restriction here too: `DIVERGENCES.md`, "List after indented code").
                self.close_leaf();
                Continuation::Open
            }
            Some(Leaf::Table { .. }) => {
                if self.continue_table(*cur, content_end) {
                    Continuation::Consumed
                } else {
                    self.close_leaf();
                    Continuation::Open
                }
            }
            Some(Leaf::Paragraph { .. }) | None => Continuation::Open,
            Some(Leaf::FencedCode { .. } | Leaf::Html { .. }) => {
                unreachable!("verbatim leaves are consumed by step 2 or closed by step 4")
            }
        }
    }

    /// Appends the rest of the line to the open paragraph, or opens one.
    /// Continuation lines keep their leading whitespace raw,
    /// text rendering strips it in the inline phase, but code spans crossing the line ending must see it.
    /// A new paragraph starts at its first content byte.
    fn paragraph_text(&mut self, cur: Cursor<'s>, content_end: u32, deep: bool) {
        if let Some(Leaf::Paragraph { segments, last_deep }) = &mut self.leaf {
            segments.push(IrSeg { seg: cur.segment(content_end), lazy: false });
            *last_deep = deep;
        } else {
            debug_assert!(self.leaf.is_none());
            self.close_list_top();
            let mut cur = cur;
            cur.skip_all_ws();
            let seg = IrSeg { seg: cur.segment(content_end), lazy: false };
            self.leaf = Some(Leaf::Paragraph { segments: vec![seg], last_deep: deep });
        }
    }

    /// Matches the open container stack against a line, consuming the prefixes it recognizes.
    /// Directive closing fences are checked outermost-first,
    /// before anything deeper sees the line
    /// (micromark runs each container's closing-fence attempt ahead of its content chunk).
    fn match_stack(&self, cur: &mut Cursor<'s>) -> StackMatch {
        let mut matched = 0usize;
        for i in 0..self.stack.len() {
            let ok = match &self.stack[i] {
                OpenContainer::Quote { .. } => {
                    let save = *cur;
                    cur.skip_indent_up_to(3);
                    if cur.partial == 0 && cur.tail().starts_with('>') {
                        cur.bump(1);
                        cur.eat_one_space();
                        true
                    } else {
                        *cur = save;
                        false
                    }
                }
                OpenContainer::List { .. } => true,
                // A blank line still gives up the item's indent, at most what it has
                // (micromark's list continuation: `factorySpace(…, size + 1)` on blank),
                // so whitespace-only lines inside a fence or HTML block lose it.
                // Footnote definitions do not (their continuation takes a blank line as-is).
                OpenContainer::Item { content_indent, .. } if scan::is_blank(cur.tail()) => {
                    let avail = cur.indent_cols();
                    cur.advance_cols(avail.min(*content_indent));
                    true
                }
                OpenContainer::Item { content_indent, .. }
                | OpenContainer::Footnote { content_indent, .. } => {
                    if scan::is_blank(cur.tail()) {
                        true
                    } else if cur.indent_cols() >= *content_indent {
                        let indent = *content_indent;
                        cur.advance_cols(indent);
                        true
                    } else {
                        false
                    }
                }
                OpenContainer::Directive { fence_len, content_indent, .. } => {
                    // Measure the indent once; probe the close on a copy
                    let avail = cur.indent_cols();
                    let mut probe = *cur;
                    probe.advance_cols(avail.min(3));
                    if probe.partial == 0 && scan::fence_close(probe.tail(), b':', *fence_len) {
                        let fence = Span::at(probe.offset(), 0..probe.tail().len());
                        return StackMatch::DirectiveClose { index: i, fence };
                    }
                    cur.advance_cols(avail.min(*content_indent));
                    true
                }
            };
            if !ok {
                break;
            }
            matched = i + 1;
        }
        StackMatch::Prefixes(matched)
    }

    /// See [`probe`]; forwards the engine's constructs.
    fn probe(&self, tail: &str, para_open: bool) -> Option<Start> {
        probe(&self.constructs, tail, para_open)
    }

    /// Closes the leaf into its container.
    fn close_leaf(&mut self) {
        let Some(leaf) = self.leaf.take() else { return };
        let ir = match leaf {
            Leaf::Paragraph { segments, .. } => {
                let rest = self.finish_paragraph(segments);
                let (Some(first), Some(last)) = (rest.first(), rest.last()) else { return };
                Ir::Paragraph {
                    span: Span::new(first.seg.span.start, last.seg.span.end),
                    segments: rest,
                }
            }
            Leaf::IndentedCode { lines, start, end, .. } => {
                Ir::Code { fenced: None, lines, span: Span::new(start, end) }
            }
            Leaf::FencedCode { fence: b'$', info, lines, start, end, .. } => {
                Ir::MathBlock { meta: info, lines, span: Span::new(start, end) }
            }
            Leaf::FencedCode { fence, info, lines, start, end, .. } => {
                Ir::Code { fenced: Some((fence, info)), lines, span: Span::new(start, end) }
            }
            Leaf::Html { kind, lines, trailing_newline, start, end } => {
                Ir::Html { kind, lines, trailing_newline, span: Span::new(start, end) }
            }
            Leaf::Table { align, rows, start, end } => {
                Ir::Table { align, rows, span: Span::new(start, end) }
            }
        };
        self.attach(ir);
    }

    /// Attaches stripped definitions and registers their labels,
    /// the inline phase's reference map fills here, with no extra tree walk.
    fn attach_definitions(&mut self, defs: Vec<Ir>) {
        for def in defs {
            if let Ir::Definition { label, .. } = &def {
                let text = Segment::join(self.source, label);
                self.refs.insert(crate::syntax::label::normalize(&text).into_owned());
            }
            self.attach(def);
        }
    }

    /// The paragraph-close prologue shared by paragraphs and setext headings:
    /// leading definitions are stripped and attached, and the last line loses its trailing whitespace.
    /// Returns the remaining paragraph lines, possibly none.
    fn finish_paragraph(&mut self, segments: Vec<IrSeg>) -> Vec<IrSeg> {
        let (defs, mut rest) = refdef::strip(self.source, segments);
        self.attach_definitions(defs);
        trim_last(self.source, &mut rest);
        rest
    }

    /// The prologue of every block start that is not a list item:
    /// it ends the open leaf, and a list on top of the stack (only a compatible item continues one).
    fn close_leaf_and_list(&mut self) {
        self.close_leaf();
        self.close_list_top();
    }

    /// Closes open containers so that `keep` remain.
    fn close_from(&mut self, keep: usize) {
        self.close_leaf();
        while self.stack.len() > keep {
            self.close_container();
        }
    }

    /// Closes an open `List` sitting on top of the stack
    /// (any new block other than a compatible item ends the list).
    fn close_list_top(&mut self) {
        debug_assert!(self.leaf.is_none());
        if matches!(self.stack.last(), Some(OpenContainer::List { .. })) {
            self.close_container();
        }
    }

    fn close_container(&mut self) {
        debug_assert!(self.leaf.is_none());
        match self.stack.pop().expect("close_container on empty stack") {
            OpenContainer::Quote { children, start, end } => {
                self.attach(Ir::Quote { children, span: Span::new(start, end) });
            }
            OpenContainer::List { items, ordered, marker, start } => {
                // Items arrive in source order; the last one ends the list
                let end = items.last().map_or(start, |item| item.span.end);
                self.attach(Ir::List { ordered, marker, items, span: Span::new(start, end) });
            }
            OpenContainer::Footnote { children, label, start, end, .. } => {
                self.attach(Ir::FootnoteDefinition {
                    label,
                    children,
                    span: Span::new(start, end),
                });
            }
            OpenContainer::Directive { children, opening, closing, end, .. } => {
                self.attach(Ir::ContainerDirective {
                    opening,
                    closing,
                    children,
                    span: Span::new(opening.start, end),
                });
            }
            OpenContainer::Item { children, marker, padding, start, end, .. } => {
                let Some(OpenContainer::List { items, .. }) = self.stack.last_mut() else {
                    unreachable!("list items always sit in a list");
                };
                items.push(IrItem { marker, padding, children, span: Span::new(start, end) });
            }
        }
    }

    /// Attaches a finished block to the current innermost container.
    fn attach(&mut self, ir: Ir) {
        let end = ir.span().end;
        let children = match self.stack.last_mut() {
            Some(
                OpenContainer::Quote { children, end: e, .. }
                | OpenContainer::Item { children, end: e, .. }
                | OpenContainer::Footnote { children, end: e, .. }
                | OpenContainer::Directive { children, end: e, .. },
            ) => {
                *e = (*e).max(end);
                children
            }
            Some(OpenContainer::List { .. }) => {
                // Lists never receive non-item children directly
                self.close_container();
                return self.attach(ir);
            }
            None => &mut self.root,
        };
        // Siblings are attached in source order
        debug_assert!(children.last().is_none_or(|prev| prev.span().end <= ir.span().start));
        children.push(ir);
    }
}

/// Content end (before the line terminator) of the line starting at `start`.
#[expect(clippy::cast_possible_truncation)] // sources are bounded to u32
fn line_content_end(source: &str, start: u32) -> u32 {
    let bytes = source.as_bytes();
    let mut i = start as usize;
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    i as u32
}

/// Start of the line past the terminator at `content_end`;
/// `None` when no further line exists (a trailing terminator opens no empty line).
#[expect(clippy::cast_possible_truncation)] // sources are bounded to u32
fn next_line_start(source: &str, content_end: u32) -> Option<u32> {
    let bytes = source.as_bytes();
    let i = content_end as usize;
    if i >= bytes.len() {
        return None;
    }
    let next = i + if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
    (next < bytes.len()).then_some(next as u32)
}

/// Drops the trailing whitespace of a closed paragraph's last line from its segment:
/// it is never content (a hard break needs a following line),
/// so the paragraph or setext heading span ends before it, like an ATX heading's.
fn trim_last(source: &str, segments: &mut [IrSeg]) {
    if let Some(last) = segments.last_mut() {
        last.seg.span.end = last.seg.span.start + trimmed_len(last.seg.span.slice(source));
    }
}

/// Byte length of `tail` with trailing whitespace removed.
#[expect(clippy::cast_possible_truncation)] // in-line lengths
fn trimmed_len(tail: &str) -> u32 {
    tail.trim_end_matches([' ', '\t']).len() as u32
}
