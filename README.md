# oxc-markdown-parser

`oxc-markdown-parser` parses CommonMark + GFM (and eventually MDX v3) into a span-faithful, style-preserving typed AST, designed for building formatters.
Prettier's extensions (math, liquid, wiki links) and `:::` container directives are enabled by default.

How it parses, and why, is in [AGENTS.md](https://github.com/oxc-project/oxc-markdown-parser/blob/main/AGENTS.md).
Deliberate differences from micromark are listed in [DIVERGENCES.md](https://github.com/oxc-project/oxc-markdown-parser/blob/main/DIVERGENCES.md).

## Example

```rust
use oxc_markdown_parser::{Allocator, Parser};

let allocator = Allocator::default();
let parser = Parser::new(&allocator, "# Hello\n");
let ret = parser.parse();
println!("{:#?}", ret.root);
assert!(ret.diagnostics.is_empty());
```

More examples are available in [`examples`](https://github.com/oxc-project/oxc-markdown-parser/tree/main/examples).

## License

MIT

# [Sponsored By](https://oxc.rs/sponsor)

<p align="center">
  <a href="https://oxc.rs/sponsor">
    <img src="https://raw.githubusercontent.com/oxc-project/sponsors/main/sponsors.svg" alt="Our sponsors" />
  </a>
</p>
