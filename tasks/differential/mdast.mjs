// The mdast oracle: `mdast-util-from-markdown` over micromark in the full
// Prettier composition (GFM with `singleTilde: false`, math, wiki-link with
// the alias hack, liquid, the html-text override) plus container
// directives, reduced to the structural signature `tasks/render/src/sig.rs`
// prints for our AST. Values are cooked the way mdast cooks them.
import { fromMarkdown as wikiLinkFromMarkdown } from "@braindb/mdast-util-wiki-link";
import { syntax as wikiLinkSyntax } from "@braindb/micromark-extension-wiki-link";
import { directiveFromMarkdown } from "mdast-util-directive";
import { fromMarkdown } from "mdast-util-from-markdown";
import { gfmFromMarkdown } from "mdast-util-gfm";
import { mathFromMarkdown } from "mdast-util-math";
import { gfm } from "micromark-extension-gfm";
// Deep import (bypassing the exports map): the package index also loads
// the KaTeX-backed HTML side.
import { math } from "./node_modules/micromark-extension-math/lib/syntax.js";
import { codes } from "micromark-util-symbol";
// Deep import (bypassing the package's exports map on purpose): only the
// container form is a default construct — text (`:name`) collides with
// prose like `12:30`, and leaf (`::name`) rides with it.
import { directiveContainer } from "./node_modules/micromark-extension-directive/lib/directive-container.js";
import { overrideHtmlTextSyntax } from "./html-text.mjs";
import { liquidFromMarkdown, liquidSyntax } from "./liquid.mjs";

// Prettier drops the autolink-literal transform (it re-scans text nodes);
// only the tokenizer's literals become links.
const gfmMdast = gfmFromMarkdown();
gfmMdast.find((e) => e.enter?.literalAutolink).transforms = [];

const OPTIONS = {
  extensions: [
    gfm({ singleTilde: false }),
    math(),
    // Prettier disables the alias divider with a NaN charCodeAt hack.
    wikiLinkSyntax({ aliasDivider: { charCodeAt: () => Number.NaN } }),
    liquidSyntax(),
    overrideHtmlTextSyntax(),
    { flow: { [codes.colon]: directiveContainer } },
  ],
  mdastExtensions: [
    gfmMdast,
    mathFromMarkdown(),
    wikiLinkFromMarkdown(),
    liquidFromMarkdown(),
    directiveFromMarkdown(),
  ],
};

/** Signatures of each markdown string's mdast. */
export function signatureAll(inputs) {
  return inputs.map((md) => signature(md, fromMarkdown(md, OPTIONS)));
}

/**
 * Inputs on which `trimTaskText` changed the signature: a normalization,
 * not a quirk exemption, so counted here to stay visible.
 */
export let taskTextTrimmed = 0;

// micromark keeps the source line-ending style; both sides fold to `\n`.
const q = (s) => JSON.stringify((s ?? "").replace(/\r\n?/g, "\n"));

function signature(src, tree) {
  let out = "";
  const blocks = (nodes) => {
    out += "[";
    for (const n of nodes) {
      block(n);
      out += ",";
    }
    out += "]";
  };
  const inlines = (nodes) => {
    out += "(";
    for (const n of nodes) inline(n);
    out += ")";
  };
  function block(n) {
    switch (n.type) {
      case "paragraph":
        out += "p";
        inlines(n.children);
        break;
      case "heading":
        out += `h${n.depth}`;
        inlines(n.children);
        break;
      case "thematicBreak":
        out += "hr";
        break;
      case "code":
        out += `code{${q(n.lang)}}{${q(n.meta)}}{${q(n.value)}}`;
        break;
      case "html":
        out += `html{${q(n.value)}}`;
        break;
      case "blockquote":
        out += "bq";
        blocks(n.children);
        break;
      case "list": {
        // cmark's tightness is list-wide; mdast splits it into the list's
        // (blank lines between items) and each item's (between children,
        // compared per item below).
        const spread = n.spread || n.children.some((item) => item.spread);
        out += `list{${n.ordered},${n.ordered ? String(n.start ?? 1) : ""},spread=${spread}}[`;
        for (const item of n.children) {
          const checked = item.checked === true ? "x" : item.checked === false ? "o" : "";
          out += `li{${checked},spread=${item.spread}}`;
          blocks(checked === "" ? item.children : trimTaskText(item.children));
          out += ",";
        }
        out += "]";
        break;
      }
      case "definition":
        out += `def{${q(n.identifier)},${q(n.url)},${q(n.title)}}`;
        break;
      case "table":
        out += `table{${(n.align ?? []).map((a) => (a == null ? "-" : a[0])).join("")}}[`;
        for (const row of n.children) {
          out += "tr[";
          for (const cell of row.children) {
            out += "td";
            inlines(cell.children);
            out += ",";
          }
          out += "],";
        }
        out += "]";
        break;
      case "footnoteDefinition":
        out += `fn{${q(n.identifier)}}`;
        blocks(n.children);
        break;
      case "math":
        out += `math{${q(n.meta)}}{${q(n.value)}}`;
        break;
      case "liquidNode":
        out += `liquid{${q(n.value)}}`;
        break;
      case "containerDirective":
        // The `[label]` of the fence line is a paragraph child to mdast;
        // our fence line stays verbatim, label included (see AGENTS.md).
        out += "dir";
        blocks(n.children.filter((c) => !c.data?.directiveLabel));
        break;
      default:
        out += n.type;
    }
  }
  function inline(n) {
    switch (n.type) {
      case "text":
        out += `t{${q(n.value)}}`;
        break;
      case "emphasis":
        out += "em";
        inlines(n.children);
        break;
      case "strong":
        out += "strong";
        inlines(n.children);
        break;
      case "delete":
        out += "del";
        inlines(n.children);
        break;
      case "inlineCode":
        out += `code{${q(n.value)}}`;
        break;
      case "link":
        // Autolinks and GFM literals are `link` nodes too; the source tells,
        // and its raw text is the whole comparison (kind and extent).
        if (src[n.position.start.offset] !== "[") {
          out += `auto{${q(src.slice(n.position.start.offset, n.position.end.offset))}}`;
          break;
        }
        out += `a{${q(n.url)},${q(n.title)}}`;
        inlines(n.children);
        break;
      case "image":
        out += `img{${q(n.url)},${q(n.title)}}{${q(n.alt)}}`;
        break;
      case "linkReference":
        out += `aref{${n.referenceType},${q(n.identifier)}}`;
        inlines(n.children);
        break;
      case "imageReference":
        out += `imgref{${n.referenceType},${q(n.identifier)}}{${q(n.alt)}}`;
        break;
      case "html":
        out += `ihtml{${q(n.value)}}`;
        break;
      case "break":
        out += "br";
        break;
      case "footnoteReference":
        out += `fnref{${q(n.identifier)}}`;
        break;
      case "inlineMath":
        out += "imath";
        break;
      case "wikiLink":
        out += "wiki";
        break;
      case "liquidNode":
        out += `liquid{${q(n.value)}}`;
        break;
      default:
        out += n.type;
    }
  }
  blocks(tree.children);
  return out;
}

// After the checkbox `mdast-util-gfm-task-list-item` drops one character
// of the first text, so extra spaces stay text and `\r\n` leaves a `\n`.
// Our paragraph starts at the content and drops a leading soft break
// (AGENTS.md). Returns the item's children with that prefix removed from a
// copy of the first paragraph; the tree itself is left as parsed.
function trimTaskText(children) {
  const [paragraph, ...rest] = children;
  const head = paragraph?.type === "paragraph" ? paragraph.children[0] : undefined;
  if (head?.type !== "text") return children;
  const value = head.value.replace(/^[ \t]*\n?/, "");
  if (value === head.value) return children;
  taskTextTrimmed++;
  const inlines = value === "" ? paragraph.children.slice(1) : [{ ...head, value }, ...paragraph.children.slice(1)];
  return [{ ...paragraph, children: inlines }, ...rest];
}
