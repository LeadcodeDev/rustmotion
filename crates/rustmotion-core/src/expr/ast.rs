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
    Var(String),
    Unary(UnOp, Box<Ast>),
    Bin(BinOp, Box<Ast>, Box<Ast>),
    Ternary(Box<Ast>, Box<Ast>, Box<Ast>),
    Call(Builtin, Vec<Ast>),
    NodeRef(String, String),
}
