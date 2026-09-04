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

# Fetch the CommonMark and GFM spec suites (pinned versions), run them, and
# regenerate the committed snapshots under tasks/conformance/snapshots/.
# Review with `git diff`.
conformance:
    cargo run -p conformance

# Fetch the conformance suite files without running them.
conformance-clone:
    cargo run -p conformance -- --clone

# Remove the fetched conformance suite files.
conformance-clean:
    cargo run -p conformance -- --clean

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
