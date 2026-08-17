//! Span invariants, checked over the fixtures and a deterministic token soup:
//!
//! - every span is in bounds and on char boundaries
//! - a node's children, extra spans and segments lie inside its span
//! - siblings are in source order and don't overlap
//! - segments (block lines, inline pieces) never contain a line ending
//! - `Text` is never empty; `blanks` is sorted and whitespace-only
//!
//! An off-by-one in any span map shows up here as a class, not a fixture.
//! (Sibling order is also a `debug_assert!` at the two push sites in the
//! parser, so the fuzz runners catch it without this harness.)

mod common;

use common::{Fact, Visitor, walk_root};
use oxc_markdown_parser::{Allocator, Parser, Span};

struct Open {
    name: &'static str,
    span: Span,
    /// End of the last child seen, for sibling order.
    last_end: u32,
}

struct Checker<'s> {
    source: &'s str,
    stack: Vec<Open>,
    errors: Vec<String>,
}

impl Checker<'_> {
    fn err(&mut self, msg: &str) {
        let path: Vec<_> = self.stack.iter().map(|o| o.name).collect();
        self.errors.push(format!("{msg} (in {})", path.join(" > ")));
    }

    /// In bounds, on char boundaries, and inside `outer` when given.
    fn contained(&mut self, what: &str, inner: Span, outer: Option<Span>) -> bool {
        let (s, e) = (inner.start as usize, inner.end as usize);
        if s > e || e > self.source.len() {
            self.err(&format!("{what} {inner:?} out of bounds"));
            return false;
        }
        if !self.source.is_char_boundary(s) || !self.source.is_char_boundary(e) {
            self.err(&format!("{what} {inner:?} splits a character"));
            return false;
        }
        if let Some(outer) = outer
            && (inner.start < outer.start || inner.end > outer.end)
        {
            self.err(&format!("{what} {inner:?} outside {outer:?}"));
            return false;
        }
        true
    }
}

impl Visitor for Checker<'_> {
    fn enter(&mut self, name: &'static str, span: Span, facts: Vec<Fact<'_>>) {
        let parent = self.stack.last().map(|o| o.span);
        if self.contained(name, span, parent) {
            if let Some(last_end) = self.stack.last().map(|o| o.last_end)
                && span.start < last_end
            {
                self.err(&format!(
                    "{name} {span:?} starts before its previous sibling ends at {last_end}"
                ));
            }
            if name == "Text" && span.is_empty() {
                self.err(&format!("empty Text at {}", span.start));
            }
        }
        if let Some(top) = self.stack.last_mut() {
            top.last_end = top.last_end.max(span.end);
        }
        for fact in facts {
            match fact {
                Fact::Span(key, s) => {
                    self.contained(key, s, Some(span));
                }
                Fact::Spans(key, spans) => {
                    for s in spans {
                        self.contained(key, *s, Some(span));
                    }
                }
                Fact::Segments(key, segments) => {
                    for seg in segments {
                        if self.contained(key, seg.span, Some(span))
                            && seg.span.slice(self.source).contains(['\n', '\r'])
                        {
                            self.err(&format!("{key} {:?} spans a line ending", seg.span));
                        }
                    }
                }
                Fact::Str(..) => {}
            }
        }
        self.stack.push(Open { name, span, last_end: span.start });
    }

    fn exit(&mut self) {
        self.stack.pop();
    }
}

fn assert_invariants(source: &str) {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source).parse();
    let mut checker = Checker { source, stack: vec![], errors: vec![] };
    walk_root(&ret.root, &mut checker);
    let mut prev = 0;
    for blank in &ret.blanks {
        if checker.contained("blank", *blank, None) {
            if blank.start < prev {
                checker.err(&format!("blank {blank:?} out of order"));
            }
            if !blank.slice(source).bytes().all(|b| b == b' ' || b == b'\t') {
                checker.err(&format!("blank {blank:?} is not whitespace"));
            }
            prev = blank.end;
        }
    }
    assert!(checker.errors.is_empty(), "{source:?}\n{}", checker.errors.join("\n"));
}

#[test]
fn fixtures() {
    insta::glob!("fixtures/**/*.md", |path| {
        assert_invariants(&std::fs::read_to_string(path).unwrap());
    });
}

/// mulberry32, the differential runner's PRNG.
fn rng(state: &mut u32) -> u32 {
    *state = state.wrapping_add(0x6D2B_79F5);
    let mut t = *state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3D));
    t ^ (t >> 14)
}

/// A subset of the differential runner's token alphabet:
/// every construct, plus the byte/column hazards (tabs, CRLF, multibyte, CJK).
const TOKENS: &[&str] = &[
    "*",
    "**",
    "_",
    "__",
    "`",
    "``",
    "[",
    "]",
    "(",
    ")",
    "<",
    ">",
    "#",
    "##",
    "-",
    "+",
    "1.",
    "2)",
    "\\",
    "&",
    "&amp;",
    "!",
    "\"",
    "'",
    "=",
    "~",
    "|",
    ":",
    "a",
    "foo",
    "bar b",
    "é",
    "漢字",
    "http://x.y",
    "user@e.com",
    "\\*",
    " ",
    "  ",
    "\t",
    "\n",
    "\n\n",
    "  \n",
    "\\\n",
    "```\n",
    "---",
    "===",
    "***",
    "    ",
    "[x]: /u",
    "[x]",
    "\"t\"",
    "<div>",
    "</div>",
    "<!-- c -->",
    "![",
    "](u)",
    "<a>",
    "</span>",
    "~~",
    "[^1]",
    "[^1]: n",
    "- [x] ",
    "- [ ] ",
    "| a | b |",
    "| - | - |",
    ":-:",
    "www.a.com",
    "$",
    "$$",
    "$$\n",
    "$x$",
    "{%",
    "%}",
    "{{",
    "}}",
    "{{ v }}",
    "{% t %}",
    "[[",
    "]]",
    "[[w]]",
    ":::",
    ":::note\n",
    "::::a\n",
    "\r\n",
    "> x\n<a>\n",
    "\u{3000}",
    "\u{2E53}",
];

#[test]
fn token_soup() {
    let mut state = 0u32;
    for _ in 0..3000 {
        let len = 2 + (rng(&mut state) % 39) as usize;
        let mut source = String::new();
        for _ in 0..len {
            source.push_str(TOKENS[(rng(&mut state) as usize) % TOKENS.len()]);
        }
        assert_invariants(&source);
    }
}
