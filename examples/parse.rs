#![expect(clippy::print_stdout)]
use oxc_markdown_parser::{Allocator, Parser};

fn main() {
    let path = std::env::args().nth(1).expect("usage: parse <file>");
    let source = std::fs::read_to_string(&path).expect("failed to read file");
    let allocator = Allocator::default();
    let parser = Parser::new(&allocator, &source);
    let ret = parser.parse();
    println!("{:#?}", ret.root);
    for diagnostic in &ret.diagnostics {
        println!("Diagnostic: {diagnostic}");
    }
}
