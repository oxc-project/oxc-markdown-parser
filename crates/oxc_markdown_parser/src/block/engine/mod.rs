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
//! Blocks close and attach to their container in `close.rs`.
//! Extension constructs with their own continuation rules live beside it:
//! `table.rs` (header stealing, row continuation) and `liquid.rs` (multi-line validation).

mod close;
mod liquid;
mod open;
mod state;
mod table;

pub use state::BlankLines;

use crate::inline::RefMap;
use crate::options::Constructs;
use crate::pos::Span;
use state::{Continuation, Leaf, OpenContainer, StackMatch};

use super::cursor::Cursor;
use super::ir::{Ir, IrSeg};
use super::probe::{Start, probe};
use super::scan;

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
        self.close_from(0);
        self.refs.finish();
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
        if cur.is_blank() {
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
        // The line belongs to the innermost blockquote whose `>` it carries
        // (micromark's blockquote position covers it; the blank table alone would make
        // a `>` line at the end of a list item look like a gap after the item).
        if let Some(end) = self.stack[..matched].iter_mut().rev().find_map(|c| match c {
            OpenContainer::Quote { end, .. } => Some(end),
            _ => None,
        }) {
            *end = (*end).max(content_end);
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
        let mut start = cur.flow_start().and_then(|c| self.probe(c.tail(), false));
        // A liquid opener interrupts only as a complete construct
        if matches!(start, Some(Start::Liquid))
            && let Some(c) = cur.flow_start()
            && self.liquid_scan(c, content_end).is_none()
        {
            start = None;
        }
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
                OpenContainer::Item { content_indent, .. } if cur.is_blank() => {
                    let avail = cur.indent_cols();
                    cur.advance_cols(avail.min(*content_indent));
                    true
                }
                OpenContainer::Item { content_indent, .. }
                | OpenContainer::Footnote { content_indent, .. } => {
                    if cur.is_blank() {
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
