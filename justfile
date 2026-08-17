# oxc-markdown-parser tasks. Run `just` (or `just --list`) to see all recipes.

# List available recipes
_default:
    @just --list

# Run the test suite (mirrors CI)
test:
    cargo test --all-features

# Format the codebase
fmt:
    cargo fmt

# Run the CommonMark spec suite and regenerate the committed snapshot under
# tasks/conformance/snapshots/. Review with `git diff`.
conformance:
    cargo run -p conformance

# Fetch the conformance suite files (pinned versions).
conformance-clone:
    mkdir -p tasks/conformance/repos
    curl -fsSL https://spec.commonmark.org/0.31.2/spec.json -o tasks/conformance/repos/commonmark-spec.json
    curl -fsSL https://raw.githubusercontent.com/github/cmark-gfm/499789b49373bfa045d0e7547e5ee63444c77bca/test/spec.txt -o tasks/conformance/repos/gfm-spec.txt

# Differential fuzz against mdast (micromark, the parse-behavior oracle).
# Requires `npm install` in tasks/differential once, and Node >= 22.18
# (runs `.ts` directly via type stripping).
# Known mismatches are documented in DIVERGENCES.md.
differential count="5000" seed="0":
    node tasks/differential/run.ts {{ count }} {{ seed }}

# Diff the vendored Prettier parser pieces (`tasks/differential/*.mjs` with a
# vendor header) against the Prettier commit pinned in each header.
# Not run in CI; part of the manual Prettier bump.
vendor-check:
    node tasks/differential/vendor.ts

# Has Prettier changed those pieces (or the composition they sit in) since
# the pinned commit? Compares the pin with `ref` upstream; run by hand when bumping Prettier.
vendor-drift ref="main":
    node tasks/differential/vendor.ts --ref {{ ref }}

# Panic-safety fuzz: char soups stressing byte/char boundaries; any crash fails.
fuzz-panic count="10000" seed="0":
    node tasks/differential/panic.ts {{ count }} {{ seed }}
