use super::ast::{Ast, BinOp, UnOp};
use super::builtins::Builtin;
use super::{ExprError, Scope};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    Num(f64),
    Var(u16),
    Neg,
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
    Call(Builtin, u8),
    NodeProp(u16),
    Ternary(Box<[Op]>, Box<[Op]>, Box<[Op]>),
}

pub(crate) struct Compiled {
    pub ops: Box<[Op]>,
    pub var_names: Box<[Box<str>]>,
    pub node_refs: Box<[(Box<str>, Box<str>)]>,
    pub free_vars: Vec<String>,
    pub is_static: bool,
}

fn is_dynamic_var_name(name: &str) -> bool {
    matches!(name, "t" | "T" | "beat" | "duration")
}

struct Compiler {
    var_names: Vec<Box<str>>,
    node_refs: Vec<(Box<str>, Box<str>)>,
    has_node_ref: bool,
}

impl Compiler {
    fn var_idx(&mut self, name: &str) -> u16 {
        if let Some(pos) = self.var_names.iter().position(|n| n.as_ref() == name) {
            pos as u16
        } else {
            self.var_names.push(name.into());
            (self.var_names.len() - 1) as u16
        }
    }

    fn node_idx(&mut self, id: &str, prop: &str) -> u16 {
        if let Some(pos) = self
            .node_refs
            .iter()
            .position(|(i, p)| i.as_ref() == id && p.as_ref() == prop)
        {
            pos as u16
        } else {
            self.node_refs.push((id.into(), prop.into()));
            (self.node_refs.len() - 1) as u16
        }
    }

    fn compile_ast(&mut self, ast: &Ast) -> Vec<Op> {
        match ast {
            Ast::Num(n) => vec![Op::Num(*n)],
            Ast::Var(name) => vec![Op::Var(self.var_idx(name))],
            Ast::Unary(UnOp::Neg, inner) => {
                let mut ops = self.compile_ast(inner);
                ops.push(Op::Neg);
                ops
            }
            Ast::Bin(op, l, r) => {
                let mut ops = self.compile_ast(l);
                ops.extend(self.compile_ast(r));
                ops.push(match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Rem => Op::Rem,
                    BinOp::Eq => Op::Eq,
                    BinOp::Ne => Op::Ne,
                    BinOp::Lt => Op::Lt,
                    BinOp::Le => Op::Le,
                    BinOp::Gt => Op::Gt,
                    BinOp::Ge => Op::Ge,
                });
                ops
            }
            Ast::Ternary(cond, then_b, else_b) => {
                let cond_ops = self.compile_ast(cond).into_boxed_slice();
                let then_ops = self.compile_ast(then_b).into_boxed_slice();
                let else_ops = self.compile_ast(else_b).into_boxed_slice();
                vec![Op::Ternary(cond_ops, then_ops, else_ops)]
            }
            Ast::Call(builtin, args) => {
                let mut ops = Vec::new();
                for a in args {
                    ops.extend(self.compile_ast(a));
                }
                ops.push(Op::Call(*builtin, args.len() as u8));
                ops
            }
            Ast::NodeRef(id, prop) => {
                self.has_node_ref = true;
                vec![Op::NodeProp(self.node_idx(id, prop))]
            }
        }
    }
}

pub(crate) fn compile(ast: &Ast) -> Compiled {
    let mut compiler = Compiler {
        var_names: Vec::new(),
        node_refs: Vec::new(),
        has_node_ref: false,
    };
    let ops = compiler.compile_ast(ast).into_boxed_slice();
    let free_vars: Vec<String> = compiler.var_names.iter().map(|n| n.to_string()).collect();
    let is_static = !compiler.has_node_ref && !free_vars.iter().any(|n| is_dynamic_var_name(n));
    Compiled {
        ops,
        var_names: compiler.var_names.into_boxed_slice(),
        node_refs: compiler.node_refs.into_boxed_slice(),
        free_vars,
        is_static,
    }
}

const MAX_STACK: usize = 256;

pub(crate) fn run(
    ops: &[Op],
    var_names: &[Box<str>],
    node_refs: &[(Box<str>, Box<str>)],
    scope: &dyn Scope,
) -> Result<f64, ExprError> {
    let mut stack = [0.0_f64; MAX_STACK];
    let mut sp = 0usize;

    macro_rules! push {
        ($v:expr) => {{
            if sp >= MAX_STACK {
                return Err(ExprError::TooDeep(MAX_STACK));
            }
            stack[sp] = $v;
            sp += 1;
        }};
    }
    macro_rules! pop {
        () => {{
            sp -= 1;
            stack[sp]
        }};
    }

    for op in ops {
        match op {
            Op::Num(n) => push!(*n),
            Op::Var(idx) => {
                let name = &var_names[*idx as usize];
                let v = scope
                    .var(name)
                    .ok_or_else(|| ExprError::UnknownIdent(name.to_string()))?;
                push!(v);
            }
            Op::Neg => {
                let a = pop!();
                push!(-a);
            }
            Op::Add => {
                let b = pop!();
                let a = pop!();
                push!(a + b);
            }
            Op::Sub => {
                let b = pop!();
                let a = pop!();
                push!(a - b);
            }
            Op::Mul => {
                let b = pop!();
                let a = pop!();
                push!(a * b);
            }
            Op::Div => {
                let b = pop!();
                let a = pop!();
                push!(a / b);
            }
            Op::Rem => {
                let b = pop!();
                let a = pop!();
                push!(a % b);
            }
            Op::Eq => {
                let b = pop!();
                let a = pop!();
                push!(if a == b { 1.0 } else { 0.0 });
            }
            Op::Ne => {
                let b = pop!();
                let a = pop!();
                push!(if a != b { 1.0 } else { 0.0 });
            }
            Op::Lt => {
                let b = pop!();
                let a = pop!();
                push!(if a < b { 1.0 } else { 0.0 });
            }
            Op::Le => {
                let b = pop!();
                let a = pop!();
                push!(if a <= b { 1.0 } else { 0.0 });
            }
            Op::Gt => {
                let b = pop!();
                let a = pop!();
                push!(if a > b { 1.0 } else { 0.0 });
            }
            Op::Ge => {
                let b = pop!();
                let a = pop!();
                push!(if a >= b { 1.0 } else { 0.0 });
            }
            Op::Call(builtin, argc) => {
                let n = *argc as usize;
                let mut args = [0.0_f64; 3];
                for slot in args.iter_mut().take(n).rev() {
                    *slot = pop!();
                }
                push!(builtin.call(&args[..n]));
            }
            Op::NodeProp(idx) => {
                let (id, prop) = &node_refs[*idx as usize];
                let v = scope.node_prop(id, prop).ok_or_else(|| {
                    ExprError::UnknownIdent(format!("node(\"{id}\", \"{prop}\")"))
                })?;
                push!(v);
            }
            Op::Ternary(cond_ops, then_ops, else_ops) => {
                let cond = run(cond_ops, var_names, node_refs, scope)?;
                let v = if cond != 0.0 {
                    run(then_ops, var_names, node_refs, scope)?
                } else {
                    run(else_ops, var_names, node_refs, scope)?
                };
                push!(v);
            }
        }
    }

    Ok(pop!())
}
