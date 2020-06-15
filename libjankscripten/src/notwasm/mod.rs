mod compile;
mod constructors;
mod index;
mod rt_bindings;
mod translation;
mod walk;
mod parser;

use index::index;
use translation::translate;

pub mod syntax;

pub use compile::compile;

#[cfg(test)]
mod test_wasm;
