//! translate NotWasm to wasm, using the rust runtime whenever possible
//!
//! preconditions: [super::compile]

use super::rt_bindings::get_rt_bindings;
use super::syntax as N;
use parity_wasm::builder::*;
use parity_wasm::elements::*;
use parity_wasm::serialize;
use std::collections::HashMap;
use std::convert::TryInto;
use Instruction::*;

const JEN_STRINGS_IDX: u32 = 0;

type FuncTypeMap = HashMap<(Vec<ValueType>, Option<ValueType>), u32>;

pub fn translate(program: N::Program) -> Result<Vec<u8>, Error> {
    serialize(translate_parity(program))
}

pub fn translate_parity(mut program: N::Program) -> Module {

    // The initial environment maps functions names to their indices.
    let mut env = Env::default(); 
    for (index, (name, _)) in program.functions.iter().enumerate() {
        env.ids.insert(name.clone(), IdIndex::Fun(index.try_into().expect("too many functions")));
    }

    let mut module = module();
    let rt_types = get_rt_bindings();
    let mut rt_indexes = HashMap::new();
    // build up indexes for mutual recursion first
    let mut type_indexes = HashMap::new();
    for (func_i, (name, ty)) in rt_types.iter().enumerate() {
        let type_i = if let N::Type::Fn(params, ret) = ty {
            let wasm_ty = (types_as_wasm(params), option_as_wasm(&**ret));
            let i_check = module.push_signature(
                signature()
                    .with_params(wasm_ty.0.clone())
                    .with_return_type(wasm_ty.1.clone())
                    .build_sig(),
            );
            assert_eq!(*type_indexes.entry(wasm_ty).or_insert(i_check), i_check);
            i_check
        } else {
            panic!("expected all elements in the runtime to have function type");
        };
        rt_indexes.insert(name.clone(), func_i as u32);
        module = module
            .import()
            .path("runtime", name)
            .with_external(External::Function(type_i))
            .build();
    }
    module = module
        .import()
        .path("runtime", "memory")
        .external()
        .memory(0, None)
        .build();
    // Create a WebAssembly function type for each function in NotWasm. These
    // go in the table of types (type_indexes).
    for func in program.functions.values() {
        // has to be wasm types to dedup properly
        let func_ty = (
            types_as_wasm(&func.fn_type.args.clone()),
            option_as_wasm(&func.fn_type.result),
        );
        let next_index = type_indexes.len() as u32;
        type_indexes.entry(func_ty).or_insert(next_index);
    }
    // data segment
    // right now there's only one global, if that changes we can refactor
    module = module
        .import()
        .path("runtime", "JEN_STRINGS")
        .with_external(External::Global(GlobalType::new(ValueType::I32, false)))
        .build();
    let module = module
        .data()
        .offset(GetGlobal(JEN_STRINGS_IDX))
        .value(program.data)
        .build();
    for (i, mut global) in program.globals {
        // can't use functions anyway so no need to worry
        let empty = HashMap::new();
        let empty2 = HashMap::new();
        let mut visitor = Translate::new(&empty, &empty2);
        visitor.translate_atom(&env, &mut global);
        let mut insts = visitor.out;
        insts.push(End);
        // TODO: do a global
    }
    // fsr we need an identity table to call indirect
    let mut table_build = module.table().with_min(program.functions.len() as u32);
    let offset = rt_indexes.len() as u32;
    let mut main_index = None;
    for (notwasm_index, name) in program.functions.keys().enumerate() {
        let wasm_index = notwasm_index as u32 + offset;
        // find main
        if name == &N::Id::Named("main".to_string()) {
            main_index = Some(wasm_index)
        }
        table_build = table_build.with_element(notwasm_index as u32, vec![wasm_index]);
    }
    let mut module = table_build.build();
    for (name, func) in program.functions.iter_mut() {
        module.push_function(translate_func(func, &env, &rt_indexes, &type_indexes, name));
    }
    // export main
    let module = module
        .export()
        .field("main")
        .internal()
        .func(main_index.expect("no main"))
        .build();
    module.build()
}

fn translate_func(
    func: &mut N::Function,
    env: &Env,
    rt_indexes: &HashMap<String, u32>,
    type_indexes: &FuncTypeMap,
    name: &N::Id,
) -> FunctionDefinition {
    let mut translator = Translate::new(rt_indexes, type_indexes);

    // Add indices for parameters
    let mut env = env.clone();
    for (arg_name, arg_typ) in func.params.iter().zip(func.fn_type.args.iter()) {
        let index = translator.next_id;
        translator.next_id += 1;
        env.ids.insert(arg_name.clone(), IdIndex::Local(index, arg_typ.clone()));
    }

    // generate the actual code
    translator.translate_rec(&mut env, &mut func.body);
    let mut insts = translator.out;
    insts.push(End);
    // before the code, if this is main we have to call rt init()
    if name == &N::Id::Named("main".to_string()) {
        let mut new_insts = vec![Call(*rt_indexes.get("init").expect("no init"))];
        new_insts.append(&mut insts);
        insts = new_insts;
    }
    let locals: Vec<_> = translator
        .locals
        .into_iter()
        .map(|t| Local::new(1, t))
        .collect();
    function()
        .signature()
        .with_params(types_as_wasm(&func.fn_type.args))
        .with_return_type(option_as_wasm(&func.fn_type.result))
        .build()
        .body()
        .with_instructions(Instructions::new(insts))
        .with_locals(locals)
        .build()
        .build()
}

fn types_as_wasm(types: &[N::Type]) -> Vec<ValueType> {
    types.iter().map(N::Type::as_wasm).collect()
}
fn option_as_wasm(ty: &Option<N::Type>) -> Option<ValueType> {
    ty.as_ref().map(N::Type::as_wasm)
}

struct Translate<'a> {
    out: Vec<Instruction>,
    rt_indexes: &'a HashMap<String, u32>,
    type_indexes: &'a FuncTypeMap,
    locals: Vec<ValueType>,
    next_id: u32,
}

#[derive(Clone, PartialEq, Default, Debug)]
struct Env {
    labels: im_rc::Vector<TranslateLabel>,
    ids: im_rc::HashMap<N::Id, IdIndex>,
}

impl Env {

    pub fn index_of_id(&self, id: &N::Id) -> u32 {
        match self.ids.get(id).expect(&format!("unbound identifier {:?}", id)) {
            IdIndex::Local(index, _) => *index,
            IdIndex::Fun(_) => panic!("expected local variable")
        }
    }
}


/// We use `TranslateLabel` to compile the named labels and breaks of NotWasm
/// to WebAssembly. In WebAssembly, blocks are not named, and break statements
/// refer to blocks using de Brujin indices (i.e., index zero for the innermost
/// block.  When recurring into a named NotWasm block, we add a 
/// `TranslateLabel::Label` that holds the block's name on to the environment.
/// To compile a `break l` statement to WebAssembly, we scan the labels for
/// the index of `TranslateLabel::Label(l)`. When the compiler introduces an
/// unnamed WebAssembly block, it pushes a `TranslateLabel::Unused` onto the
/// `LabelEnv`, which ensures that indices shift correctly.
#[derive(Clone, PartialEq, Debug)]
enum TranslateLabel {
    Unused,
    Label(N::Label)
}

/// We use `IdIndex` to resolve identifiers that appear in a NotWasm program
/// while compiling to WebAssembly. Before compiling the body of a function, we
/// populate an `IdEnv` to map each function name `f` to its index `n`
/// (`IdIndex::Fun(n)`). 
#[derive(Clone, PartialEq, Debug)]
enum IdIndex {
    Local(u32, N::Type),
    Fun(u32)
}

impl<'a> Translate<'a> {
    fn new(rt_indexes: &'a HashMap<String, u32>, type_indexes: &'a FuncTypeMap) -> Self {
        Self {
            out: Vec::new(),
            rt_indexes,
            type_indexes,
            next_id: 0,
            locals: Vec::new()
        }
    }

    // We are not using a visitor, since we have to perform an operation on every
    // give of statement and expression. Thus, the visitor wouldn't give us much.
    //
    // The `translate_rec` function receives a mutable reference to `Env`, which
    // allows it to introduce new variable declarations. This makes block
    // statements easier to compile, since each statement in a block can alter
    // the environment of its successor. (The alternative would be to have
    // `translate_rec` return a new environment.) However, we have to take care
    // to clone `env` when we enter a new block scope.
    pub(self) fn translate_rec(&mut self, env: &mut Env, stmt: &mut N::Stmt) {
        match stmt {
            N::Stmt::Empty => (),
            N::Stmt::Block(ss) => {
                // don't surround in an actual block, those are only useful
                // when labeled
                let mut inner_env = env.clone();
                for s in ss {
                    self.translate_rec(&mut inner_env, s);
                }
            }
            N::Stmt::Var(id, expr, typ) => {
                // Binds variable in env after compiling expr (prevents
                // circularity).
                self.translate_expr(env, expr);
                let index = self.next_id;
                self.next_id += 1;
                self.locals.push(typ.as_wasm());
                env.ids.insert(id.clone(), IdIndex::Local(index, typ.clone()));
                self.out.push(SetLocal(index));
            }
            N::Stmt::Assign(id, expr) => {
                // place value on stack
                self.translate_expr(env, expr);
                self.out.push(SetLocal(env.index_of_id(id)));
            }
            N::Stmt::If(cond, conseq, alt) => {
                self.translate_atom(env, cond);
                self.out.push(If(BlockType::NoResult));
                let mut env1 = env.clone();
                env1.labels.push_front(TranslateLabel::Unused);
                self.translate_rec(&mut env1, conseq);
                self.out.push(Else);
                let mut env2 = env.clone();
                env2.labels.push_front(TranslateLabel::Unused);
                self.translate_rec(&mut env2, alt);
                self.out.push(End);
            }
            N::Stmt::Loop(body) => {
                // breaks should be handled by surrounding label already
                self.out.push(Loop(BlockType::NoResult));
                let mut env1 = env.clone();
                env1.labels.push_front(TranslateLabel::Unused);
                self.translate_rec(&mut env1, body);
                // loop doesn't automatically continue, don't ask me why
                self.out.push(Br(0));
                self.out.push(End);
            }
            N::Stmt::Label(x, stmt) => {
                if let N::Label::App(_) = x {
                    panic!("Label::App was not elimineted by elim_gotos");
                }
                self.out.push(Block(BlockType::NoResult));
                let mut env1 = env.clone();
                env1.labels.push_front(TranslateLabel::Label(x.clone()));
                self.translate_rec(&mut env1, stmt);
                self.out.push(End);
            }
            N::Stmt::Break(label) => {
                let l = TranslateLabel::Label(label.clone());
                let i = env.labels.index_of(&l).expect(&format!("unbound label {:?}", label));
                self.out.push(Br(i as u32));
            },
            N::Stmt::Return(atom) => {
                self.translate_atom(env, atom);
                self.out.push(Return);
            }
            N::Stmt::Trap => {
                self.out.push(Unreachable);
            }
            N::Stmt::Goto(..) => {
                panic!(
                    "this should be NotWasm, not GotoWasm. did you run elim_gotos? did it work?"
                );
            }
        }
    }

    fn translate_binop(&mut self, op: &N::BinaryOp) {
        match op {
            N::BinaryOp::I32Eq => self.out.push(I32Eq),
            N::BinaryOp::I32Add => self.out.push(I32Add),
            N::BinaryOp::I32Sub => self.out.push(I32Sub),
            N::BinaryOp::I32GT => self.out.push(I32GtS),
            N::BinaryOp::I32Ge => self.out.push(I32GeS),
            N::BinaryOp::I32Le => self.out.push(I32LeS),
            N::BinaryOp::I32Mul => self.out.push(I32Mul),
            N::BinaryOp::I32And => self.out.push(I32And),
            N::BinaryOp::I32Or => self.out.push(I32Or),
        }
    }

    fn translate_expr(&mut self, env: &Env, expr: &mut N::Expr) {
        match expr {
            N::Expr::Atom(atom) => self.translate_atom(env, atom),
            N::Expr::HT(ty) => self.rt_call_mono("ht_new", ty),
            N::Expr::Array(ty) => self.rt_call_mono("array_new", ty),
            N::Expr::HTSet(ht, field, val, ty) => {
                self.translate_atom(env, ht);
                self.translate_atom(env, field);
                self.translate_atom(env, val);
                self.rt_call_mono("ht_set", ty);
            }
            N::Expr::Push(array, val, ty) => {
                self.translate_atom(env, array);
                self.translate_atom(env, val);
                self.rt_call_mono("array_push", ty);
            }
            N::Expr::CallDirect(f, args) => {
                for arg in args {
                    self.out.push(GetLocal(env.index_of_id(arg)));
                }
                match env.ids.get(f) {
                    Some(IdIndex::Fun(i)) => {
                        // this one's a little weird. we index in notwasm
                        // by 0 = first user function. but wasm indexes by 0 =
                        // first rt function. se we have to offset. but only
                        // on direct calls, because our function table takes
                        // care of it on indirect calls
                        self.out.push(Call(i + self.rt_indexes.len() as u32));
                    }
                    _ => panic!("expected Func ID"),
                };
            }
            N::Expr::CallIndirect(func, f_typ, args) => {
                // for arg in args {
                //     self.out.push(GetLocal(arg.index()));
                // }
                panic!("Indirect calls");
                // match func {
                //     N::Id::Index(i) => {
                //         self.out.push(GetLocal(*i));
                //         let index = if let Some(i) = self
                //             .type_indexes
                //             .get(&(types_as_wasm(params_tys), ret_ty.as_wasm()))
                //         {
                //             *i
                //         } else {
                //             panic!("unknown function type");
                //         };
                //         self.out.push(CallIndirect(index, 0));
                //     }
                //     N::Id::Func(i) => {
                //         // this one's a little weird. we index in notwasm
                //         // by 0 = first user function. but wasm indexes by 0 =
                //         // first rt function. se we have to offset. but only
                //         // on direct calls, because our function table takes
                //         // care of it on indirect calls
                //         self.out.push(Call(*i + self.rt_indexes.len() as u32));
                //     }
                //     _ => panic!("id can't be function call"),
                // }
            }
            N::Expr::ToString(a) => {
                self.translate_atom(env, a);
                self.rt_call("string_from_str");
            }
        }
    }

    fn translate_atom(&mut self, env: &Env, atom: &mut N::Atom) {
        match atom {
            N::Atom::Lit(lit) => match lit {
                N::Lit::I32(i) => self.out.push(I32Const(*i)),
                N::Lit::Interned(addr) => {
                    self.out.push(GetGlobal(JEN_STRINGS_IDX));
                    self.out.push(I32Const(*addr as i32));
                    self.out.push(I32Add);
                }
                N::Lit::String(..) => panic!("uninterned string"),
                N::Lit::Bool(b) => self.out.push(I32Const(*b as i32)),
                _ => todo!(),
            },
            N::Atom::Id(id) => match env.ids.get(id).expect(&format!("unbound identifier {:?} {:?}", id, env)) {
                IdIndex::Local(n, _) => self.out.push(GetLocal(*n)),
                IdIndex::Fun(n) => self.out.push(I32Const(*n as i32)),
            },
            N::Atom::HTGet(ht, field, ty) => {
                self.translate_atom(env, ht);
                self.translate_atom(env, field);
                self.rt_call_mono("ht_get", ty);
            }
            N::Atom::Index(ht, index, ty) => {
                self.translate_atom(env, ht);
                self.translate_atom(env, index);
                self.rt_call_mono("array_index", ty);
            }
            N::Atom::StringLen(string) => {
                self.translate_atom(env, string);
                self.rt_call("string_len");
            }
            N::Atom::Binary(op, a, b) => {
                self.translate_atom(env, a);
                self.translate_atom(env, b);
                self.translate_binop(op);
            }
        }
    }
    fn rt_call(&mut self, name: &str) {
        if let Some(i) = self.rt_indexes.get(name) {
            self.out.push(Call(*i));
        } else {
            panic!("cannot find rt {}", name);
        }
    }
    /// call a monomorphized function, which follows specific naming
    /// conventions: `name_type`, with type given by [N::Type::fmt]
    fn rt_call_mono(&mut self, name: &str, ty: &N::Type) {
        self.rt_call(&format!("{}_{}", name, ty));
    }
}

impl N::Type {
    pub fn as_wasm(&self) -> ValueType {
        match self {
            // NOTE(arjun): We do not need to support I64, since JavaScript cannot
            // natively represent 64-bit integers.
            N::Type::F64 => ValueType::F64,
            // almost everything is a pointer type
            _ => ValueType::I32,
        }
    }
}
