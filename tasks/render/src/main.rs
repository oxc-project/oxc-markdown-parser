//! `sig-batch`: reads a JSON array of markdown strings on stdin, prints the
//! JSON array of their structural signatures. The subprocess bridge for the
//! differential and panic fuzz runners (`tasks/differential/harness.ts`).
//!
//! Parses with the default options — the full Prettier composition plus
//! container directives, matching the mdast oracle's configuration.
//! `--parse-only` (the panic fuzz) skips the signatures: the array then
//! holds empty strings, one per input, so the batch length still checks out.

use oxc_markdown_parser::{Allocator, Parser};

fn main() {
    let parse_only = std::env::args().any(|a| a == "--parse-only");
    let buf = std::io::read_to_string(std::io::stdin()).unwrap();
    let inputs: Vec<String> = serde_json::from_str(&buf).expect("JSON array of strings");
    let mut allocator = Allocator::default();
    let outputs: Vec<String> = inputs
        .iter()
        .map(|md| {
            let ret = Parser::new(&allocator, md).parse();
            let sig =
                if parse_only { String::new() } else { render::sig::signature(md, &ret.root) };
            allocator.reset();
            sig
        })
        .collect();
    serde_json::to_writer(std::io::stdout().lock(), &outputs).unwrap();
}
