# AGENTS.md - AI Assistant Guide for oxc-markdown-parser

A CommonMark + GFM (+ MDX v3 later, via construct toggles) parser producing a span-faithful, style-preserving typed AST.
It is the parsing layer for a Prettier-compatible markdown formatter.

## The one rule

Parse behavior targets micromark, except where micromark is plainly wrong.

micromark is what Prettier parses with; matching its structure is what makes the formatter's output comparable.
Its quirks are reproduced by default, spec prose notwithstanding: run `just differential` before "fixing" one, and pin the behavior with a fixture instead.

Whether a quirk is kept or dropped is decided by three questions, in order, plus two overrides;
every drop is an entry in `DIVERGENCES.md` naming the deciding question, with a reproducible example, exempted by shape in the differential runner.
Kept quirks that were weighed are listed there too, so the question is not reopened.

- 1: Is there a norm?
  - CommonMark and GFM have the spec and cmark.
  - A Prettier extension (liquid, wiki link, math, directive) has none;
    - its own definition is the norm: README grammar, tests, explicit intent in the code.
- 2: Does micromark deviate from it?
  - If not, keep; nothing to decide.
- 3: Is the deviation intent or accident?
   - Intent is backed by a test, a changelog entry, an issue, or explicit code (`interrupt ? ok`): keep.
   - Accident is a by-product of the machinery (UTF-16 code units, negative codes, a flag's lifetime, a missed branch) with an unambiguous tree under the norm: drop.

Overrides, decided before question 3:

- The difference never reaches Prettier's output?
  - follow the norm; mimicry buys nothing.
- Prettier's own output is broken there (not idempotent, or content changes)?
  - drop; nobody can depend on it.

When in doubt, keep: a drop needs grounds, a keep does not.

## Architecture

Two phases, three steps:

1. Block phase (`src/block/engine/`, `Engine`).
   cmark-style container-stack line loop; `engine/mod.rs` sequences the five per-line steps as one method each,
   `engine/open.rs` holds the block openers, `engine/state.rs` the open-container / leaf enums.
   `src/block/probe.rs` is the single table of block starts (priority-ordered, construct-gated);
   the open loop, the lazy-continuation test, the liquid interrupt check and the HTML quirk all consume it.
   `src/block/scan/` has one file per construct family (`commonmark`, `html`, `table`, `footnote`, `math`, `directive`) with the pure line scanners.
   A new block construct is one `Start` variant, one probe row, one opener in `open.rs`, one scanner.
   Extension constructs with their own continuation rules sit beside the loop: `engine/table.rs`, `engine/liquid.rs`.
   `src/block/lexical.rs` is the public line-start classification for formatters, a facade over `probe`.
   Output is an IR tree (`block/ir.rs`) whose leaves hold logical-line `Segment`s:
   container prefixes stripped, leading whitespace of continuation lines kept raw (code spans need it).
   Definitions are stripped at paragraph close (`block/refdef.rs`) and their normalized labels collected on the engine.
2. Build (`src/block/build.rs`).
   IR to arena AST; computes span-derived facts (list tightness, task checkboxes) and runs the inline phase per leaf.
3. Inline phase (`src/inline/`).
   micromark-shaped tokenizer over the joined logical text;
   `inline/input.rs` keeps an exact per-byte map back to source offsets.
   Code spans, autolinks and raw HTML resolve immediately;
   `*_~` runs and brackets are recorded, then `emphasis.rs` (delimiter stack) and `link.rs` (bracket stack) restructure the flat node list.

Shared grammar lives once, in `src/syntax/`:
- `link_target.rs`: destination / title / label scanning for refdefs and inline links
- `liquid.rs`: `{% %}` / `{{ }}` delimiters for the flow and text constructs
- `html.rs::tag_end`: the tag grammar past the name (attributes, `>`), block + inline tags
- `unicode.rs`: whitespace / punctuation classes (flanking, directive names)
- `decode.rs` (+ `entities.rs`): escapes + entities, public
- `label.rs`: label normalization, public

## Invariants

- Spans are original-source offsets, via exact per-byte maps, never arithmetic over rebuilt buffers.
- No cooked values in the AST.
  Consumers slice raw source through spans; `decode` / `label` are the shared cookers.
  Style facts (markers, fence chars, break kinds, reference kinds, lazy lines) are first-class fields.
- Formatter policy stays out (sentence splitting, CJK classification, alignment).
  The AST records lexical facts only, e.g. `Text::contains_cjk`.
- MD and MDX are one parser.
  Dialects are `Constructs` toggles (`src/options.rs`); the `<` / `{` / 4-space decision points are the MDX hook sites.
  The API is always `(ast, Vec<Diagnostic>)`; markdown mode is structurally diagnostic-free.

## Testing

| Command | Covers |
| --- | --- |
| `cargo test` | unit tests, AST snapshots (`tests/ast.rs`), span invariants (`tests/spans.rs`) |
| `cargo insta review` | accept snapshot changes after an intentional AST change |
| `just conformance` | CommonMark spec.json + GFM spec.txt, HTML output, snapshot-pinned |
| `cargo run -p conformance -- <N>` | one example (`g<N>` for GFM) |
| `just differential [count] [seed]` | structural fuzz against mdast (micromark in the Prettier composition) |
| `just fuzz-panic` | multibyte / boundary char soups, any panic fails |

First run: `npm i` in `tasks/differential`; `just conformance` fetches its suite files itself.

- The HTML suites never see spans, segment padding, lazy lines or reference kinds.
  `tests/fixtures/**/*.md` pins that layer: small per-construct files (`block/`, `inline/`, `hazards/`), one compact tree each.
  Add a fixture when a change touches spans or style facts; do not copy the spec suite.
- `tests/common/mod.rs` destructures every node exhaustively,
  so a new AST field fails to compile there until it is classified.
- The conformance pin is bidirectional:
  a newly passing example also fails the run until the snapshot is committed.
- The differential runner compares an mdast-shaped signature of both trees (`tasks/render/src/sig.rs`, `tasks/differential/mdast.mjs`).
  Outside the signature, so not under test:
  a directive fence's `[label]` (a paragraph child to mdast, part of our verbatim fence line),
  and the line after a task-list checkbox with nothing else on it (mdast keeps all but one character of that whitespace as text; our paragraph starts at the content, so a soft break there is ours alone).
  It exempts mismatches by input shape (`QUIRKS` in `run.ts`, one per `DIVERGENCES.md` section).
- The oracle mirrors Prettier's parser composition (`mdast.mjs`);
  the two pieces Prettier does not publish (`liquid.mjs`, `html-text.mjs`) are vendored verbatim under a header naming the Prettier commit.
  Neither check runs in CI; both are for bumping Prettier, by hand:
  `just vendor-check` diffs them against that commit, `just vendor-drift` diffs the commit against Prettier `main` for those files and the composition.
  Bumping means re-copying the file, updating the header, and re-reading `parse-markdown.js` for composition changes.
  The oracle's npm versions are pinned exactly to Prettier's `package.json` at that commit and excluded from Renovate; bump them by hand in the same step.
  Shapes exempt, they do not verify: watch the per-quirk counters; a regression on a quirk-shaped input hides there.
