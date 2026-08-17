//! Block phase.
//!
//! - `engine`: the container-stack line loop (cmark's model), producing an IR tree
//! - `probe`: the single, priority-ordered table of block starts
//! - `lexical`: the public line-start classification for formatters, a facade over `probe`
//! - `scan`: per-construct line scanners the probe table is built from
//! - `cursor`: byte/column tracking over one physical line
//! - `refdef`: reference-definition stripping at paragraph close
//! - `ir`: the intermediate tree; `build` turns it into the arena AST and runs the inline phase

mod build;
mod cursor;
mod engine;
mod ir;
pub mod lexical;
mod probe;
mod refdef;
mod scan;

pub use build::build;
pub use engine::Engine;
pub use ir::IrSeg;
