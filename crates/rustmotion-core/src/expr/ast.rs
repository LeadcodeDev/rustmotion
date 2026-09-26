//! The parse tree [`super::parser`] builds and [`super::eval::compile`]
//! lowers into the flat program actually executed.
//!
//! Kept as a plain, owned tree (not a flat token slice with indices) because
//! it only ever exists transiently, inside [`crate::expr::Expr::parse`]:
//! built once, walked once by the compiler, then dropped. Nothing here is
//! stored on [`crate::expr::Expr`] — see that struct's doc for why it keeps
//! the compiled program instead.

use super::builtins::Builtin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ast {
    Num(f64),
    /// A `$name` reference, resolved against a [`super::Scope`] at eval time
    /// — the only kind of identifier this grammar defers past parse time.
    Var(String),
    Unary(UnOp, Box<Ast>),
    Bin(BinOp, Box<Ast>, Box<Ast>),
    /// `cond ? then : else`. Kept as its own node (not desugared into
    /// something else) because it is the one place evaluation must *not*
    /// walk both children — see `eval::Op::Ternary`'s doc for why that
    /// matters beyond performance.
    Ternary(Box<Ast>, Box<Ast>, Box<Ast>),
    /// A builtin call. The callee is already resolved to a fixed-arity
    /// [`Builtin`] by the parser — there is no other kind of call.
    Call(Builtin, Vec<Ast>),
    /// `node("id", "prop")` — resolved against [`super::Scope::node_prop`]
    /// at eval time. The two arguments are string literals, not
    /// sub-expressions: a node id/prop name is never itself computed.
    NodeRef(String, String),
}
