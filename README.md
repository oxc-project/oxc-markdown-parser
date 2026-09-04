# oxc-markdown-parser

`oxc-markdown-parser` parses CommonMark + GFM (and eventually MDX v3) into a span-faithful, style-preserving typed AST, designed for building formatters.

- Parse behavior targets [micromark](https://github.com/micromark/micromark), the parser behind Prettier's markdown support (= CommonMark + GFM as GitHub renders it).
  Deliberate differences are listed in [DIVERGENCES.md](./DIVERGENCES.md).
- Style facts (markers, fence chars, reference kinds, …) are first-class AST fields, so a printer never re-derives them by peeking at the source.
- Values are not cooked: consumers slice the original source through spans.
- MD and MDX are the same parser with construct toggles (`Constructs`); the API always returns `(root, diagnostics)`, and diagnostics are structurally empty in MD mode.
- Prettier's extensions (math, liquid, wiki links) and `:::` container directives are supported and enabled by default.

## Example

```rust
use oxc_markdown_parser::{Allocator, Parser};

let allocator = Allocator::default();
let parser = Parser::new(&allocator, "# Hello\n");
let ret = parser.parse();
println!("{:#?}", ret.root);
assert!(ret.diagnostics.is_empty());
```

More examples are available in [`examples`](./examples).

## Conformance

`just conformance` renders two suites to HTML and snapshots the results under [`tasks/conformance/snapshots`](./tasks/conformance/snapshots):

- the [CommonMark spec](https://spec.commonmark.org/) examples (`spec.json`),
- the [GFM spec](https://github.github.com/gfm/) extension sections (`spec.txt` from cmark-gfm).

The pinned suite files are fetched on first run.
All CommonMark examples pass; the remaining GFM differences are genuine micromark-vs-cmark-gfm disagreements, pinned in the snapshot.

## Differential testing

`just differential` fuzzes the parser against [mdast-util-from-markdown](https://github.com/syntax-tree/mdast-util-from-markdown) (micromark in Prettier's composition) and compares a structural signature of both trees.
`just fuzz-panic` stresses byte/char boundaries; any panic fails.

Both need Node >= 22.18 and `npm install` in `tasks/differential` once.

## License

MIT

# [Sponsored By](https://oxc.rs/sponsor)

<p align="center">
  <a href="https://oxc.rs/sponsor">
    <img src="https://raw.githubusercontent.com/oxc-project/sponsors/main/sponsors.svg" alt="Our sponsors" />
  </a>
</p>
