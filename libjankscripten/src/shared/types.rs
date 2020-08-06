//! The types associated with the JankyScript language.

use crate::notwasm::syntax::{FnType, Type as NotWasmType};

// TODO(arjun): should be exactly the same as NotWasm types for a first pass.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Any,
    Float,
    Int,
    Bool,
    Function(Vec<Type>, Box<Type>),
    String,
    Array,
    DynObject,
    // TODO: others
}

impl Type {
    pub fn notwasm_typ(&self) -> NotWasmType {
        match self {
            Type::Any => NotWasmType::Any,
            Type::Float => NotWasmType::F64,
            Type::Int => NotWasmType::I32,
            Type::Bool => NotWasmType::Bool,
            Type::Function(arg_typs, ret_ty) => NotWasmType::Fn(FnType {
                args: arg_typs.iter().map(|t| t.notwasm_typ()).collect(),
                result: Some(Box::new(ret_ty.notwasm_typ())),
            }),
            Type::String => NotWasmType::String,
            Type::Array => NotWasmType::Array,
            Type::DynObject => NotWasmType::DynObject,
        }
    }

    pub fn is_ground(&self) -> bool {
        match self {
            Type::Function(args, result_type) => {
                match **result_type {
                    Type::Any => {
                        for a in args.iter() {
                            if let Type::Any = a {
                                // pass
                            } else {
                                return false;
                            }
                        }
                        return true;
                    }
                    _ => false,
                }
            }
            _ => true,
        }
    }

    pub fn ground_function(n: usize) -> Type {
        Type::Function(vec![Type::Any; n], Box::new(Type::Any))
    }
}
