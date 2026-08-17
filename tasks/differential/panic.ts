// Panic-safety fuzz: the parser must never crash on any valid UTF-8 input.
//
// Generates char soups that stress byte/char boundaries — multibyte
// characters directly abutting markdown syntax characters, combining marks,
// controls, NUL, BMP-edge and astral characters — and feeds them through
// the `sig-batch` binary. A crash (non-zero exit / short output)
// fails. Usage: node tasks/differential/panic.ts [count] [seed]
import { Rng, makeCorpus, signatureOurs } from "./harness.ts";

const CHARS = [
  // Markdown syntax
  "*", "_", "`", "[", "]", "(", ")", "<", ">", "!", "#", "-", "+", ".",
  "\\", "&", ";", '"', "'", "=", "~", "|", ":", "@", "$", "%", "{", "}",
  // Multibyte: 2-, 3-, 4-byte encodings; punctuation category members
  "é", "ß", "ẞ", "·", "–", "«", "»", "¿", "漢", "字", "（", "）", "、",
  "　", "！", "🎉", "𝄞", "𓀀",
  // Combining / zero-width / boundary oddities
  "́", "‍", "​", "﻿", "�",
  // Whitespace-ish and controls
  " ", "\t", "\n", "\r", " ", " ", "", "\x00", "\x01",
  "\x7f",
  // Plain filler
  "a", "1",
];

const TEMPLATES: Array<(x: string) => string> = [
  (x) => `*${x}*`,
  (x) => `**${x}**_${x}_`,
  (x) => `[${x}](${x})`,
  (x) => `[${x}]: /${x}\n[${x}]`,
  (x) => `<${x}@${x}.com>`,
  (x) => `<${x}:${x}>`,
  (x) => `\`${x}\n${x}\``,
  (x) => `![${x}](${x} '${x}')`,
  (x) => `# ${x}\n${x}\n===\n`,
  (x) => `> ${x}\n${x}`,
  (x) => `- ${x}\n  - ${x}`,
  (x) => `\\${x}\\\n${x}`,
  (x) => `&${x};`,
  (x) => `<!--${x}-->`,
  (x) => `<a b='${x}'>`,
  (x) => `~~~${x}\n${x}`,
  (x) => `$$${x}\n${x}\n$$`,
  (x) => `$${x}$`,
  (x) => `{{${x}}}`,
  (x) => `{%${x}%}`,
  (x) => `[[${x}]]`,
  (x) => `:::${x}\n${x}\n:::`,
  (x) => `:::a[${x}]{${x}}\n${x}`,
];

function gen(rng: Rng): string {
  if (rng.random() < 0.3) {
    return rng.choice(TEMPLATES)(rng.soup(CHARS, rng.int(1, 4)));
  }
  return rng.soup(CHARS, rng.int(1, 120));
}

function main(): number {
  const { count, seed, corpus } = makeCorpus(10000, gen);

  // Batch so one crash doesn't hide which inputs survived; bisect on
  // failure.
  const batchSize = 500;
  for (let at = 0; at < count; at += batchSize) {
    const batch = corpus.slice(at, at + batchSize);
    const result = signatureOurs(batch, true);
    if (result.outputs === null) {
      // Bisect to a crashing input: halve, keep a failing half.
      let failing = batch;
      let stderr = result.stderr;
      while (failing.length > 1) {
        const half = failing.slice(0, failing.length >> 1);
        const r = signatureOurs(half, true);
        if (r.outputs === null) {
          failing = half;
          stderr = r.stderr;
        } else {
          const rest = failing.slice(half.length);
          const r2 = signatureOurs(rest, true);
          if (r2.outputs !== null) {
            console.log("batch failed but neither half reproduces; stderr:");
            console.log(result.stderr.slice(-2000));
            return 1;
          }
          failing = rest;
          stderr = r2.stderr;
        }
      }
      console.log("CRASH on input:");
      console.log(JSON.stringify(failing[0]));
      console.log(stderr.slice(-2000));
      return 1;
    }
  }
  console.log(`panic fuzz: ${count} inputs survived (seed ${seed})`);
  return 0;
}

process.exitCode = main();
