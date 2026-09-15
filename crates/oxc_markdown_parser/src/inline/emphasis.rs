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
//! Resolution never moves a node: a pair only trims its two delimiter runs and is recorded as a [`Formed`],
//! so every index stays valid; [`build`] makes the tree once at the end.
//! Exhausted delimiters stay in place with `remaining == 0`.
//! Node spans come straight from the delimiter runs' own ranges, never from arithmetic over child spans.

use std::ops::Range;

use crate::{
    Constructs, syntax,
    syntax::unicode::{is_punctuation, is_whitespace},
};

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

/// A pair the resolver formed:
/// the node's extent (markers included), its marker and marker length.
pub struct Formed {
    pub r: Range<usize>,
    pub marker: u8,
    pub len: u8,
}

impl Tokenizer<'_, '_> {
    pub(crate) fn delimiter_run(&mut self) {
        let bytes = self.t.as_bytes();
        let marker = bytes[self.pos];
        let start = self.pos;
        let len = syntax::run_len(&bytes[start..], marker);
        let prev = self.t[..start].chars().next_back();
        let next = self.t[start + len..].chars().next();
        let run = Run::new(marker, len, prev, next, self.constructs);
        self.flush_text(start);
        self.nodes.push(PN::Text(start..start + len));
        // micromark registers a family's resolver when its construct succeeds,
        // which is every `*`/`_` run and every valid-length `~` run,
        // regardless of whether the run can open or close.
        if run.valid {
            self.first_delim_tilde.get_or_insert(marker == b'~');
        }
        if run.can_open || run.can_close {
            self.delims.push(Delim {
                node: self.nodes.len() - 1,
                marker,
                remaining: len,
                can_open: run.can_open,
                can_close: run.can_close,
            });
        }
        self.pos = start + len;
        self.text_start = self.pos;
    }

    /// Resolution inside a bracket label:
    /// micromark's fixed `insideSpan` order (strikethrough, then attention).
    pub(crate) fn process_emphasis(&mut self, bottom: usize) {
        resolve(&mut self.nodes, &mut self.delims[bottom..], true, &mut self.formed);
    }

    /// End-of-run resolution: the first-used family resolves first.
    pub(crate) fn process_emphasis_final(&mut self) {
        let tilde_first = self.first_delim_tilde.unwrap_or(false);
        resolve(&mut self.nodes, &mut self.delims, tilde_first, &mut self.formed);
    }
}

fn resolve(nodes: &mut [PN], delims: &mut [Delim], tilde_first: bool, formed: &mut Vec<Formed>) {
    let order = if tilde_first { [true, false] } else { [false, true] };
    for tilde_pass in order {
        if delims.iter().any(|d| d.remaining > 0 && d.can_close && (d.marker == b'~') == tilde_pass)
        {
            pass(nodes, delims, tilde_pass, formed);
        }
    }
}

/// One family pass of the spec's "process emphasis" over `delims`.
#[expect(clippy::cast_possible_truncation)] // a pair uses one or two characters
fn pass(nodes: &mut [PN], delims: &mut [Delim], tilde_pass: bool, formed: &mut Vec<Formed>) {
    // Lowest index worth scanning per closer class:
    // [marker is `_`][closer len % 3][closer can also open].
    let mut openers_bottom = [[[0; 2]; 3]; 2];
    // Strikethrough pairs equal-sized runs only:
    // a failed scan rules out every opener of that size below.
    let mut tilde_bottom = [0; 3];
    let mut closer = 0;
    while closer < delims.len() {
        let c = &delims[closer];
        if (c.marker == b'~') != tilde_pass || !c.can_close || c.remaining == 0 {
            closer += 1;
            continue;
        }
        let class = (usize::from(c.marker == b'_'), c.remaining % 3, usize::from(c.can_open));
        let scan_bottom = if tilde_pass {
            tilde_bottom[c.remaining]
        } else {
            openers_bottom[class.0][class.1][class.2]
        };

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
            if tilde_pass {
                tilde_bottom[c.remaining] = closer;
            } else {
                openers_bottom[class.0][class.1][class.2] = closer;
            }
            if !delims[closer].can_open {
                delims[closer].remaining = 0;
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

        // Live delimiters of the other family between the pair resolve
        // within the new node's children (micromark's `insideSpan`);
        // the pair's own family between them is dead either way.
        let between = &mut delims[opener + 1..closer];
        if between.iter().any(|d| d.remaining > 0 && (d.marker == b'~') != tilde_pass) {
            for d in between.iter_mut() {
                if (d.marker == b'~') == tilde_pass {
                    d.remaining = 0;
                }
            }
            resolve(nodes, between, true, formed);
        }
        for d in between.iter_mut() {
            d.remaining = 0;
        }

        // Trim the used characters off the opener's tail and the closer's head;
        // they become the new node's span.
        let o_end = text_range(nodes, opener_node).end;
        text_range(nodes, opener_node).end = o_end - use_n;
        let c_start = text_range(nodes, closer_node).start;
        text_range(nodes, closer_node).start = c_start + use_n;
        formed.push(Formed {
            r: o_end - use_n..c_start + use_n,
            marker: delims[opener].marker,
            len: use_n as u8,
        });

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

/// The tree of the flat `nodes` and the pairs `formed` over them (consumed; they nest properly).
/// Emptied delimiter runs are dropped.
pub fn build(nodes: impl IntoIterator<Item = PN>, formed: &mut Vec<Formed>) -> Vec<PN> {
    // Pre-order
    formed.sort_unstable_by(|a, b| a.r.start.cmp(&b.r.start).then(b.r.end.cmp(&a.r.end)));
    let mut pending = formed.drain(..).peekable();
    let mut out = Vec::new();
    let mut stack: Vec<(Formed, Vec<PN>)> = Vec::new();
    let close = |stack: &mut Vec<(Formed, Vec<PN>)>, out: &mut Vec<PN>| {
        let (pair, children) = stack.pop().expect("a pair is open");
        let node = PN::Emph { marker: pair.marker, len: pair.len, children, r: pair.r };
        stack.last_mut().map_or(out, |(_, children)| children).push(node);
    };
    for node in nodes {
        let at = node.start();
        // A pair holds at least one node, so none both ends and starts before this one
        while stack.last().is_some_and(|(pair, _)| pair.r.end <= at) {
            close(&mut stack, &mut out);
        }
        while pending.peek().is_some_and(|pair| pair.r.start <= at) {
            stack.push((pending.next().expect("peeked"), Vec::new()));
        }
        if matches!(&node, PN::Text(r) if r.is_empty()) {
            continue;
        }
        stack.last_mut().map_or(&mut out, |(_, children)| children).push(node);
    }
    debug_assert_eq!(pending.len(), 0, "every pair starts before the last node");
    while !stack.is_empty() {
        close(&mut stack, &mut out);
    }
    out
}

/// A `*` / `_` / `~` run as the tokenizer sees it: its marker, length and flanking.
/// Built by [`Run::new`], for [`pairs`] on a text a printer is about to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub marker: u8,
    pub len: usize,
    pub can_open: bool,
    pub can_close: bool,
    /// The run registers its family's resolver (every `*` / `_` run, a `~` run of a valid length).
    pub valid: bool,
}

impl Run {
    /// `prev` / `next` are the characters around the run (`None` at a boundary).
    pub fn new(
        marker: u8,
        len: usize,
        prev: Option<char>,
        next: Option<char>,
        constructs: &Constructs,
    ) -> Self {
        // GFM strikethrough: runs of one (only with `singleTilde`) or two tildes;
        // longer runs never participate (micromark's construct fails on them).
        let min = if constructs.gfm_strikethrough_single_tilde { 1 } else { 2 };
        let valid = marker != b'~' || (min..=2).contains(&len);
        let (can_open, can_close) = if valid {
            classify(marker, prev, next, constructs.gfm_strikethrough)
        } else {
            (false, false)
        };
        Self { marker, len, can_open, can_close, valid }
    }
}

/// One emphasis / strong / strikethrough node [`pairs`] would form: the runs' indices and its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pair {
    pub opener: usize,
    pub closer: usize,
    /// Two characters of each `*` / `_` run (`**` / `__`); a `~` pair is strikethrough, never strong.
    pub strong: bool,
}

/// The nodes the delimiter runs of one inline sequence form, sorted by position.
///
/// Decided by the parser's own resolver (flanking, rule of three, family order).
/// The runs are those of one bracket level (`in_link`: a link's text resolves on its own,
/// strikethrough first); text between them is opaque, only the runs' flanking matters.
/// At the top level the family used first resolves first.
///
/// A printer that changes a marker or joins lines runs it on what it is about to emit
/// and compares the result with the tree it prints, before committing to the change.
pub fn pairs(runs: &[Run], in_link: bool) -> Vec<Pair> {
    let tilde_first = in_link || runs.iter().find(|r| r.valid).is_some_and(|r| r.marker == b'~');
    // Lay the runs out as text nodes with one opaque text node between neighbors
    // (same-marker runs never touch; a pair always has a node between it).
    let mut nodes = Vec::with_capacity(runs.len() * 2);
    let mut delims = Vec::new();
    let mut starts = Vec::with_capacity(runs.len());
    let mut pos = 0;
    for run in runs {
        if !nodes.is_empty() {
            nodes.push(PN::Text(pos..pos + 1));
            pos += 1;
        }
        starts.push(pos);
        if run.can_open || run.can_close {
            delims.push(Delim {
                node: nodes.len(),
                marker: run.marker,
                remaining: run.len,
                can_open: run.can_open,
                can_close: run.can_close,
            });
        }
        nodes.push(PN::Text(pos..pos + run.len));
        pos += run.len;
    }
    let mut formed = Vec::new();
    resolve(&mut nodes, &mut delims, tilde_first, &mut formed);
    let run_at = |offset: usize| starts.partition_point(|&s| s <= offset) - 1;
    let mut out: Vec<Pair> = formed
        .iter()
        .map(|pair| Pair {
            opener: run_at(pair.r.start),
            closer: run_at(pair.r.end - 1),
            strong: pair.marker != b'~' && pair.len == 2,
        })
        .collect();
    out.sort_unstable();
    out
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
///
/// `prev` / `next` are the characters around the run; `None` is a boundary (whitespace class).
/// Returns `(can_open, can_close)`.
/// Public as [`crate::attention`] for printers that must know whether a literal run they emit would pair up.
pub fn classify(marker: u8, prev: Option<char>, next: Option<char>, tilde: bool) -> (bool, bool) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(marker: u8, len: usize, prev: Option<char>, next: Option<char>) -> Run {
        Run::new(marker, len, prev, next, &Constructs::markdown())
    }

    #[test]
    fn pairs_nested_and_rule_of_three() {
        // `***b***`: strong inside emphasis, both from the same two runs
        let runs = [run(b'*', 3, None, Some('b')), run(b'*', 3, Some('b'), None)];
        assert_eq!(
            pairs(&runs, false),
            vec![
                Pair { opener: 0, closer: 1, strong: false },
                Pair { opener: 0, closer: 1, strong: true }
            ]
        );
        // `*a**b*`: the `**` cannot pair (rule of three), the outer pair forms
        let runs = [
            run(b'*', 1, None, Some('a')),
            run(b'*', 2, Some('a'), Some('b')),
            run(b'*', 1, Some('b'), None),
        ];
        assert_eq!(pairs(&runs, false), vec![Pair { opener: 0, closer: 2, strong: false }]);
        // `**a _**b**_ c**` (micromark): `**a _` strong, then `**_ c**` strong
        let runs = [
            run(b'*', 2, None, Some('a')),
            run(b'_', 1, Some(' '), Some('*')),
            run(b'*', 2, Some('_'), Some('b')),
            run(b'*', 2, Some('b'), Some('_')),
            run(b'_', 1, Some('*'), Some(' ')),
            run(b'*', 2, Some('c'), None),
        ];
        assert_eq!(
            pairs(&runs, false),
            vec![
                Pair { opener: 0, closer: 2, strong: true },
                Pair { opener: 3, closer: 5, strong: true }
            ]
        );
    }

    #[test]
    fn pairs_family_order() {
        // `~~*a~~*`: strikethrough first (its run comes first) swallows the `*` opener
        let runs = [
            run(b'~', 2, None, Some('*')),
            run(b'*', 1, Some('~'), Some('a')),
            run(b'~', 2, Some('a'), Some('*')),
            run(b'*', 1, Some('~'), None),
        ];
        assert_eq!(pairs(&runs, true), vec![Pair { opener: 0, closer: 2, strong: false }]);
    }
}
