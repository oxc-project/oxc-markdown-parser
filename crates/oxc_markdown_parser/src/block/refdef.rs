//! Link reference definition stripping.
//!
//! Runs when a paragraph closes:
//! leading `[label]: dest "title"` runs are peeled off into `Definition` nodes and the rest stays a paragraph.
//! A single forward pass with a 999-character label cap (the spec's own DoS guard);
//! a failed candidate leaves everything from its first line intact.

use crate::ast::Destination;
use crate::inline::input::Input;
use crate::syntax::{link_target, skip_ws};

use super::ir::{Ir, IrSeg};

/// Splits leading reference definitions off a closing paragraph's segments.
pub fn strip(source: &str, mut segments: Vec<IrSeg>) -> (Vec<Ir>, Vec<IrSeg>) {
    if !segments.first().is_some_and(|s| s.seg.span.slice(source).starts_with('[')) {
        return (Vec::new(), segments);
    }

    // The same joined view the inline phase uses (`\n` separators, exact per-byte offsets)
    let input = Input::new(source, &segments);
    let joined = input.text.as_str();

    let mut defs = Vec::new();
    let mut pos = 0usize;
    while pos < joined.len() {
        let Some((def, end)) = parse_one(&input, pos) else {
            break;
        };
        defs.push(def);
        pos = end;
    }

    // A definition always ends at a line ending,
    // so `pos` sits at a line start (or past the end); everything before it was consumed.
    let consumed_lines = if pos >= joined.len() { segments.len() } else { input.lines_before(pos) };
    segments.drain(..consumed_lines);
    // A raw continuation line promoted to first line starts the remaining paragraph;
    // its leading whitespace is not content.
    if consumed_lines > 0
        && let Some(first) = segments.first_mut()
    {
        let raw = first.seg.span.slice(source);
        let trim = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        first.seg.span.start += u32::try_from(trim).unwrap_or(0);
        first.seg.padding = 0;
    }
    (defs, segments)
}

/// Parses one definition starting at `pos` (which is a line start).
/// Returns the node and the joined position just past its final line ending.
fn parse_one(input: &Input, pos: usize) -> Option<(Ir, usize)> {
    let s = input.text.as_str();
    let b = s.as_bytes();
    // Any amount of leading spaces/tabs:
    // the line already sits inside the content chunk,
    // and micromark's definition factory strips its prefix without a column limit
    // (`    [b]: /u` after a definition is a second definition, not code);
    // the joined view renders partial-tab padding as spaces, which this skips too.
    // The ≤3-column rule only picks which lines enter the chunk.
    // The node starts at the `[`, never at a continuation line's raw indent.
    let start = skip_ws(b, pos, 0);
    // `strip` checks the first line; later candidates must open a label themselves
    // (`-|-]:` after a definition is paragraph text, not a label).
    if b.get(start) != Some(&b'[') {
        return None;
    }
    let mut p = start;

    let label_start = p + 1;
    let label_close = link_target::label_end(s, p)?;
    let label_end = label_close - 1;
    p = label_close;
    if b.get(p) != Some(&b':') {
        return None;
    }
    p = skip_ws(b, p + 1, 1);

    let (dest, angle) = link_target::destination(s, p)?;
    if dest.is_empty() {
        return None;
    }
    let (dest_start, dest_end) = (dest.start, dest.end);
    p = dest_end;

    // Optional title, requiring whitespace after the destination;
    // on failure fall back to a title-less definition ending at the destination's line.
    let mut title = None;
    let mut content_end = dest_end;
    let mut end = None;
    let title_start = skip_ws(b, p, 1);
    if title_start > p
        && let Some(title_range) = link_target::title(s, title_start)
        && let Some(line_end) = at_line_end(b, title_range.end)
    {
        title = Some(input.pieces(title_range.clone()));
        content_end = title_range.end;
        end = Some(line_end);
    }
    let end = match end {
        Some(end) => end,
        None => at_line_end(b, dest_end)?,
    };

    let def = Ir::Definition {
        label: input.pieces(label_start..label_end),
        destination: Destination { span: input.span(dest_start..dest_end), angle_bracketed: angle },
        title,
        span: input.span(start..content_end),
    };
    Some((def, end))
}

/// If only spaces/tabs remain until the next newline (or EOF),
/// returns the position just past that line ending.
fn at_line_end(b: &[u8], mut p: usize) -> Option<usize> {
    while let Some(&c) = b.get(p) {
        match c {
            b' ' | b'\t' => p += 1,
            b'\n' => return Some(p + 1),
            _ => return None,
        }
    }
    Some(p)
}
