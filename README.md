# oxc-markdown-parser

A CommonMark + GFM (and eventually MDX v3) parser that produces a span-faithful,
style-preserving typed AST, designed for building formatters.

- Parse behavior targets micromark (= CommonMark + GFM as GitHub renders it).
- Style facts (markers, fence chars, reference kinds, …) are first-class AST
  fields, so a printer never has to re-derive them by peeking at the source.
- MD and MDX are the same parser with construct toggles; the API always returns
  `(ast, diagnostics)` and diagnostics are structurally empty in MD mode.
