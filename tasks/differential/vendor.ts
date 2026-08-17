// Prettier's parser composition is what the oracle must mirror. Two of its
// pieces are source files the npm package does not ship (it bundles them),
// so they are vendored verbatim under a header naming the Prettier commit;
// two more are mirrored by hand in `mdast.mjs` rather than vendored.
//
//   node tasks/differential/vendor.ts
//     Each vendored file is byte-equal to its upstream at the pinned commit.
//     Never fails because of upstream activity.
//   node tasks/differential/vendor.ts --ref <git-ref>
//     Every watched upstream file is unchanged between the pinned commit and
//     `<git-ref>` (`main`, a release tag). Fails when Prettier moved, which
//     means: re-vendor, re-read the composition, bump the headers. Run by
//     hand when bumping Prettier, not in CI.
import fs from "node:fs";
import path from "node:path";

const HERE = import.meta.dirname;
const MARKER = "// ---- vendored ----\n";
const HEADER = /^\/\/ Vendored verbatim from prettier\/prettier@(\w+)\n\/\/ (\S+)\n/;
// Our own modules: every other `.mjs` here must carry the vendor header.
const OURS = new Set(["mdast.mjs"]);
// Upstream files mirrored in `mdast.mjs` rather than vendored: the parser
// composition itself, and the autolink-transform hack.
const MIRRORED = [
  "src/language-markdown/parse/parse-markdown.js",
  "src/language-markdown/parse/micromark/mdast-util-gfm.js",
];

async function upstream(ref: string, file: string): Promise<string> {
  const url = `https://raw.githubusercontent.com/prettier/prettier/${ref}/${file}`;
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`${url} -> ${res.status}`);
  }
  return res.text();
}

type Vendored = { name: string; sha: string; file: string; body: string };

function readVendored(): Vendored[] {
  const names = fs.readdirSync(HERE).filter((f) => f.endsWith(".mjs") && !OURS.has(f));
  return names.map((name) => {
    const text = fs.readFileSync(path.join(HERE, name), "utf8");
    const header = HEADER.exec(text);
    const at = text.indexOf(MARKER);
    if (header === null || at === -1) {
      throw new Error(`${name}: missing vendor header (add it, or list the file in OURS)`);
    }
    return { name, sha: header[1], file: header[2], body: text.slice(at + MARKER.length) };
  });
}

/** The one commit every vendored header names. */
function pinned(vendored: Vendored[]): string {
  const shas = new Set(vendored.map((v) => v.sha));
  if (shas.size !== 1) {
    throw new Error(`vendored files pin different commits: ${[...shas].join(", ")}`);
  }
  return vendored[0].sha;
}

async function checkPinned(vendored: Vendored[]): Promise<string[]> {
  const results = await Promise.all(
    vendored.map(async (v) => {
      const same = v.body === (await upstream(v.sha, v.file));
      console.log(`${v.name}: ${same ? "matches" : "DIFFERS FROM"} prettier@${v.sha}`);
      return same ? null : v.name;
    }),
  );
  return results.filter((r): r is string => r !== null);
}

async function checkDrift(vendored: Vendored[], ref: string): Promise<string[]> {
  const sha = pinned(vendored);
  const files = [...vendored.map((v) => v.file), ...MIRRORED];
  const results = await Promise.all(
    files.map(async (file) => {
      const [was, now] = await Promise.all([upstream(sha, file), upstream(ref, file)]);
      const same = was === now;
      console.log(`${file}: ${same ? "unchanged" : "CHANGED"} between ${sha} and ${ref}`);
      return same ? null : file;
    }),
  );
  return results.filter((r): r is string => r !== null);
}

const refAt = process.argv.indexOf("--ref");
const ref = refAt === -1 ? null : process.argv[refAt + 1];
if (refAt !== -1 && !ref) {
  console.error("usage: node tasks/differential/vendor.ts [--ref <git-ref>]");
  process.exit(2);
}
const vendored = readVendored();
const problems = ref === null ? await checkPinned(vendored) : await checkDrift(vendored, ref);
process.exitCode = problems.length === 0 ? 0 : 1;
