//! AST snapshots: one compact tree per fixture, spans and style facts included.
//!
//! The HTML-based suites (conformance, differential) never see spans, segment
//! padding, lazy lines or reference kinds; this is where those are pinned.
//! Review with `cargo insta review` after an intentional AST change.

mod common;

use std::{fmt::Write as _, fs};

use common::{Fact, Visitor, walk_root};
use insta::{Settings, assert_snapshot, glob};
use oxc_markdown_parser::{Allocator, Parser, Span};

struct Printer<'s> {
    source: &'s str,
    out: String,
    depth: usize,
}

fn offsets(span: Span) -> String {
    format!("[{}..{}]", span.start, span.end)
}

impl Printer<'_> {
    /// `[a..b] "slice"`: the slice makes offsets reviewable.
    fn quoted(&self, span: Span) -> String {
        format!("{} {:?}", offsets(span), span.slice(self.source))
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }
}

impl Visitor for Printer<'_> {
    fn enter(&mut self, name: &'static str, span: Span, facts: Vec<Fact<'_>>) {
        // Leaves whose meaning is their text print the slice on the node line
        let mut head = match name {
            "Text" | "HardBreak" | "ThematicBreak" | "Autolink" | "AutolinkLiteral"
            | "WikiLink" | "FootnoteReference" => format!("{name} {}", self.quoted(span)),
            _ => format!("{name} {}", offsets(span)),
        };
        let mut lists: Vec<String> = vec![];
        for fact in facts {
            match fact {
                Fact::Str(key, value) if value.is_empty() => write!(head, " {key}").unwrap(),
                Fact::Str(key, value) => write!(head, " {key}={value}").unwrap(),
                Fact::Span(key, s) => write!(head, " {key}={}", self.quoted(s)).unwrap(),
                Fact::Spans(key, spans) => {
                    if !spans.is_empty() {
                        let items: Vec<_> = spans.iter().map(|s| self.quoted(*s)).collect();
                        write!(head, " {key}=[{}]", items.join(", ")).unwrap();
                    }
                }
                Fact::Segments(key, segments) => {
                    for (i, seg) in segments.iter().enumerate() {
                        let pad = if seg.padding > 0 {
                            format!(" pad={}", seg.padding)
                        } else {
                            String::new()
                        };
                        lists.push(format!("{key}[{i}] {}{pad}", self.quoted(seg.span)));
                    }
                }
            }
        }
        self.line(&head);
        self.depth += 1;
        for item in lists {
            self.line(&item);
        }
    }

    fn exit(&mut self) {
        self.depth -= 1;
    }
}

#[test]
fn ast_snapshots() {
    glob!("fixtures/**/*.md", |path| {
        let source = fs::read_to_string(path).unwrap();
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, &source).parse();
        let mut printer = Printer { source: &source, out: String::new(), depth: 0 };
        walk_root(&ret.root, &mut printer);
        if !ret.blanks.is_empty() {
            let blanks: Vec<_> = ret.blanks.iter().map(|s| offsets(*s)).collect();
            printer.line(&format!("blanks: {}", blanks.join(" ")));
        }
        for diagnostic in &ret.diagnostics {
            printer.line(&format!("diagnostic: {diagnostic}"));
        }

        let file_name = path.file_name().unwrap().to_str().unwrap();
        let mut settings = Settings::clone_current();
        settings.set_snapshot_path(path.parent().unwrap());
        settings.remove_snapshot_suffix();
        settings.set_prepend_module_to_snapshot(false);
        settings.remove_input_file();
        settings.set_omit_expression(true);
        settings.bind(|| {
            assert_snapshot!(file_name.to_string(), printer.out);
        });
    });
}
