// Differential fuzz against micromark (the parse-behavior oracle).
//
// Generates a deterministic corpus of markdown-token soups, reduces each to
// a structural signature on both sides — our AST (the `sig-batch` binary)
// and `mdast-util-from-markdown`'s tree (in-process, `mdast.mjs`) — and
// reports divergences. mdast, not micromark's HTML, is what Prettier
// consumes, so the HTML compiler's line-ending quirks never enter here.
// Usage: node tasks/differential/run.ts [count] [seed]
import { Rng, makeCorpus, signatureOurs } from "./harness.ts";
import { signatureAll, taskTextTrimmed } from "./mdast.mjs";

const TOKENS = [
  "*", "**", "_", "__", "`", "``", "[", "]", "(", ")", "<", ">", "#", "##",
  "-", "+", "1.", "2)", ">", "\\", "&", "&amp;", "&#35;", "!", '"', "'",
  "=", "~", "|", ":", "a", "foo", "bar b", "é", "漢字", "http://x.y",
  "user@e.com", "\\*", "\\\\", " ", "  ", "\t", "\n", "\n\n", "  \n",
  "\\\n", "```\n", "---", "===", "***", "    ", "[x]: /u", "[x]",
  '"t"', "'t'", "<div>", "</div>", "<!-- c -->", "![", "](u)",
  // HTML type 7 (complete non-block tag): cannot interrupt a paragraph,
  // and its lazy-line handling is a documented micromark divergence.
  "<a>", "<span x=1>", "</span>", "> x\n<a>\n",
  // HTML type 1 (raw text elements): swallow everything up to the closing
  // tag, blank lines and container prefixes included.
  "<script>", "</script>", "<pre x>", "</pre>", "<textarea>\n", "<style", "</style>",
  "~~", "~", "[^1]", "[^1]: n", "- [x] ", "- [ ] ", "| a | b |", "| - | - |",
  "-|-", ":-:", "www.a.com", "http://x.y/p(q)", "u@e.com", "W", "h",
  // Label normalization: micromark folds lower→upper (σ/ς, ﬀ/FF)
  // and only collapses markdown whitespace (U+3000 stays).
  "[σ]: /u", "[ς]", "[ﬀ]: /u", "[ff]", "\u3000",
  // Punctuation added after Unicode 13 (flanking classification), and an
  // astral symbol (the `astral punctuation` divergence).
  "\u2E53", "\uFDFE", "😀",
  // Prettier extensions: math ($/$$), liquid ({%…%}/{{…}}), wiki-link,
  // container directives (:::).
  "$", "$$", "$$\n", "$x$", "$$m\n", "{%", "%}", "{{", "}}", "{{ v }}",
  "{% t %}", "[[", "]]", "[[w]]",
  ":::", "::::", ":::\n", ":::note\n", ":::note", "::::a\n", "[l]", "{.c}",
  ":::a[l]{#i k=v}\n",
  // Indented code and whitespace-only lines of 4+ columns (the `deep-blank
  // after indented code` quirk).
  "    a\n", "    \n", "\t\n", "      \n", "  \t\r\n",
  // Lone "\r" is deliberately absent: a bare CR adjacent to another line
  // ending makes the CRLF-normalized comparison ambiguous (two breaks vs
  // one), producing false mismatches. "\r\n" covers the CRLF behavior.
  "\r\n", "  \r\n",
];

function gen(rng: Rng): string {
  // Normalize to a trailing newline: micromark only emits the final line
  // ending when the input has one, cmark-style output always does.
  return rng.soup(TOKENS, rng.int(2, 40)).replace(/\n+$/, "") + "\n";
}

// `search(text, from)` for the offset searches below: only these regexes
// carry `g` (so `lastIndex` can seed the start position), and every use
// goes through here, which also resets that shared state.
function searchFrom(re: RegExp, text: string, from: number): RegExpExecArray | null {
  re.lastIndex = from;
  return re.exec(text);
}

// Shared fragments for the shapes below.
// Markdown's whitespace is only space and tab: `\s`/`\S` would misclassify
// U+3000 and friends, so the fragments spell the classes out.
// Four columns of indent: a tab reaches the stop from any of the first four columns.
const indent4 = String.raw`(?: {0,3}\t| {4})`;
const nonBlank = String.raw`[^ \t\r\n]`;
const blankLine = String.raw`[ \t]*\r?\n`;
const astral = String.raw`[\u{10000}-\u{10FFFF}]`;
const marker = String.raw`(?:[-+*]|\d{1,9}[.)])`;
// A container opener with its marker whitespace (a bare `>` needs none).
const containerOpener = String.raw`[ \t]{0,3}(?:>|${marker}[ \t])`;
// A list marker and what ends it, the line's end spelled out so a shape
// without the `m` flag (where `^` must mean document start) can use it.
const listMarker = String.raw`${marker}(?:[ \t]|\r?\n|$)`;

// A whitespace-only line with 4+ columns next to indented code (micromark
// keeps it as trailing code content; the spec drops it).
const deepBlank = new RegExp(String.raw`^${indent4}[ \t]*\r?$`, "m");
const codeIndent = new RegExp(String.raw`^${indent4}${nonBlank}`, "m");

// Prettier's liquid extension: a flow attempt at a `{%`/`{{` line that
// fails poisons micromark's interrupt/lazy bookkeeping for later lines —
// restricted list markers stay paragraph text, fences/math/html end early.
// An attempt fails when no line after the opener ends with its closer, or
// when a container-start line (which interrupts the non-`concrete`
// construct) comes first. The exemption also requires a block-ish line
// after the failed opener (the thing the corrupted state mis-parses), to
// keep its blast radius small.
const LIQUID_DELIMS: Array<[RegExp, RegExp]> = [
  [/^[ \t]{0,3}\{%/gm, /%\}[ \t]*\r?$/gm],
  [/^[ \t]{0,3}\{\{/gm, /\}\}[ \t]*\r?$/gm],
];
const poisonTarget = /^[ \t]{0,3}(?:[-+*#<>`~$]|\d{1,9}[.)])/gm;
// micromark's `document` (container) constructs (blockquote, a non-empty list
// item, a GFM footnote definition) with the paragraph-interrupt restriction on
// ordered items (must start at 1), which is what ends a non-`concrete` flow
// attempt mid-construct.
const interruptingContainerLine =
  /^[ \t]{0,3}(?:>|[-+*][ \t]+\S|0{0,8}1[.)][ \t]+\S|\[\^[^\]\s]+\]:)/gm;

function failedLiquid(text: string): boolean {
  for (const [opener, closer] of LIQUID_DELIMS) {
    for (const m of text.matchAll(opener)) {
      const end = m.index + m[0].length;
      const close = searchFrom(closer, text, end);
      const failed =
        close === null ||
        (() => {
          const container = searchFrom(interruptingContainerLine, text, end);
          return container !== null && container.index < close.index;
        })();
      if (failed && searchFrom(poisonTarget, text, end) !== null) {
        return true;
      }
    }
  }
  return false;
}

// The directive extension's factoryName treats tabs (a negative code) and
// astral characters (surrogates) as name characters: holes in its Unicode
// predicates. We classify both properly.
const directiveTabName = new RegExp(String.raw`^[ \t]{0,3}:{3,}${nonBlank}*(?:\t|${astral})`, "mu");

// A blank line inside a directive's content, itself inside a list item:
// micromark's lazily tokenized sub-document leaks the blank line ending to
// the item, which becomes loose. Blank lines inside a blockquote or a fence
// do not do this, and neither does ours.
const blankInDirectiveInItem = new RegExp(
  String.raw`^[ \t]{0,3}${marker}[ \t][^\n]*\n(?:[^\n]*\n)*?[ \t]+:{3,}[^ \t\r\n:][^\n]*\n(?:[^\n]*\n)*?${blankLine}`,
  "m",
);

// A directive fence directly interrupting a paragraph makes micromark drop
// that paragraph's footnote calls (extension bug; we keep them) — so the
// footnote-ish bracket must sit on the interrupted line itself.
const directiveInterruptFn = /^[^\n]*\[\^[^\n]*\r?\n[ \t]{0,3}:{3,}/m;

// A reference that precedes a definition sitting inside a directive's
// (lazily subtokenized) content stays text in micromark's tree; we resolve
// position-independently — so a `[` must precede the opener.
const directiveOpenLine = /^[ \t]{0,3}:{3,}[^\s:]/gm;
const defLine = /^[ \t>]*\[[^\]\n]*\]:/gm;

function forwardRefToDefInDirective(text: string): boolean {
  const firstBracket = text.indexOf("[");
  if (firstBracket === -1) {
    return false;
  }
  // Any opener, not just the first: a nested directive hides its definitions the same way.
  for (const m of text.matchAll(directiveOpenLine)) {
    if (firstBracket < m.index && searchFrom(defLine, text, m.index + m[0].length) !== null) {
      return true;
    }
  }
  return false;
}

// Indented code opened on the line that closes a container (blockquote or
// list item) by mismatch: micromark ends it there and opens a second block
// on the next indented line; ours (spec) is one block. A complete-tag line
// (a lazy type 7 tag the quote absorbed) or a blank line may sit between.
const tagLine = String.raw`[ \t]{0,3}<\/?[A-Za-z][A-Za-z0-9-]*(?:[ \t][^<>\n]*)?>[ \t]*\r?\n`;
const codeAfterContainerClose = new RegExp(
  String.raw`^[ \t]{0,3}(?:>.*|${marker}[ \t]*)\r?\n(?:${tagLine}|${blankLine})?${indent4}[ \t]*${nonBlank}.*\r?\n${indent4}[ \t]*${nonBlank}`,
  "m",
);

// A fence opener (code or math), possibly behind container markers, with an
// info string that ends in whitespace: micromark's meta token keeps the
// whitespace; our span ends at the last non-blank.
const fenceMetaTrailing = new RegExp(
  String.raw`^(?:[ \t>]|${marker}[ \t])*(?:\x60{3,}[^\x60\n]*|~{3,}[^\n]*|\${2,}[^$\n]*)${nonBlank}[ \t]+\r?$`,
  "m",
);

// An astral character next to an attention marker: punctuation to the spec
// (and us), a surrogate to micromark, so flanking differs.
const astralAttention = new RegExp(String.raw`[*_~]${astral}|${astral}[*_~]`, "u");

// micromark fixes its paragraph-interrupt flag at line start and keeps it
// through every container opened on that line; the two shapes below are the
// two ways a list marker ends up under that flag with no paragraph to
// interrupt (the spec's restriction is on the open paragraph alone).
// A list marker line after an indented code line (itself at document start,
// after a blank line, or after another indented line, so it is code and not
// paragraph continuation); blank lines between allowed, as micromark keeps
// the code open across them. No `m` flag: `^` must mean document start.
const listAfterIndentedCode = new RegExp(
  String.raw`(?:^|\n${blankLine}|\n${indent4}[^\n]*\n)${indent4}[ \t]*${nonBlank}[^\n]*\n(?:${blankLine})*(?:${containerOpener})*[ \t]{0,3}${listMarker}`,
);
// Container openers followed on the same line by a list marker, right after
// a non-blank line (the paragraph the first opener closed).
const listAfterContainerOnLine = new RegExp(
  String.raw`[^\n]\r?\n(?:${containerOpener})+[ \t]{0,3}${listMarker}`,
);

type Quirk = [string, (text: string) => boolean];

// Documented divergences (one per `DIVERGENCES.md` section), exempted by
// input shape: a match only exempts, it does not verify — the per-quirk
// counters below keep the masking visible.
const QUIRKS: Quirk[] = [
  ["deep-blank after indented code", (t) => deepBlank.test(t) && codeIndent.test(t)],
  ["fence meta trailing whitespace", (t) => fenceMetaTrailing.test(t)],
  ["indented code split after container close", (t) => codeAfterContainerClose.test(t)],
  ["astral punctuation", (t) => astralAttention.test(t)],
  ["list after indented code", (t) => listAfterIndentedCode.test(t)],
  ["list after a container opened on the same line", (t) => listAfterContainerOnLine.test(t)],
  ["liquid interrupt poisoning", failedLiquid],
  ["directive tab name", (t) => directiveTabName.test(t)],
  ["blank line in directive in item", (t) => blankInDirectiveInItem.test(t)],
  ["directive interrupt drops footnotes", (t) => directiveInterruptFn.test(t)],
  ["forward ref to def in directive", forwardRefToDefInDirective],
];

/** Names the documented micromark quirk a mismatch falls under, or null. */
function classify(text: string): string | null {
  return QUIRKS.find(([, pred]) => pred(text))?.[0] ?? null;
}

function main(): number {
  const { count, seed, corpus } = makeCorpus(5000, gen);

  const rust = signatureOurs(corpus);
  if (rust.outputs === null) {
    console.error("sig-batch failed:", rust.stderr);
    return 1;
  }
  const ours = rust.outputs;
  const theirs = signatureAll(corpus);

  // Classify each mismatch as it is found.
  const knownCases: Array<{ i: number; quirk: string }> = [];
  const unexplained: number[] = [];
  for (let i = 0; i < count; i++) {
    if (ours[i] === theirs[i]) {
      continue;
    }
    const quirk = classify(corpus[i]);
    if (quirk === null) {
      unexplained.push(i);
    } else {
      knownCases.push({ i, quirk });
    }
  }
  const matched = count - knownCases.length - unexplained.length;
  console.log(
    `differential: ${matched}/${count} match (seed ${seed}), ` +
      `${knownCases.length} known divergences, ` +
      `${unexplained.length} unexplained`,
  );
  // Keep the masked cases visible so the exemption can't silently grow.
  const counts = new Map<string, number>();
  for (const { quirk } of knownCases) {
    counts.set(quirk, (counts.get(quirk) ?? 0) + 1);
  }
  for (const [q, n] of [...counts].sort(([a], [b]) => a.localeCompare(b))) {
    console.log(`known[${q}]: ${n}`);
  }
  if (taskTextTrimmed > 0) {
    console.log(`normalized[task-list text after the checkbox]: ${taskTextTrimmed}`);
  }
  for (const { i, quirk } of knownCases.slice(0, 2)) {
    console.log(`known sample [${quirk}]:`, JSON.stringify(corpus[i].slice(0, 80)));
  }
  for (const i of unexplained.slice(0, 10)) {
    console.log("=== input ===");
    console.log(JSON.stringify(corpus[i]));
    console.log("--- ours ---");
    console.log(JSON.stringify(ours[i]));
    console.log("--- mdast ---");
    console.log(JSON.stringify(theirs[i]));
  }
  if (unexplained.length > 10) {
    console.log(`... and ${unexplained.length - 10} more`);
  }
  return unexplained.length > 0 ? 1 : 0;
}

process.exitCode = main();
