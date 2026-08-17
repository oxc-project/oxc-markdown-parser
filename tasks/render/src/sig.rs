//! mdast-shaped structural signature of the AST: our side of the differential
//! fuzz against `mdast-util-from-markdown` (`tasks/differential/mdast.mjs`
//! prints the same shape from micromark's tree).
//!
//! Compared: block structure, verbatim values (code, HTML, math, liquid),
//! list facts, definition/link targets, table alignment, the inline nesting
//! of non-text nodes, and text runs as mdast merges them (decoded, soft
//! breaks as `\n`, adjacent runs joined).
//! Values are cooked the way mdast cooks them, through the crate's own
//! cookers (`decode`, `label`) so the fuzz pins the rules a formatter uses.
use std::borrow::Cow;
use std::fmt::Write;

use oxc_markdown_parser::ast::*;
use oxc_markdown_parser::decode::{self, decode};
use oxc_markdown_parser::{Segment, Span, label};

use crate::{destination_text, ordered_start, split_info, strip_delims};

pub fn signature(source: &str, root: &Root<'_>) -> String {
    let mut s = Sig {
        source,
        out: String::with_capacity(source.len() * 2),
        text: String::new(),
        skip_leading_break: false,
    };
    s.blocks(&root.children);
    s.out
}

struct Sig<'s> {
    source: &'s str,
    out: String,
    /// Scratch for the text run being merged; empty between `inlines` calls.
    text: String,
    /// Armed for a task-list item's content: its first paragraph drops a leading soft break
    /// (see `ListItem::checkbox`). Consumed by the next `inlines` call whatever it holds.
    skip_leading_break: bool,
}

impl<'s> Sig<'s> {
    fn slice(&self, span: Span) -> &'s str {
        span.slice(self.source)
    }

    fn join(&self, pieces: &[Segment]) -> Cow<'s, str> {
        Segment::join(self.source, pieces)
    }

    fn title(&self, title: Option<&[Segment]>) -> String {
        title.map(|t| decode::title(self.source, t)).unwrap_or_default()
    }

    fn blocks(&mut self, blocks: &[Block<'_>]) {
        self.out.push('[');
        for b in blocks {
            self.block(b);
            self.out.push(',');
        }
        self.out.push(']');
    }

    fn inlines(&mut self, inlines: &[Inline<'_>]) {
        self.out.push('(');
        let skip = std::mem::take(&mut self.skip_leading_break);
        let inlines = match inlines {
            [Inline::SoftBreak(_), rest @ ..] if skip => rest,
            _ => inlines,
        };
        for i in inlines {
            match i {
                Inline::Text(t) => {
                    let decoded = decode(self.slice(t.span), true);
                    self.text.push_str(&decoded);
                }
                Inline::SoftBreak(_) => self.text.push('\n'),
                other => {
                    self.flush_text();
                    self.inline(other);
                }
            }
        }
        self.flush_text();
        self.out.push(')');
    }

    /// Emits the merged text run, if any. Always called before recursing,
    /// so the scratch buffer is empty whenever a nested `inlines` starts.
    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let _ = write!(self.out, "t{{{}}}", q(&self.text));
            self.text.clear();
        }
    }

    /// mdast's `image.alt`: the label's text content (`mdast-util-to-string` over the children).
    /// Prettier prints alt from the source, so this stays an oracle-only cook.
    fn alt(&self, inlines: &[Inline<'_>], out: &mut String) {
        for i in inlines {
            match i {
                Inline::Text(t) => out.push_str(&decode(self.slice(t.span), true)),
                Inline::SoftBreak(_) => out.push('\n'),
                Inline::Emphasis(e) => self.alt(&e.children, out),
                Inline::Strong(e) => self.alt(&e.children, out),
                Inline::Strikethrough(e) => self.alt(&e.children, out),
                Inline::Link(l) => self.alt(&l.children, out),
                Inline::Image(i) => self.alt(&i.children, out),
                Inline::CodeSpan(c) => out.push_str(&self.join(&c.pieces)),
                Inline::HtmlInline(h) => out.push_str(&self.join(&h.pieces)),
                Inline::Liquid(l) => out.push_str(&self.join(&l.pieces)),
                // `$…$` minus its delimiters (the value mdast-util-math stores);
                // the run length is not recorded, so this trims every `$`
                Inline::MathSpan(m) => {
                    let joined = self.join(&m.pieces);
                    out.push_str(joined.trim_matches('$'));
                }
                // A link's text content is its label; an autolink's is the address,
                // a wiki link's its target (`[[target]]`)
                Inline::Autolink(a) => out.push_str(strip_delims(self.slice(a.span))),
                Inline::AutolinkLiteral(a) => out.push_str(self.slice(a.span)),
                Inline::WikiLink(w) => {
                    let raw = self.slice(w.span);
                    out.push_str(&raw[2..raw.len() - 2]);
                }
                // No text content: a break, a footnote call, MDX
                Inline::HardBreak(_)
                | Inline::FootnoteReference(_)
                | Inline::MdxExpression(_)
                | Inline::MdxJsx(_) => {}
            }
        }
    }

    fn block(&mut self, block: &Block<'_>) {
        match block {
            Block::Paragraph(p) => {
                self.out.push('p');
                self.inlines(&p.children);
            }
            Block::Heading(h) => {
                let _ = write!(self.out, "h{}", h.level);
                self.inlines(&h.children);
            }
            Block::ThematicBreak(_) => self.out.push_str("hr"),
            Block::CodeBlock(c) => {
                let (lang, meta) = match c.kind {
                    CodeBlockKind::Fenced { info: Some(info), .. } => split_info(self.slice(info)),
                    _ => ("", ""),
                };
                let (lang, meta) = (decode(lang, true), decode(meta, true));
                let value = self.join(&c.lines);
                let _ = write!(self.out, "code{{{}}}{{{}}}{{{}}}", q(&lang), q(&meta), q(&value));
            }
            Block::HtmlBlock(h) => {
                let mut value = self.join(&h.lines).into_owned();
                if h.trailing_newline {
                    value.push('\n');
                }
                let _ = write!(self.out, "html{{{}}}", q(&value));
            }
            Block::Blockquote(b) => {
                self.out.push_str("bq");
                self.blocks(&b.children);
            }
            Block::List(l) => {
                let ordered = matches!(l.marker, ListMarker::Ordered { .. });
                let start = l
                    .children
                    .first()
                    .filter(|_| ordered)
                    .and_then(|item| ordered_start(item.marker, self.source))
                    .map_or(String::new(), |n| n.to_string());
                let _ = write!(self.out, "list{{{ordered},{start},spread={}}}[", !l.tight);
                for item in &l.children {
                    let checked = match item.checkbox {
                        Some(TaskCheckbox { checked: true, .. }) => "x",
                        Some(_) => "o",
                        None => "",
                    };
                    let _ = write!(self.out, "li{{{checked},spread={}}}", item.spread);
                    // A soft break right after the checkbox is ours alone (`ListItem::checkbox`)
                    self.skip_leading_break = item.checkbox.is_some();
                    self.blocks(&item.children);
                    self.out.push(',');
                }
                self.out.push(']');
            }
            Block::Definition(d) => {
                let label = label::normalize(&self.join(&d.label)).into_owned();
                let url = destination_text(self.source, &d.destination);
                let title = self.title(d.title.as_deref().map(|v| &**v));
                let _ = write!(self.out, "def{{{},{},{}}}", q(&label), q(&url), q(&title));
            }
            Block::Table(t) => {
                self.out.push_str("table{");
                for a in &t.align {
                    self.out.push(match a {
                        TableAlign::None => '-',
                        TableAlign::Left => 'l',
                        TableAlign::Right => 'r',
                        TableAlign::Center => 'c',
                    });
                }
                self.out.push_str("}[");
                for row in &t.children {
                    self.out.push_str("tr[");
                    for cell in &row.children {
                        self.out.push_str("td");
                        self.inlines(&cell.children);
                        self.out.push(',');
                    }
                    self.out.push_str("],");
                }
                self.out.push(']');
            }
            Block::FootnoteDefinition(f) => {
                let label = label::normalize(self.slice(f.label));
                let _ = write!(self.out, "fn{{{}}}", q(&label));
                self.blocks(&f.children);
            }
            Block::MathBlock(m) => {
                let meta = m.meta.map(|s| decode(self.slice(s), true)).unwrap_or_default();
                let value = self.join(&m.lines);
                let _ = write!(self.out, "math{{{}}}{{{}}}", q(&meta), q(&value));
            }
            Block::Liquid(l) => {
                let value = self.join(&l.pieces);
                let _ = write!(self.out, "liquid{{{}}}", q(&value));
            }
            Block::ContainerDirective(d) => {
                self.out.push_str("dir");
                self.blocks(&d.children);
            }
            Block::MdxEsm(_) | Block::MdxExpression(_) | Block::MdxJsx(_) => {
                self.out.push_str("mdx")
            }
        }
    }

    fn inline(&mut self, inline: &Inline<'_>) {
        match inline {
            // Merged into text runs by `inlines`; never reaches here
            Inline::Text(_) | Inline::SoftBreak(_) => {}
            Inline::Emphasis(e) => {
                self.out.push_str("em");
                self.inlines(&e.children);
            }
            Inline::Strong(e) => {
                self.out.push_str("strong");
                self.inlines(&e.children);
            }
            Inline::Strikethrough(e) => {
                self.out.push_str("del");
                self.inlines(&e.children);
            }
            Inline::CodeSpan(c) => {
                let value = self.join(&c.pieces);
                let _ = write!(self.out, "code{{{}}}", q(&value));
            }
            Inline::Link(l) => {
                self.link("a", &l.kind);
                self.inlines(&l.children);
            }
            // mdast images carry `alt` as a string, not children
            Inline::Image(i) => {
                self.link("img", &i.kind);
                let mut alt = String::new();
                self.alt(&i.children, &mut alt);
                let _ = write!(self.out, "{{{}}}", q(&alt));
            }
            // Raw text on both sides: `<…>` or a GFM literal, and the literal's extent
            Inline::Autolink(_) | Inline::AutolinkLiteral(_) => {
                let _ = write!(self.out, "auto{{{}}}", q(self.slice(inline.span())));
            }
            Inline::HtmlInline(h) => {
                let value = self.join(&h.pieces);
                let _ = write!(self.out, "ihtml{{{}}}", q(&value));
            }
            Inline::HardBreak(_) => self.out.push_str("br"),
            Inline::FootnoteReference(f) => {
                let label = label::normalize(self.slice(f.label));
                let _ = write!(self.out, "fnref{{{}}}", q(&label));
            }
            Inline::MathSpan(_) => self.out.push_str("imath"),
            Inline::WikiLink(_) => self.out.push_str("wiki"),
            Inline::Liquid(l) => {
                let value = self.join(&l.pieces);
                let _ = write!(self.out, "liquid{{{}}}", q(&value));
            }
            Inline::MdxExpression(_) | Inline::MdxJsx(_) => self.out.push_str("mdx"),
        }
    }

    fn link(&mut self, tag: &str, kind: &LinkKind<'_>) {
        match kind {
            LinkKind::Inline { destination, title } => {
                let url = destination_text(self.source, destination);
                let title = self.title(title.as_deref().map(|v| &**v));
                let _ = write!(self.out, "{tag}{{{},{}}}", q(&url), q(&title));
            }
            LinkKind::Reference { kind, label } => {
                let kind = match kind {
                    ReferenceKind::Full => "full",
                    ReferenceKind::Collapsed => "collapsed",
                    ReferenceKind::Shortcut => "shortcut",
                };
                let label = label::normalize(&self.join(label)).into_owned();
                let _ = write!(self.out, "{tag}ref{{{kind},{}}}", q(&label));
            }
        }
    }
}

/// JSON-quoted, so values with delimiters or line endings stay unambiguous.
fn q(s: &str) -> String {
    serde_json::to_string(s).expect("strings always serialize")
}
