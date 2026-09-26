//! Lowers an [`Ast`] into a flat program of [`Op`]s once, and runs that
//! program against a [`Scope`] as many times as needed afterwards.
//!
//! This is the piece that makes the two-tier evaluation model in the
//! [`crate::expr`] module doc actually cheap: [`compile`] walks the tree a
//! single time, at [`crate::expr::Expr::parse`] time, and everything it
//! produces — the op list, the interned variable-name table, the interned
//! node-reference table — is owned by the resulting [`Expr`](super::Expr)
//! and never rebuilt. [`run`] then executes that fixed program using a
//! stack-allocated `[f64; MAX_STACK]` array: no `Vec`, no `String`, no heap
//! traffic of any kind on the per-frame path, however many times a frame
//! loop calls it.
//!
//! Arithmetic (`Num`, `Var`, unary/binary operators, builtin calls) compiles
//! to genuinely flat, linear bytecode executed by a simple push/pop
//! interpreter over that array — a real stack machine for the part of the
//! grammar that has no branching. [`Op::Ternary`] is the one construct that
//! must *not* evaluate eagerly (a condition guards a division by zero, or a
//! branch none reads a node that doesn't exist this frame — evaluating both
//! sides unconditionally would surface an error the author's own branching
//! was written to avoid), so it holds its three arms as their own
//! independently-compiled flat programs and `run` recurses into exactly one
//! of them. That recursion is bounded by the same nesting cap `parser.rs`
//! already enforces at parse time, so it can never run deeper than
//! `MAX_DEPTH` stack frames — nowhere near enough to threaten the native
//! stack.

use super::ast::{Ast, BinOp, UnOp};
use super::builtins::Builtin;
use super::{ExprError, Scope};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    Num(f64),
    /// Index into the compiled [`Expr`](super::Expr)'s variable-name table.
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
    /// Pops `argc` values (in argument order), pushes one result.
    Call(Builtin, u8),
    /// Index into the compiled [`Expr`](super::Expr)'s node-reference table.
    NodeProp(u16),
    /// `(cond, then, else)`, each its own flat program — see the module doc
    /// on why this can't be three ordinary operands on the value stack.
    Ternary(Box<[Op]>, Box<[Op]>, Box<[Op]>),
}

pub(crate) struct Compiled {
    pub ops: Box<[Op]>,
    pub var_names: Box<[Box<str>]>,
    pub node_refs: Box<[(Box<str>, Box<str>)]>,
    pub free_vars: Vec<String>,
    pub is_static: bool,
}

/// Names that can never be folded at load time because their value is only
/// known once the frame loop actually starts (or, for `duration`, because
/// computing it soundly at this stage would mean re-deriving the transition
/// arithmetic `crates/rustmotion/src/encode` owns — see
/// `crates/rustmotion/src/loader.rs`'s fold pass for where that arithmetic
/// actually lives). An expression naming any of these is never static,
/// regardless of what else it references.
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

/// Bound on the value stack `run` uses. Every op is compiled from an AST
/// whose nesting is itself capped at parse time (`parser::MAX_DEPTH`, 64),
/// and no single grammar construct pushes more than a small constant number
/// of pending values per nesting level, so this is never approached by any
/// expression that made it past `parse` — it exists purely so a bug in that
/// invariant fails loudly (an `ExprError`) instead of indexing out of
/// bounds.
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
                let mut args = [0.0_f64; 3]; // max builtin arity is 3 (clamp/lerp/smoothstep)
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
