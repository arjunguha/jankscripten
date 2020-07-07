//! NotWasm: It is not WebAssembly, but quite close to it.
mod compile;
#[allow(dead_code)]
mod constructors;
mod intern;
mod parser;
mod rt_bindings;
mod translation;
mod walk;

mod elim_gotos;
mod label_apps;

use intern::intern;
use translation::translate;

use elim_gotos::elim_gotos;
use label_apps::label_apps;

pub mod syntax;
pub mod type_checking;

pub use compile::compile;
pub use parser::parse;

#[cfg(test)]
mod test_wasm;
