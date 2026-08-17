//! GFM table rows and delimiter rows.

use std::ops::Range;

use crate::ast::TableAlign;
use crate::syntax::skip_ws;

use super::trim_range;

/// Splits a table row line into trimmed cell byte-ranges.
/// An escaped `\|` does not separate; leading/trailing pipes are decoration;
/// cells are trimmed of spaces/tabs.
/// Content after the last pipe forms a final cell only when non-empty.
pub fn table_row_cells(line: &str) -> Vec<Range<usize>> {
    let s = line.trim_end_matches([' ', '\t']);
    let bytes = s.as_bytes();
    let mut start = skip_ws(bytes, 0, 0);
    if bytes.get(start) == Some(&b'|') {
        start += 1;
    }
    let mut cells = Vec::new();
    let mut j = start;
    while j < bytes.len() {
        match bytes[j] {
            // A backslash escapes the next byte (`\|` stays in the cell)
            b'\\' => j += 1,
            b'|' => {
                cells.push(trim_range(s, start..j));
                start = j + 1;
            }
            _ => {}
        }
        j += 1;
    }
    let rest = trim_range(s, start..s.len());
    if !rest.is_empty() {
        cells.push(rest);
    }
    cells
}

/// A table delimiter row: cells of `:?-+:?` only.
/// No pipe is required, all-dash rows never reach here because setext/thematic-break/list probes win first.
/// Returns per-column alignment.
pub fn table_delimiter_row(tail: &str) -> Option<Vec<TableAlign>> {
    if tail.bytes().any(|b| !matches!(b, b'|' | b'-' | b':' | b' ' | b'\t')) {
        return None;
    }
    let cells = table_row_cells(tail);
    if cells.is_empty() {
        return None;
    }
    let mut align = Vec::with_capacity(cells.len());
    for cell in cells {
        let c = &tail[cell];
        let left = c.starts_with(':');
        let right = c.ends_with(':') && c.len() > 1;
        let dashes = &c[usize::from(left)..c.len() - usize::from(right)];
        if dashes.is_empty() || dashes.bytes().any(|b| b != b'-') {
            return None;
        }
        align.push(match (left, right) {
            (true, true) => TableAlign::Center,
            (true, false) => TableAlign::Left,
            (false, true) => TableAlign::Right,
            (false, false) => TableAlign::None,
        });
    }
    Some(align)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_cells() {
        assert_eq!(table_row_cells("| a | b |"), vec![2..3, 6..7]);
        assert_eq!(table_row_cells("a|b"), vec![0..1, 2..3]);
        // Escaped pipes stay in the cell; empty cells are kept between pipes
        assert_eq!(table_row_cells(r"| a\|b | |"), vec![2..6, 9..9]);
        // Trailing content without a closing pipe is a cell only when non-empty
        assert_eq!(table_row_cells("| a |  "), vec![2..3]);
        assert_eq!(table_row_cells("| a | b"), vec![2..3, 6..7]);
    }

    #[test]
    fn delimiter_rows() {
        assert_eq!(
            table_delimiter_row("| --- | :-- | --: | :-: |"),
            Some(vec![TableAlign::None, TableAlign::Left, TableAlign::Right, TableAlign::Center])
        );
        assert_eq!(table_delimiter_row("-|-"), Some(vec![TableAlign::None, TableAlign::None]));
        assert_eq!(table_delimiter_row("| : |"), None, "a colon alone is not a column");
        assert_eq!(table_delimiter_row("| a |"), None);
        assert_eq!(table_delimiter_row("|"), None);
    }
}
