# Divergences from micromark

micromark is the oracle, as `mdast-util-from-markdown` exposes it (the tree Prettier consumes).
Each entry below is a place where we deliberately do not match it, with the input that shows it.
`decided by:` is the question from the rule in `AGENTS.md` that settled it
(norm, deviation, intent-or-accident, or one of the two overrides);
`runner:` is the `QUIRKS` name in `tasks/differential/run.ts` that exempts the shape.
The kept quirks that were weighed the same way close the file.

Inputs are markdown blocks (fenced with `~~~~` so inner backtick fences survive);
outputs are shown as HTML for readability, with `<!-- micromark -->` / `<!-- ours -->` labels
(the runner itself compares tree signatures).
Inputs are literal and copy-pasteable; where trailing spaces, tabs or a final line ending matter, the prose says so, since an editor that trims whitespace would hide it.

micromark's HTML compiler adds line-ending quirks of its own (dropped or glued line endings around blocks inside list items and footnote definitions);
those never reach the tree and are not listed here.

## Trailing deep-blank lines of indented code

The spec drops trailing blank lines of indented code;
micromark keeps a whitespace-only line of four-plus columns as content.
Prettier prints that line, then trims its trailing whitespace, so it is not idempotent there; we keep the spec.
The second input line is six spaces.
`decided by: override, Prettier's output is not idempotent`
`runner: deep-blank after indented code`

~~~~markdown
    a
      
b
~~~~

```html
<!-- micromark: two spaces on the blank line -->
<pre><code>a
  
</code></pre>
<!-- commonmark.js, ours -->
<pre><code>a
</code></pre>
```

## Indented code split after a container closes

An indented code block opened on the line that closes a blockquote or list item by container mismatch
ends after that one line in micromark; the next indented line opens a second code block.
The spec (and cmark) keep one block, and so do we.
Prettier prints the two blocks with a blank line between, which re-parses as a single block with a blank line inside:
the content changes.
Unreported upstream.
`decided by: override, Prettier's output changes the content`
`runner: indented code split after container close`

~~~~markdown
>
    a
    b
~~~~

```html
<!-- micromark -->
<blockquote>
</blockquote>
<pre><code>a
</code></pre>
<pre><code>b
</code></pre>
<!-- cmark, ours -->
<blockquote>
</blockquote>
<pre><code>a
b
</code></pre>
```

## Fenced code meta keeps trailing whitespace

micromark's `codeFencedFenceMeta` token runs to the line ending, trailing spaces and tabs included,
so `code.meta` ends with them; our info span ends at the last non-blank character.
Math fences (`$$ meta `) behave the same on both sides.
Prettier prints `lang meta` and trims the line, so the difference never reaches its output.
The input's fence line ends with two spaces.
`decided by: override, never reaches Prettier's output`
`runner: fence meta trailing whitespace`

~~~~markdown
```a b  
x
```
~~~~

```html
<!-- micromark: meta "b  " -->
<pre><code class="language-a">x
</code></pre>
<!-- ours: meta "b" -->
<pre><code class="language-a">x
</code></pre>
```

## Astral punctuation

micromark classifies UTF-16 code units, so an astral character (an emoji, a supplementary-plane symbol) is a lone surrogate to its flanking rules: never punctuation.
The spec's flanking rules classify Unicode punctuation and symbols (categories P and S), astral planes included; we follow the spec (`syntax/unicode.rs`).
Prettier prints the emphasis micromark finds; the fixed tree is the spec's.
Unreported upstream.
`decided by: accident (UTF-16 code units)`
`runner: astral punctuation`

~~~~markdown
a*😀*b
~~~~

```html
<!-- micromark -->
<p>a<em>😀</em>b</p>
<!-- cmark, ours -->
<p>a*😀*b</p>
```

## List after indented code

An indented code block ends at the first non-indented line; nothing on that line interrupts anything.
micromark applies the paragraph-interrupt restriction to a list marker there (non-empty, ordered starts at 1), so `2. x` stays a paragraph.
Prettier prints the paragraph; the fixed tree is the spec's (`§5.2`, the restriction applies only when a paragraph is interrupted).
Unreported upstream.
`decided by: accident (the interrupt flag is set for indented code too)`
`runner: list after indented code`

~~~~markdown
    code
2. x
~~~~

```html
<!-- micromark -->
<pre><code>code
</code></pre>
<p>2. x</p>
<!-- cmark, ours -->
<pre><code>code
</code></pre>
<ol start="2">
<li>x</li>
</ol>
```

## List after a container opened on the same line

A blockquote (or list item) opening on a line closes the paragraph before it; inside the new container no paragraph is open, so a list marker there is a plain list start.
micromark's interrupt flag is line-level: it still restricts the marker after the container opened, so `2. b` stays paragraph text.
Prettier prints the paragraph; the fixed tree is the spec's.
Unreported upstream.
`decided by: accident (the interrupt flag lives for the whole line)`
`runner: list after a container opened on the same line`

~~~~markdown
a
> 2. b
~~~~

```html
<!-- micromark -->
<p>a</p>
<blockquote>
<p>2. b</p>
</blockquote>
<!-- cmark, ours -->
<p>a</p>
<blockquote>
<ol start="2">
<li>b</li>
</ol>
</blockquote>
```

## Failed liquid flow attempt poisons the interrupt flag

Prettier's own `micromark-extension-liquid` (vendored here).
A `{%` / `{{` line whose flow attempt fails leaves micromark's interrupt bookkeeping set,
so after the next leaf construct the following line is still parsed under the paragraph-interrupt restrictions.
Prettier 3.9+ misparses these the same way; we parse the line as the block start it is.
Prettier prints `2)` as paragraph text.
The `2)` line ends with two spaces.
`decided by: accident (the interrupt flag outlives the failed attempt); the extension is the norm`
`runner: liquid interrupt poisoning`

~~~~markdown
{%
<!-->
2)  
~~~~

```html
<!-- micromark -->
<p>{%</p>
<!-->
<p>2)</p>
<!-- ours -->
<p>{%</p>
<!-->
<ol start="2">
<li></li>
</ol>
```

## Directive names accept tabs and astral characters

`micromark-extension-directive` tests tabs (a negative code in micromark) with Unicode predicates guarded by `code > -1`,
so a tab counts as a name character; an astral character is a surrogate to the same predicates, so `:::😀` is a directive too.
We treat the tab as whitespace and the astral symbol as punctuation, per the extension's documented grammar.
Prettier has no directive construct.
The input has a tab after `:::`.
`decided by: accident (negative codes and surrogates in the Unicode predicates); the extension's grammar is the norm`
`runner: directive tab name`

~~~~markdown
:::	a
x
~~~~

```html
<!-- micromark: a container directive named "\t", holding the paragraph -->
<p>x</p>
<!-- ours: no directive, one paragraph -->
<p>:::	a
x</p>
```

## Blank lines inside a directive make the enclosing list item loose

A directive's content is a lazily tokenized sub-document; its blank line endings surface at the enclosing list item,
which micromark then marks loose.
A blank line inside a nested blockquote or fence does not do that, and neither does one inside a directive for us.
Prettier has no directive construct.
`decided by: accident (the sub-document's blank line endings leak); the extension is the norm`
`runner: blank line in directive in item`

~~~~markdown
- a
  :::note
  b

  :::
~~~~

```html
<!-- micromark: loose -->
<ul>
<li>
<p>a</p>
<x-dir>b</x-dir>
</li>
</ul>
<!-- ours: tight -->
<ul>
<li>a
<x-dir>b</x-dir>
</li>
</ul>
```

## Directive interrupt drops footnote calls

A container-directive fence interrupting a paragraph makes that paragraph's footnote calls literal.
No other interrupter does this; we keep the call.
Prettier has no directive construct.
`decided by: accident (the sub-document boundary); the extension is the norm`
`runner: directive interrupt drops footnotes`

~~~~markdown
a [^1] b
::::a

[^1]: d
~~~~

```html
<!-- micromark -->
<p>a [^1] b</p>
<!-- ours -->
<p>a <sup>…</sup> b</p>
```

## Forward references to definitions inside directives

Directive content is a lazily subtokenized inner document,
so a reference before the directive does not see a definition inside it and stays text.
Prettier has no directive construct; our reference map resolves position-independently.
`decided by: accident (the sub-document boundary); the extension is the norm`
`runner: forward ref to def in directive`

~~~~markdown
x [l] y

::::a
[l]: /u
::::
~~~~

```html
<!-- micromark -->
<p>x [l] y</p>
<!-- ours -->
<p>x <a href="/u">l</a> y</p>
```

## Kept: quirks weighed and reproduced

Spec deviations we reproduce on purpose. Each was put through the same questions; none is an accident.

- **A complete type 7 tag on a lazy line interrupts the paragraph** (`> a\n<a>\n> c`: an HTML block inside the quote; cmark ≥ 0.30 reads paragraph text).
  Intent: `html-flow.js` lifts the restriction when `parser.lazy[line]`, and micromark's tests pin it.
- **A type 7 tag line outranks a table** (`a\n<a>\n:-:`: an HTML block, not a table with header `<a>`).
  Neither spec covers a delimiter row stealing the paragraph's last line; no evidence either way, so kept.
- **A `{%` / `{{` line closes the paragraph even when the tag never closes** (`a\n{% x\nb`: two paragraphs).
  Intent: the extension returns `ok` on the opener alone in interrupt mode (`isFlow && interrupt ? ok`).
- **During table continuation a liquid opener is only a candidate** (`{% x` stays a row unless the tag completes).
  Intent: `_gfmTableDynamicInterruptHack` exists for exactly this.
- **Whitespace-only text after a task-list checkbox** (`- [x]   \n  :-:` is a table, not a task item).
  No deviation: the table construct runs before the task-list check, in cmark-gfm as well.

## GFM spec suite: pinned cmark-gfm difference

The GFM suite's expectations come from cmark-gfm.
Where micromark differs, the failure stays pinned in `tasks/conformance/snapshots/gfm.snap`.

- Example 628: cmark-gfm recognizes `ftp://` autolink literals; micromark (and we) only `http` / `https` / `www.` / email.
- `<input>` attribute order differs too; the runner canonicalizes it.
