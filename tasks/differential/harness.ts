// Shared pieces of the differential/panic fuzz runners: a deterministic
// PRNG, the seeded-corpus preamble, and the subprocess bridge to our
// parser's batch signature binary.
import { spawnSync } from "node:child_process";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "../..");

/**
 * mulberry32: `Math.random` is unseedable, and the corpus must be
 * reproducible from `[count] [seed]` alone.
 */
export class Rng {
  #state: number;

  constructor(seed: number) {
    this.#state = seed >>> 0;
  }

  /** Uniform float in `[0, 1)`. */
  random(): number {
    let t = (this.#state = (this.#state + 0x6d2b79f5) | 0);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }

  /** Integer in `[lo, hi]`, both inclusive. */
  int(lo: number, hi: number): number {
    return lo + Math.floor(this.random() * (hi - lo + 1));
  }

  choice<T>(items: readonly T[]): T {
    return items[this.int(0, items.length - 1)];
  }

  /** `n` random picks from `items`, concatenated. */
  soup(items: readonly string[], n: number): string {
    return Array.from({ length: n }, () => this.choice(items)).join("");
  }
}

/** Parses the `[count] [seed]` positionals every runner takes. */
function cliArgs(defaultCount: number): { count: number; seed: number } {
  const count = Number(process.argv[2] ?? defaultCount);
  const seed = Number(process.argv[3] ?? 0);
  if (!Number.isInteger(count) || !Number.isInteger(seed)) {
    console.error(`usage: node ${process.argv[1]} [count] [seed]`);
    process.exit(1);
  }
  return { count, seed };
}

/** The runners' shared preamble: a seeded corpus from `[count] [seed]`. */
export function makeCorpus(
  defaultCount: number,
  gen: (rng: Rng) => string,
): { count: number; seed: number; corpus: string[] } {
  const { count, seed } = cliArgs(defaultCount);
  const rng = new Rng(seed);
  return { count, seed, corpus: Array.from({ length: count }, () => gen(rng)) };
}

let built = false;

export type Batch = { outputs: string[] | null; stderr: string };

// The `render` crate's binary (`tasks/render/Cargo.toml`, `[[bin]]`).
const BIN = path.join(ROOT, "target/debug/sig-batch");

/**
 * One up-front `cargo build` keeps the auto-rebuild guarantee; the
 * per-batch spawns then exec the binary directly. (`cargo run` per batch
 * would re-check the whole workspace fingerprint each time — that check
 * dominates the runtime of a many-batch run like panic.ts.)
 */
function ensureBuilt(): string | null {
  if (!built) {
    const b = spawnSync("cargo", ["build", "-q", "-p", "render"], { cwd: ROOT, encoding: "utf8" });
    if (b.status !== 0) {
      return b.stderr ?? "";
    }
    built = true;
  }
  return null;
}

/**
 * Signatures of a batch from our parser (`tasks/render`'s `sig-batch`).
 * `outputs` is null when the binary failed (a panic, a build error, output
 * that is not one entry per input); `stderr` then holds the diagnostics.
 * `parseOnly` skips the signatures (the panic fuzz never reads them).
 */
export function signatureOurs(inputs: string[], parseOnly = false): Batch {
  const buildError = ensureBuilt();
  if (buildError !== null) {
    return { outputs: null, stderr: buildError };
  }
  const r = spawnSync(BIN, parseOnly ? ["--parse-only"] : [], {
    input: JSON.stringify(inputs),
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 1 << 28,
  });
  let outputs: string[] | null = null;
  if (r.status === 0) {
    try {
      const parsed = JSON.parse(r.stdout);
      if (Array.isArray(parsed) && parsed.length === inputs.length) {
        outputs = parsed;
      }
    } catch {}
  }
  return { outputs, stderr: r.stderr ?? "" };
}
