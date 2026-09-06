//! gha-see - offline GitHub Actions visualizer & validator.

pub mod analysis;
pub mod api;
pub mod discover;
pub mod eval;
pub mod expr;
pub mod fetch;
pub mod findings;
pub mod graph;
pub mod ir;
pub mod matrix;
pub mod parse;
pub mod triggers;
pub mod uses;

pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}
