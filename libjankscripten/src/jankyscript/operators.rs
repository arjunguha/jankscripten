use super::syntax::*;
use crate::notwasm::syntax as notwasm;
use crate::rts_function::RTSFunction;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::collections::HashSet;

#[macro_export]
macro_rules! typ {
    (int) => (Type::Int);
    (float) => (Type::Float);
    (bool) => (Type::Bool);
    (string) => (Type::String);
    (any) => (Type::Any);
    (dynobject) => (Type::DynObject);
    (array) => (Type::Array);
    (fun($( $arg:tt ),*) -> $ret:tt) =>
        (Type::Function(vec![ $( typ!($arg) ),* ], Box::new(typ!($ret))));
    (fun_vec($($args:tt)*) -> $($ret:tt)*) =>
        (Type::Function($($args)*, Box::new(typ!($($ret)*))));
    (unquote ( $($x:tt)* ) ) => ($($x)*);
}

/// A NotWasm operator: either a primitive Wasm operator, or a call to a function in the runtime
/// system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotwasmOp {
    BinOp(notwasm::BinaryOp),
    UnOp(notwasm::UnaryOp),
    RTS(RTSFunction),
    Metavar(usize),
    Missing,
}

#[derive(Default, Debug)]
struct Overload {
    overloads: Vec<(Type, NotwasmOp)>,
    on_other_args: Option<(Type, NotwasmOp)>,
}

#[derive(Debug, Default)]
pub struct OverloadTable {
    table: HashMap<JsOp, Overload>,
}

impl NotwasmOp {
    pub fn make_app(&self, mut args: Vec<Expr>, p: crate::pos::Pos) -> Expr {
        match self {
            NotwasmOp::BinOp(notwasm_op) => {
                let e2 = args.pop().unwrap();
                let e1 = args.pop().unwrap();
                assert_eq!(args.len(), 0);
                Expr::Binary(notwasm_op.clone(), Box::new(e1), Box::new(e2), p)
            }
            NotwasmOp::UnOp(notwasm_op) => {
                let e = args.pop().unwrap();
                assert_eq!(args.len(), 0);
                Expr::Unary(notwasm_op.clone(), Box::new(e), p)
            }
            NotwasmOp::RTS(rts_fun) => Expr::PrimCall(rts_fun.clone(), args, p),
            NotwasmOp::Missing | NotwasmOp::Metavar(..) => panic!("received {:?}", self),
        }
    }
}

impl Default for NotwasmOp {
    /// The default is `NotwasmOp::Missing`
    fn default() -> Self {
        NotwasmOp::Missing
    }
}

impl From<notwasm::BinaryOp> for NotwasmOp {
    fn from(op: notwasm::BinaryOp) -> NotwasmOp {
        NotwasmOp::BinOp(op)
    }
}

impl From<notwasm::UnaryOp> for NotwasmOp {
    fn from(op: notwasm::UnaryOp) -> NotwasmOp {
        NotwasmOp::UnOp(op)
    }
}

impl From<RTSFunction> for NotwasmOp {
    fn from(f: RTSFunction) -> NotwasmOp {
        NotwasmOp::RTS(f)
    }
}

impl OverloadTable {
    /// The set of all operators defined in the table. Not clear if we really need this to be
    /// a set, if each overload maps to a unique NotWasmOp, which may be the case. However, no
    /// harm in building a set.
    pub fn all_ops(&self) -> HashSet<NotwasmOp> {
        let mut ops = HashSet::<NotwasmOp>::new();
        for overloads in self.table.values() {
            for (_, overload) in &overloads.overloads {
                ops.insert(overload.clone());
            }
            if let Some((_, op)) = &overloads.on_other_args {
                ops.insert(op.clone());
            }
        }
        return ops;
    }

    // fn to_z3(&self, cxt: &'a z3::Context)
    fn add(&mut self, op: impl Into<JsOp>, typ: Type, notwasm: impl Into<NotwasmOp>) {
        let overload = self.table.entry(op.into()).or_insert(Overload::default());
        overload.overloads.push((typ, notwasm.into()));
    }

    fn add_on_any(&mut self, op: impl Into<JsOp>, typ: Type, notwasm: impl Into<NotwasmOp>) {
        let overload = self.table.entry(op.into()).or_insert(Overload::default());
        overload.on_other_args = Some((typ, notwasm.into()));
    }

    pub fn overloads<'a, 'b>(
        &'a self,
        op: &'b JsOp,
    ) -> impl Iterator<Item = &'a (Type, NotwasmOp)> {
        self.table
            .get(op)
            .expect(&format!("no overloads found for {:?}", op))
            .overloads
            .iter()
    }

    pub fn on_any<'a, 'b>(&'a self, op: &'b JsOp) -> Option<&'a (Type, NotwasmOp)> {
        self.table.get(op).unwrap().on_other_args.as_ref()
    }
}

lazy_static! {
    pub static ref OVERLOADS: OverloadTable = {
        use crate::javascript::syntax::BinaryOp::*;
        use crate::javascript::syntax::UnaryOp as JUO;
        use notwasm::BinaryOp::*;
        let mut table = OverloadTable::default();
        table.add(Plus, typ!(fun(int, int) -> int), I32Add);
        table.add(Plus, typ!(fun(string, string) -> string), RTSFunction::StringCat);
        table.add_on_any(Plus, typ!(fun(any, any) -> any), RTSFunction::Plus);


        table.add(Minus, typ!(fun(float, float) -> float), F64Sub);
        table.add(Minus, typ!(fun(int, int) -> int), I32Sub);
        table.add_on_any(Minus, typ!(fun(any, any) -> any), RTSFunction::Minus);

        table.add(LeftShift, typ!(fun(int, int) -> int), I32Shl);
        table.add(Times, typ!(fun(float, float) -> float), F64Mul);
        table.add(Times, typ!(fun(int, int) -> int), I32Mul);
        table.add(Times, typ!(fun(any, any) -> any), RTSFunction::Times);
        table.add(Over, typ!(fun(float, float) -> float), F64Div);
        table.add_on_any(Over, typ!(fun(any, any) -> float), RTSFunction::Over);
        table.add_on_any(LeftShift, typ!(fun(int, int) -> int), I32Shl);
        table.add_on_any(StrictEqual, typ!(fun(any, any) -> bool), RTSFunction::StrictEqual);
        // ]).others(typ!(int)),

        table.add(JUO::Minus, typ!(fun(int) -> int), UnaryOp::I32Neg);
        table.add(JUO::Minus, typ!(fun(float) -> float), UnaryOp::F64Neg);
        table.add_on_any(JUO::Minus, typ!(fun(any) -> any), RTSFunction::Neg);

        table
    };
}
