//! The extension text constructs that resolve immediately, like code spans:
//! math (`$`), wiki links (`[[…]]`) and liquid (`{% %}` / `{{ }}`).

use crate::syntax;

use super::{PN, Tokenizer, matching_run};

impl Tokenizer<'_, '_> {
    /// `$$ … $$` math (micromark-extension-math; `$ … $` too with `math_text_single_dollar`):
    /// the opening run is maximal, the closing run must match it exactly,
    /// content may span line endings but not the end of input.
    /// Resolves immediately, like code spans, failed attempt consumes the whole run
    /// (mid-run `$`s can never open, matching micromark's previous-character gate).
    pub(super) fn math_span(&mut self) {
        let start = self.pos;
        let n = syntax::run_len(&self.t.as_bytes()[start..], b'$');
        let min = if self.constructs.math_text_single_dollar { 1 } else { 2 };
        let close = if n < min { None } else { matching_run(self.t.as_bytes(), start + n, b'$', n) };
        let Some(close) = close else {
            self.pos = start + n;
            return;
        };
        self.flush_text(start);
        self.nodes.push(PN::MathSpan(start..close + n));
        self.pos = close + n;
        self.text_start = self.pos;
    }

    /// `[[target]]` (alias-less braindb grammar): no line endings inside,
    /// the target needs at least one non-whitespace byte,
    /// and the first `]` must start the closing `]]`, anything else fails the construct.
    pub(super) fn try_wiki_link(&mut self) -> bool {
        if !self.constructs.wiki_link {
            return false;
        }
        let bytes = self.t.as_bytes();
        let start = self.pos;
        if bytes.get(start + 1) != Some(&b'[') || start + 2 < self.wiki_dead {
            return false;
        }
        let mut i = start + 2;
        let mut data = false;
        loop {
            match bytes.get(i) {
                None | Some(b'\n') => {
                    self.wiki_dead = i;
                    return false;
                }
                Some(b']') => {
                    if !(data && bytes.get(i + 1) == Some(&b']')) {
                        self.wiki_dead = i;
                        return false;
                    }
                    self.flush_text(start);
                    self.nodes.push(PN::WikiLink(start..i + 2));
                    self.pos = i + 2;
                    self.text_start = self.pos;
                    return true;
                }
                Some(&b) => {
                    data |= !matches!(b, b' ' | b'\t');
                    i += 1;
                }
            }
        }
    }

    /// `{% … %}` / `{{ … }}` in text:
    /// the closer is the first occurrence of the matching two-byte sequence (no nesting),
    /// may span line endings, fails at the end of input.
    pub(super) fn try_liquid(&mut self) -> bool {
        if !self.constructs.liquid {
            return false;
        }
        let start = self.pos;
        let Some(closer) = syntax::liquid::open(&self.t[start..]) else { return false };
        let exhausted = &mut self.liquid_exhausted[usize::from(closer == b'}')];
        if *exhausted {
            return false;
        }
        let Some(after) = syntax::liquid::close(&self.t[start + 2..], closer) else {
            // No closer of this kind exists from here on; every later attempt fails too
            *exhausted = true;
            return false;
        };
        let end = start + 2 + after;
        self.flush_text(start);
        self.nodes.push(PN::Liquid(start..end));
        self.pos = end;
        self.text_start = self.pos;
        true
    }
}
