#![allow(dead_code, unused_imports)]
use super::syntax::*;

use std::collections::HashMap;
use combine_language::{LanguageDef, LanguageEnv, Identifier};
use combine::error::ParseError;
use combine::parser;
use combine::parser::char::{alpha_num, char, digit, letter, spaces, string};
use combine::stream::easy;
use combine::stream::Stream;
use combine::stream::state::State;

use combine::{
    attempt, between, eof, many, many1, optional, satisfy_map, sep_end_by, token, Parser,
    sep_by, value, chainl1, satisfy
};
use std::fmt;

type Lang<'a, I> = LanguageEnv<'a, I>;

parser! {
    fn lit['a, 'b, I](lang: &'b Lang<'a, I>)(I) -> Lit
    where [I: Stream<Item = char>]
    {
        lang.integer().map(|n| Lit::I32(n as i32))
        .or(lang.reserved("true").with(value(Lit::Bool(true))))
        .or(lang.reserved("false").with(value(Lit::Bool(false))))

    }
}

parser! {
    fn id['a, 'b, I](lang: &'b Lang<'a, I>)(I) -> Id
    where [I: Stream<Item = char>]
    {
        lang.identifier().map(|x| Id::Named(x.to_string()))
    }
}

parser! {
    fn binop['a, 'b, I](lang: &'b Lang<'a, I>)(I) -> BinaryOp
    where [I: Stream<Item = char>]
    {
        lang.reserved_op("+").with(value(BinaryOp::I32Add))
        .or(lang.reserved_op(">").with(value(BinaryOp::I32GT)))
        .or(lang.reserved_op("-").with(value(BinaryOp::I32Eq)))
        .or(lang.reserved_op("==").with(value(BinaryOp::I32Sub)))
    }
}

parser! {
    fn atom['a, 'b, I](lang: &'b Lang<'a, I>)(I) -> Atom
    where [I: Stream<Item = char>]
    {
        chainl1(
            atom_item(lang),
            binop(lang).map(|op| move |lhs, rhs| Atom::Binary(op, Box::new(lhs), Box::new(rhs), Type::I32)))
    }    
}

parser! {
    fn atom_item['a, 'b, I](lang: &'b Lang<'a, I>)(I) -> Atom
    where [I: Stream<Item = char>]
    {
        lit(lang).map(|l| Atom::Lit(l))
        .or(id(lang).map(|x| Atom::Id(x)))
        .or(lang.parens(atom(lang)))
    }
}

enum AfterId {
    Args(Vec<Id>),
    Op(BinaryOp, Atom)
}

parser! {
    fn expr['a, 'b, I](lang: &'b Lang<'a, I>)(I) ->  Expr
    where [ I: Stream<Item = char>]
    {
        id(lang)
        .and(optional(lang.parens(sep_by(id(lang), lang.reserved_op(","))).map(|args| AfterId::Args(args))
            .or(binop(lang).and(atom(lang)).map(|(op, e)| AfterId::Op(op, e)))))
        .map(|(f, opt_args)| match opt_args {
            None => Expr::Atom(Atom::Id(f)),
            Some(AfterId::Args(args)) => Expr::CallDirect(f, args),
            Some(AfterId::Op(op, e)) => Expr::Atom(Atom::Binary(op, Box::new(Atom::Id(f)), Box::new(e), Type::I32))
        })
        .or(atom(lang).map(|a| Expr::Atom(a)))
    }
}

parser! {
    fn type_['a, 'b, I](lang: &'b Lang<'a, I>)(I) -> Type
    where [ I: Stream<Item = char>]
    {
        lang.reserved("i32").with(value(Type::I32))
    }
}

parser! {
    fn stmt['a, 'b, I](lang: &'b Lang<'a,I>)(I) -> Stmt
    where [I: Stream<Item = char>]
    {
        let var = lang.reserved("var")
            .with(id(lang))
            .skip(lang.reserved_op(":"))
            .and(type_(lang))
            .skip(lang.reserved_op("="))
            .and(expr(lang))
            .skip(lang.reserved_op(";"))
            .map(|((x,t),e)| {
                let x = Stmt::Var(x, e, t);
                println!("{:?}", x);
                x
            });

        let assign = id(lang)
            .skip(lang.reserved_op("="))
            .and(expr(lang))
            .skip(lang.reserved_op(";"))
            .map(|(x,e)| Stmt::Assign(x, e));

        let if_ = lang.reserved("if")
            .with(lang.parens(atom(lang)))
            .and(block(lang))
            .skip(lang.reserved("else"))
            .and(block(lang))
            .map(|((a, s1), s2)| Stmt::If(a, Box::new(s1), Box::new(s2)));
        
        let loop_ = lang.reserved("loop")
            .with(block(lang))
            .map(|s| Stmt::Loop(Box::new(s)));

        let return_ = lang.reserved("return")
            .with(atom(lang))
            .skip(lang.reserved_op(";"))
            .map(|a| Stmt::Return(a));

        let break_ = lang.reserved("break")
            .with(id(lang))
            .skip(lang.reserved_op(";"))
            .map(|l| Stmt::Break(l));

        var.or(if_).or(return_).or(block(lang)).or(loop_).or(break_).or(assign)
    }
}

parser! {
    fn block['a, 'b, I](lang: &'b Lang<'a,I>)(I) -> Stmt
    where [I: Stream<Item = char>]
    {
        lang.braces(many(stmt(lang)))
            .map(|ss| Stmt::Block(ss))
    }
}

parser! {
    fn function['a, 'b, I](lang: &'b Lang<'a,I>)(I) -> (Id, Function)
    where [I: Stream<Item = char>]
    {
        lang.reserved("function")
        .with(id(lang))
        .and(lang.parens(
            sep_by(id(lang).skip(lang.reserved_op(":")).and(type_(lang)), lang.reserved_op(","))))
        .skip(lang.reserved_op(":"))
        .and(type_(lang))
        .and(block(lang))
        .map(|(((f, params_tys), ret_ty), body)| 
        (f, Function { locals: vec![], body, params_tys, ret_ty }))
    }
}

parser! {
    fn program['a, 'b, I](lang: &'b Lang<'a,I>)(I) -> Program
    where [I: Stream<Item = char>]
    {
        lang.white_space()
        .with(many::<HashMap<_, _>, _>(function(lang)))
        .skip(eof())
        .map(|functions| {
            let classes = HashMap::new();
            let globals = HashMap::new();
            Program { functions, classes, globals }
        })
    }
}

pub fn parse(input: &str) -> Program {
    // NOTE(arjun): It would be nice to extract this language definition into
    // a function. But, I have no idea what its type should be, since the
    // input type is not &str, but a "stream" (defined in the combine library).
    let lang = LanguageEnv::new(LanguageDef {
        ident: Identifier {
            start: letter(),
            rest: alpha_num(),
            reserved: ["if", "else", "true", "false", "function", "loop", 
                "return", "i32" ]
                .iter()
                .map(|x| (*x).into())
                .collect(),
        },
        // NOTE(arjun): This is from the combine-language documentation. I have
        // not bothered to understand it. But, it ought to define a pattern that
        // matches operators. Our operators are quite straightforward.
        op: Identifier {
            start: satisfy(|c| "+-*/".chars().any(|x| x == c)),
            rest: satisfy(|c| "+-*/".chars().any(|x| x == c)),
            reserved: ["+", "-", "*", "/"].iter().map(|x| (*x).into()).collect()
        },
        comment_start: string("/*").map(|_| ()),
        comment_end: string("*/").map(|_| ()),
        comment_line: string("//").map(|_| ()),
    });

    let mut parser = program(&lang);
    let input_stream = State::new(input);
    let p = parser.easy_parse(input_stream).expect("parsing error");
    return p.0;
}

