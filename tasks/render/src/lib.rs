//! Minimal CommonMark HTML renderer over the AST — test infrastructure
//! only, never a product renderer. It feeds the conformance runner (the
//! CommonMark and GFM spec suites, whose expectations are HTML; the
//! extension constructs are off there and render nothing). Mirrors cmark's
//! html.c output discipline (`cr()` before block tags, tight-list paragraph
//! unwrapping, houdini href escaping).

pub mod sig;

use std::borrow::Cow;
use std::fmt::Write as _;

use cow_utils::CowUtils;
use rustc_hash::FxHashMap;

use oxc_markdown_parser::{Segment, Span, ast::*, decode, label};

pub fn render<'s>(source: &'s str, root: &'s Root<'_>) -> String {
    let mut defs = FxHashMap::default();
    collect_defs(source, &root.children, &mut defs);
    let mut r = Renderer {
        source,
        out: String::with_capacity(source.len() + source.len() / 2),
        defs,
        in_table_cell: false,
    };
    for block in &root.children {
        r.block(block, false);
    }
    r.cr();
    r.out
}

#[derive(Clone, Copy)]
struct Def<'r> {
    destination: Destination,
    title: Option<&'r [Segment]>,
}

fn collect_defs<'r>(source: &str, blocks: &'r [Block<'_>], defs: &mut FxHashMap<String, Def<'r>>) {
    for block in blocks {
        match block {
            Block::Definition(d) => {
                defs.entry(label::normalize(&Segment::join(source, &d.label)).into_owned())
                    .or_insert(Def {
                        destination: d.destination,
                        title: d.title.as_ref().map(|t| t.as_slice()),
                    });
            }
            Block::Blockquote(q) => collect_defs(source, &q.children, defs),
            Block::List(l) => {
                for item in &l.children {
                    collect_defs(source, &item.children, defs);
                }
            }
            _ => {}
        }
    }
}

struct Renderer<'s> {
    source: &'s str,
    out: String,
    defs: FxHashMap<String, Def<'s>>,
    /// GFM: `\|` renders as `|` even inside code spans within table cells.
    /// A field, not a `block` argument: its extent is the whole inline
    /// subtree of a cell, not a single `block` call.
    in_table_cell: bool,
}

impl Renderer<'_> {
    fn cr(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    /// `tight`: child of a tight list item, so paragraphs render unwrapped.
    /// Applies to exactly that block, never to grandchildren via a container's recursion.
    fn block(&mut self, block: &Block, tight: bool) {
        match block {
            Block::Paragraph(p) => {
                if tight {
                    self.inlines(&p.children);
                } else {
                    self.cr();
                    self.out.push_str("<p>");
                    self.inlines(&p.children);
                    self.out.push_str("</p>\n");
                }
            }
            Block::Heading(h) => {
                self.cr();
                let _ = write!(self.out, "<h{}>", h.level);
                self.inlines(&h.children);
                let _ = writeln!(self.out, "</h{}>", h.level);
            }
            Block::ThematicBreak(_) => {
                self.cr();
                self.out.push_str("<hr />\n");
            }
            Block::CodeBlock(c) => {
                self.cr();
                self.out.push_str("<pre><code");
                if let CodeBlockKind::Fenced { info: Some(info), .. } = c.kind {
                    let (lang, _) = split_info(info.slice(self.source));
                    self.out.push_str(" class=\"language-");
                    self.out.push_str(&esc(&decode::decode(lang, true)));
                    self.out.push('"');
                }
                self.out.push('>');
                for line in &c.lines {
                    self.segment(line, true);
                    self.out.push('\n');
                }
                self.out.push_str("</code></pre>\n");
            }
            Block::HtmlBlock(h) => {
                self.cr();
                for (i, line) in h.lines.iter().enumerate() {
                    if i > 0 {
                        self.out.push('\n');
                    }
                    self.segment(line, false);
                }
                if h.trailing_newline {
                    self.out.push('\n');
                }
                // cmark: `cr()` after the literal as well
                self.cr();
            }
            Block::Blockquote(q) => {
                self.cr();
                self.out.push_str("<blockquote>\n");
                for child in &q.children {
                    self.block(child, false);
                }
                self.cr();
                self.out.push_str("</blockquote>\n");
            }
            Block::List(l) => {
                self.cr();
                let ordered = matches!(l.marker, ListMarker::Ordered { .. });
                if ordered {
                    let start = l
                        .children
                        .first()
                        .and_then(|item| ordered_start(item.marker, self.source))
                        .unwrap_or(1);
                    if start == 1 {
                        self.out.push_str("<ol>\n");
                    } else {
                        let _ = writeln!(self.out, "<ol start=\"{start}\">");
                    }
                } else {
                    self.out.push_str("<ul>\n");
                }
                for item in &l.children {
                    self.out.push_str("<li>");
                    let mut rest = item.children.as_slice();
                    if let Some(checkbox) = item.checkbox {
                        // The checkbox sits before the first paragraph's content
                        // (possibly behind stripped definitions), inside its <p> when the list is loose.
                        // micromark keeps the whitespace after `]` as text; across a line ending it is the line ending.
                        if !l.tight {
                            self.out.push_str("\n<p>");
                        }
                        self.out.push_str("<input type=\"checkbox\" disabled=\"\"");
                        if checkbox.checked {
                            self.out.push_str(" checked=\"\"");
                        }
                        self.out.push_str(" />");
                        // The parser only puts stripped definitions before that paragraph
                        while let [Block::Definition(_), tail @ ..] = rest {
                            rest = tail;
                        }
                        if let [Block::Paragraph(p), tail @ ..] = rest {
                            let gap = Span::new(checkbox.span.end, p.span.start).slice(self.source);
                            self.out.push_str(if gap.contains('\n') { "\n" } else { gap });
                            self.inlines(&p.children);
                            rest = tail;
                        }
                        if !l.tight {
                            self.out.push_str("</p>\n");
                        }
                    }
                    for child in rest {
                        self.block(child, l.tight);
                    }
                    // No cr() here: in a tight item the last paragraph's
                    // text keeps `</li>` on its line; every block emits its
                    // own line ending.
                    self.out.push_str("</li>\n");
                }
                self.out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
            }
            Block::Table(t) => {
                let Some((header, rows)) = t.children.split_first() else { return };
                self.cr();
                self.out.push_str("<table>\n<thead>\n<tr>\n");
                for (i, cell) in header.children.iter().enumerate() {
                    self.table_cell("th", t.align.get(i).copied(), Some(cell));
                }
                self.out.push_str("</tr>\n</thead>\n");
                if !rows.is_empty() {
                    self.out.push_str("<tbody>\n");
                    for row in rows {
                        self.out.push_str("<tr>\n");
                        for i in 0..header.children.len() {
                            self.table_cell("td", t.align.get(i).copied(), row.children.get(i));
                        }
                        self.out.push_str("</tr>\n");
                    }
                    self.out.push_str("</tbody>\n");
                }
                self.out.push_str("</table>\n");
            }
            Block::Definition(_) => {}
            // Extension constructs (footnotes, math, liquid, directives, MDX):
            // never in the spec suites, render nothing rather than crash.
            _ => {}
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text(t) => {
                    let raw = t.span.slice(self.source);
                    self.out.push_str(&esc(&decode::decode(raw, true)));
                }
                Inline::SoftBreak(_) => self.out.push('\n'),
                Inline::HardBreak(_) => self.out.push_str("<br />\n"),
                Inline::CodeSpan(c) => {
                    self.out.push_str("<code>");
                    let mut content =
                        Segment::join(self.source, &c.pieces).cow_replace('\n', " ").into_owned();
                    if self.in_table_cell {
                        content = content.cow_replace("\\|", "|").into_owned();
                    }
                    self.out.push_str(&esc(&content));
                    self.out.push_str("</code>");
                }
                Inline::HtmlInline(h) => {
                    self.out.push_str(&decode::html_inline(self.source, &h.pieces));
                }
                Inline::Autolink(a) => {
                    let inner = strip_delims(a.span.slice(self.source));
                    self.out.push_str("<a href=\"");
                    if a.email {
                        self.out.push_str("mailto:");
                    }
                    // Entities are not decoded in autolink URLs (micromark).
                    self.out.push_str(&encode_href(inner));
                    self.out.push_str("\">");
                    self.out.push_str(&esc(inner));
                    self.out.push_str("</a>");
                }
                Inline::AutolinkLiteral(a) => {
                    let raw = a.span.slice(self.source);
                    let prefix = match oxc_markdown_parser::literal_kind(raw) {
                        oxc_markdown_parser::LiteralKind::Http => "",
                        oxc_markdown_parser::LiteralKind::Www => "http://",
                        oxc_markdown_parser::LiteralKind::Email => "mailto:",
                    };
                    self.out.push_str("<a href=\"");
                    self.out.push_str(prefix);
                    self.out.push_str(&encode_href(raw));
                    self.out.push_str("\">");
                    self.out.push_str(&esc(raw));
                    self.out.push_str("</a>");
                }
                Inline::Emphasis(e) => {
                    self.out.push_str("<em>");
                    self.inlines(&e.children);
                    self.out.push_str("</em>");
                }
                Inline::Strong(s) => {
                    self.out.push_str("<strong>");
                    self.inlines(&s.children);
                    self.out.push_str("</strong>");
                }
                Inline::Strikethrough(s) => {
                    self.out.push_str("<del>");
                    self.inlines(&s.children);
                    self.out.push_str("</del>");
                }
                Inline::Link(l) => {
                    if let Some((href, title)) = self.resolve(&l.kind) {
                        self.out.push_str("<a href=\"");
                        self.out.push_str(&href);
                        self.out.push('"');
                        self.title_attr(title);
                        self.out.push('>');
                        self.inlines(&l.children);
                        self.out.push_str("</a>");
                    }
                }
                Inline::Image(i) => {
                    if let Some((src, title)) = self.resolve(&i.kind) {
                        self.out.push_str("<img src=\"");
                        self.out.push_str(&src);
                        self.out.push_str("\" alt=\"");
                        let mut alt = String::new();
                        self.alt_text(&i.children, &mut alt);
                        self.out.push_str(&alt);
                        self.out.push('"');
                        self.title_attr(title);
                        self.out.push_str(" />");
                    }
                }
                // Extension inlines (footnotes, math, wiki links, liquid, MDX): as above.
                _ => {}
            }
        }
    }

    /// ` title="…"`, when present. Shared by the link and image arms.
    fn title_attr(&mut self, title: Option<String>) {
        if let Some(title) = title {
            self.out.push_str(" title=\"");
            self.out.push_str(&title);
            self.out.push('"');
        }
    }

    /// Resolves a link/image to (href, title-attr value).
    fn resolve(&self, kind: &LinkKind) -> Option<(String, Option<String>)> {
        let (destination, title) = match kind {
            LinkKind::Inline { destination, title } => {
                (*destination, title.as_ref().map(|t| t.as_slice()))
            }
            LinkKind::Reference { label, .. } => {
                let text = Segment::join(self.source, label);
                let def = self.defs.get(label::normalize(&text).as_ref())?;
                (def.destination, def.title)
            }
        };
        let href = encode_href(&destination_text(self.source, &destination));
        let title = title.map(|pieces| esc(&decode::title(self.source, pieces)).into_owned());
        Some((href, title))
    }

    /// The image `alt` attribute: text/code escaped, raw HTML passed
    /// through verbatim, breaks as newlines.
    fn alt_text(&mut self, inlines: &[Inline], out: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Text(t) => {
                    out.push_str(&esc(&decode::decode(t.span.slice(self.source), true)));
                }
                Inline::SoftBreak(_) | Inline::HardBreak(_) => out.push('\n'),
                Inline::CodeSpan(c) => {
                    out.push_str(&esc(
                        &Segment::join(self.source, &c.pieces).cow_replace('\n', " ")
                    ));
                }
                Inline::HtmlInline(h) => out.push_str(&decode::html_inline(self.source, &h.pieces)),
                Inline::Emphasis(e) => self.alt_text(&e.children, out),
                Inline::Strong(s) => self.alt_text(&s.children, out),
                Inline::Strikethrough(s) => self.alt_text(&s.children, out),
                Inline::Link(l) => self.alt_text(&l.children, out),
                Inline::Image(i) => self.alt_text(&i.children, out),
                Inline::Autolink(a) => {
                    out.push_str(&esc(strip_delims(a.span.slice(self.source))));
                }
                _ => {}
            }
        }
    }

    fn table_cell(&mut self, tag: &str, align: Option<TableAlign>, cell: Option<&TableCell>) {
        self.out.push('<');
        self.out.push_str(tag);
        match align {
            Some(TableAlign::Left) => self.out.push_str(" align=\"left\""),
            Some(TableAlign::Right) => self.out.push_str(" align=\"right\""),
            Some(TableAlign::Center) => self.out.push_str(" align=\"center\""),
            Some(TableAlign::None) | None => {}
        }
        self.out.push('>');
        if let Some(cell) = cell {
            self.in_table_cell = true;
            self.inlines(&cell.children);
            self.in_table_cell = false;
        }
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    fn segment(&mut self, seg: &Segment, escape: bool) {
        for _ in 0..seg.padding {
            self.out.push(' ');
        }
        let raw = seg.span.slice(self.source);
        if escape {
            self.out.push_str(&esc(raw));
        } else {
            self.out.push_str(raw);
        }
    }
}

/// Strips the single-byte delimiters enclosing `s` (`<…>`, `"…"`, `(…)`).
pub(crate) fn strip_delims(s: &str) -> &str {
    &s[1..s.len() - 1]
}

/// A destination as mdast stores it: minus the angle brackets, decoded.
pub(crate) fn destination_text<'s>(source: &'s str, d: &Destination) -> Cow<'s, str> {
    let raw = d.span.slice(source);
    decode::decode(if d.angle_bracketed { strip_delims(raw) } else { raw }, true)
}

/// A fenced code info string split as micromark tokenizes it:
/// the language up to the first space/tab, the meta after the whitespace run.
/// Both raw; decode each separately, as micromark does.
pub(crate) fn split_info(info: &str) -> (&str, &str) {
    let (lang, rest) = info.split_once([' ', '\t']).unwrap_or((info, ""));
    (lang, rest.trim_start_matches([' ', '\t']))
}

pub(crate) fn ordered_start(marker: Span, source: &str) -> Option<u64> {
    let digits = marker.slice(source);
    digits[..digits.len().saturating_sub(1)].parse().ok()
}

fn esc(s: &str) -> Cow<'_, str> {
    // Borrow-through fast path, same idiom as the crate's `decode`: most
    // text has nothing to escape.
    if !s.contains(['&', '<', '>', '"']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// cmark's houdini href escaping: a safe-set of bytes stays literal, `&`
/// and `'` become entities, everything else is %-encoded per UTF-8 byte.
/// One micromark refinement: a stray `%` is itself %-encoded — "valid"
/// escapes are `%` plus two ASCII alphanumerics (micromark's
/// `normalizeUri`, looser than hex). cmark leaves `%` alone, but no spec
/// example exercises a stray `%` in an href, so this stays safe for the
/// cmark-oracle suites too.
fn encode_href(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'&' => out.push_str("&amp;"),
            b'%' => {
                let valid = bytes.get(i + 1).is_some_and(u8::is_ascii_alphanumeric)
                    && bytes.get(i + 2).is_some_and(u8::is_ascii_alphanumeric);
                out.push_str(if valid { "%" } else { "%25" });
            }
            _ if b.is_ascii_alphanumeric() || b"-_.+!*'(),#@?=;:/$~".contains(&b) => {
                out.push(b as char);
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(char::from(HEX[usize::from(b >> 4)]));
                out.push(char::from(HEX[usize::from(b & 0xF)]));
            }
        }
    }
    out
}
