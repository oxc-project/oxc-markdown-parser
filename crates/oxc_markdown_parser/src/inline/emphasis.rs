//! Emphasis and strikethrough via the CommonMark delimiter stack.
//!
//! `*`/`_`/`~` runs are pushed as text nodes plus a [`Delim`] carrying their flanking classification,
//! then resolved in two family passes (attention vs strikethrough) mirroring micromark's resolver mechanics:
//!
//! - At the end of a text run, the family whose first delimiter appeared first resolves first
//!   (micromark registers resolvers in first-use order);
//!   inside brackets and inside every formed pair,
//!   strikethrough resolves before attention (micromark's fixed `insideSpan` order).
//! - When a pair forms, live delimiters of the *other* family between the pair are re-resolved recursively
//!   within the new node's children;
//!   the pair's own family between them is dead (the spec's rule).
//!
//! Exhausted delimiters stay in place with `remaining == 0`,
//! so indices remain stable for openers_bottom.
//! Node spans come straight from the delimiter runs' own ranges, never from arithmetic over child spans.

use std::ops::Range;

use crate::syntax::unicode::{is_punctuation, is_whitespace};

use super::link::Bracket;
use super::{PN, Tokenizer};

#[derive(Clone, Copy)]
pub struct Delim {
    pub node: usize,
    pub marker: u8,
    /// Unconsumed delimiter characters (0 = dead entry).
    pub remaining: usize,
    pub can_open: bool,
    pub can_close: bool,
}

impl Tokenizer<'_, '_> {
    pub(crate) fn delimiter_run(&mut self) {
        let bytes = self.t.as_bytes();
        let marker = bytes[self.pos];
        let start = self.pos;
        let run = bytes[start..].iter().take_while(|&&b| b == marker).count();
        let prev = self.t[..start].chars().next_back();
        let next = self.t[start + run..].chars().next();
        // GFM strikethrough: runs of one (only with `singleTilde`) or two tildes;
        // longer runs never participate (micromark's construct fails on them).
        let min = if self.constructs.gfm_strikethrough_single_tilde { 1 } else { 2 };
        let valid_run = marker != b'~' || (min..=2).contains(&run);
        let (can_open, can_close) = if valid_run {
            classify(marker, prev, next, self.constructs.gfm_strikethrough)
        } else {
            (false, false)
        };
        self.flush_text(start);
        self.nodes.push(PN::Text(start..start + run));
        // micromark registers a family's resolver when its construct succeeds,
        // which is every `*`/`_` run and every valid-length `~` run,
        // regardless of whether the run can open or close.
        if valid_run {
            self.first_delim_tilde.get_or_insert(marker == b'~');
        }
        if can_open || can_close {
            self.delims.push(Delim {
                node: self.nodes.len() - 1,
                marker,
                remaining: run,
                can_open,
                can_close,
            });
        }
        self.pos = start + run;
        self.text_start = self.pos;
    }

    /// Resolution inside a bracket label:
    /// micromark's fixed `insideSpan` order (strikethrough, then attention).
    pub(crate) fn process_emphasis(&mut self, bottom: usize) {
        resolve(&mut self.nodes, &mut self.delims, &mut self.brackets, bottom, true);
    }

    /// End-of-run resolution: the first-used family resolves first.
    pub(crate) fn process_emphasis_final(&mut self) {
        let tilde_first = self.first_delim_tilde.unwrap_or(false);
        resolve(&mut self.nodes, &mut self.delims, &mut self.brackets, 0, tilde_first);
    }
}

fn resolve(
    nodes: &mut Vec<PN>,
    delims: &mut [Delim],
    brackets: &mut [Bracket],
    bottom: usize,
    tilde_first: bool,
) {
    let order = if tilde_first { [true, false] } else { [false, true] };
    for tilde_pass in order {
        if delims[bottom..]
            .iter()
            .any(|d| d.remaining > 0 && d.can_close && (d.marker == b'~') == tilde_pass)
        {
            pass(nodes, delims, brackets, bottom, tilde_pass);
        }
    }
}

/// One family pass of the spec's "process emphasis" over `delims[bottom..]`.
fn pass(
    nodes: &mut Vec<PN>,
    delims: &mut [Delim],
    brackets: &mut [Bracket],
    bottom: usize,
    tilde_pass: bool,
) {
    // Lowest index worth scanning per closer class:
    // [marker is `_`][closer len % 3][closer can also open].
    let mut openers_bottom = [[[bottom; 2]; 3]; 2];
    let mut closer = bottom;
    while closer < delims.len() {
        let c = &delims[closer];
        if (c.marker == b'~') != tilde_pass || !c.can_close || c.remaining == 0 {
            closer += 1;
            continue;
        }
        // Strikethrough pairs whole equal-sized runs,
        // with no rule of three and no openers_bottom participation.
        let class = (usize::from(c.marker == b'_'), c.remaining % 3, usize::from(c.can_open));
        let scan_bottom =
            if tilde_pass { bottom } else { openers_bottom[class.0][class.1][class.2] };

        let mut opener = None;
        let mut i = closer;
        while i > scan_bottom {
            i -= 1;
            let o = &delims[i];
            if o.marker != c.marker || !o.can_open || o.remaining == 0 {
                continue;
            }
            if tilde_pass {
                if o.remaining == c.remaining {
                    opener = Some(i);
                    break;
                }
                continue;
            }
            // Rule of three, micromark's exact form:
            // current (partially consumed) sizes, not original run lengths.
            // micromark mutates sequence boundaries as it pairs.
            let forbidden = (o.can_close || c.can_open)
                && !c.remaining.is_multiple_of(3)
                && (o.remaining + c.remaining).is_multiple_of(3);
            if !forbidden {
                opener = Some(i);
                break;
            }
        }
        let Some(opener) = opener else {
            if !tilde_pass {
                openers_bottom[class.0][class.1][class.2] = closer;
                if !delims[closer].can_open {
                    delims[closer].remaining = 0;
                }
            }
            closer += 1;
            continue;
        };

        let use_n = if tilde_pass {
            delims[closer].remaining
        } else if delims[opener].remaining >= 2 && delims[closer].remaining >= 2 {
            2
        } else {
            1
        };
        let opener_node = delims[opener].node;
        let closer_node = delims[closer].node;

        let mut inner: Vec<PN> = nodes.drain(opener_node + 1..closer_node).collect();
        // Same-marker runs never touch (they'd be one run),
        // so at least one node sat between the pair.
        let removed = closer_node - opener_node - 1;
        debug_assert!(removed >= 1);
        let shift = removed - 1;

        // Live delimiters of the other family between the pair resolve
        // within the new node's children (micromark's `insideSpan`);
        // the pair's own family between them is dead either way.
        let mut inner_delims: Vec<Delim> = Vec::new();
        for d in &mut delims[opener + 1..closer] {
            if d.remaining > 0 && (d.marker == b'~') != tilde_pass {
                inner_delims.push(Delim { node: d.node - (opener_node + 1), ..*d });
            }
            d.remaining = 0;
        }
        if !inner_delims.is_empty() {
            resolve(&mut inner, &mut inner_delims, &mut [], 0, true);
        }

        // Trim the used characters off the opener's tail and (the now adjacent) closer's head;
        // they become the new node's span.
        let o_end = text_range(nodes, opener_node).end;
        text_range(nodes, opener_node).end = o_end - use_n;
        let c_start = text_range(nodes, opener_node + 1).start;
        text_range(nodes, opener_node + 1).start = c_start + use_n;

        let emph = PN::Emph {
            marker: delims[opener].marker,
            strong: !tilde_pass && use_n == 2,
            children: inner,
            r: o_end - use_n..c_start + use_n,
        };
        nodes.insert(opener_node + 1, emph);

        for d in delims.iter_mut() {
            if d.node >= closer_node {
                d.node -= shift;
            }
        }
        for b in brackets.iter_mut() {
            if b.node >= closer_node {
                b.node -= shift;
            }
        }

        delims[opener].remaining -= use_n;
        delims[closer].remaining -= use_n;
        // The opener's size changed mod 3,
        // so rule-of-three verdicts recorded in openers_bottom above it are stale
        // (micromark judges with current sizes and rescans everything):
        // rewind same-marker bottoms down to the opener.
        // A used-up opener needs no rewind, it can never pair again,
        // so its recorded verdicts stay correct.
        if !tilde_pass && delims[opener].remaining > 0 {
            let m = usize::from(delims[opener].marker == b'_');
            for b in openers_bottom[m].iter_mut().flatten() {
                *b = (*b).min(opener);
            }
        }
        if delims[closer].remaining == 0 {
            closer += 1;
        }
    }
}

fn text_range(nodes: &mut [PN], node: usize) -> &mut Range<usize> {
    match &mut nodes[node] {
        PN::Text(r) => r,
        _ => unreachable!("delimiter nodes are text runs"),
    }
}

/// Flanking classification, micromark's `attention.js` version.
///
/// This deliberately follows micromark rather than the spec's prose:
/// on top of the left/right-flanking rules,
/// a `*`/`_` run whose neighboring character is itself an attention marker
/// (`*` `_`, and `~` with strikethrough on) may always open (next) / close (prev).
/// A `~` run gets the plain attention rules,
/// no marker relaxation, no `_` adjustment (micromark's `gfm-strikethrough`).
/// Parse behavior targets micromark exactly, quirks included.
fn classify(marker: u8, prev: Option<char>, next: Option<char>, tilde: bool) -> (bool, bool) {
    let is_marker = |c: Option<char>| {
        marker != b'~' && (matches!(c, Some('*' | '_')) || (tilde && c == Some('~')))
    };
    let before = group(prev);
    let after = group(next);

    let open = after == 0 || (after == 2 && before != 0) || is_marker(next);
    let close = before == 0 || (before == 2 && after != 0) || is_marker(prev);
    if marker == b'_' {
        (open && (before != 0 || !close), close && (after != 0 || !open))
    } else {
        (open, close)
    }
}

/// 1 = whitespace (sequence boundaries included), 2 = punctuation, 0 = other.
///
/// Whitespace is micromark's `unicodeWhitespace` (JS `\s`),
/// not Rust's `char::is_whitespace`: U+0085 is "other", U+FEFF is whitespace.
fn group(c: Option<char>) -> u8 {
    match c {
        None => 1,
        Some(c) if is_whitespace(c) => 1,
        Some(c) if is_punctuation(c) => 2,
        Some(_) => 0,
    }
}
