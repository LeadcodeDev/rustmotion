mod ast;
mod builtins;
mod eval;
mod lexer;
mod parser;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait Scope {
    fn var(&self, name: &str) -> Option<f64>;

    fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
        let _ = (id, prop);
        None
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExprError {
    #[error("cannot parse expression `{src}`: {reason}")]
    Parse { src: String, reason: String },
    #[error("unknown identifier `{0}`")]
    UnknownIdent(String),
    #[error("`{name}` takes {expected} argument(s), got {got}")]
    Arity {
        name: String,
        expected: usize,
        got: usize,
    },
    #[error("expression nests deeper than {0} levels")]
    TooDeep(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    ops: Box<[eval::Op]>,
    var_names: Box<[Box<str>]>,
    node_refs: Box<[(Box<str>, Box<str>)]>,
    free_vars: Vec<String>,
    is_static: bool,
}

impl Expr {
    pub fn parse(src: &str) -> Result<Self, ExprError> {
        let body = strip_expr_prefix(src);
        let tree = parser::parse(body)?;
        let compiled = eval::compile(&tree);
        Ok(Expr {
            ops: compiled.ops,
            var_names: compiled.var_names,
            node_refs: compiled.node_refs,
            free_vars: compiled.free_vars,
            is_static: compiled.is_static,
        })
    }

    pub fn eval(&self, scope: &dyn Scope) -> Result<f64, ExprError> {
        eval::run(&self.ops, &self.var_names, &self.node_refs, scope)
    }

    pub fn free_vars(&self) -> Vec<String> {
        self.free_vars.clone()
    }

    pub fn is_static(&self) -> bool {
        self.is_static
    }
}

fn strip_expr_prefix(src: &str) -> &str {
    let trimmed = src.trim_start();
    match trimmed.strip_prefix('=') {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    }
}

/// A scenario value that is either a plain literal or, prefixed with `=`,
/// an expression to evaluate.
///
/// `#[serde(untagged)]`: deserialization tries `Literal(T)` first, falling
/// back to `Expr(String)` only when the JSON value doesn't fit `T`. That
/// ordering is why this works cleanly for the numbers and strictly-typed
/// colours this issue targets — a JSON string can never satisfy a numeric
/// `T`, so `"= $W/2"` always falls through to `Expr` — but is a deliberate
/// non-goal for `Computed<String>`: there, *every* JSON string
/// (`"= ..."` included) already satisfies `Literal(String)` first and
/// `Expr` is never reached. Schema fields that want expression support are
/// expected to use a strictly-typed `T` (a number, or a colour type with
/// its own validating `Deserialize`), not a bare `String`.
///
/// This type carries no evaluation logic of its own — resolving an
/// `Expr(String)` variant means calling [`Expr::parse`] on its contents and
/// then [`Expr::eval`] (or folding it statically, see the [`crate::expr`]
/// module doc) — deliberately, so this crate does not have to hand back an
/// opinion on *when* that happens; that is a decision for whatever owns the
/// field (the static-folding pass, or the per-frame engine context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Computed<T> {
    Literal(T),
    Expr(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{PI, TAU};

    struct MapScope<'a>(&'a [(&'a str, f64)]);
    impl Scope for MapScope<'_> {
        fn var(&self, name: &str) -> Option<f64> {
            self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
        }
    }

    struct EmptyScope;
    impl Scope for EmptyScope {
        fn var(&self, _name: &str) -> Option<f64> {
            None
        }
    }

    #[test]
    fn parses_with_and_without_equals_prefix() {
        let a = Expr::parse("= 1 + 1").unwrap();
        let b = Expr::parse("1 + 1").unwrap();
        assert_eq!(a.eval(&EmptyScope).unwrap(), 2.0);
        assert_eq!(b.eval(&EmptyScope).unwrap(), 2.0);
    }

    #[test]
    fn plain_arithmetic() {
        let expr = Expr::parse("= (2 + 3) * 4 - 10 / 2 % 3").unwrap();
        assert_eq!(expr.eval(&EmptyScope).unwrap(), 18.0);
    }

    #[test]
    fn ternary_short_circuits() {
        let expr = Expr::parse("= 1 > 0 ? 5 : (1 / 0)").unwrap();
        assert_eq!(expr.eval(&EmptyScope).unwrap(), 5.0);
    }

    #[test]
    fn ternary_untaken_branch_errors_are_not_raised() {
        let expr = Expr::parse("= 1 < 0 ? $missing : 42").unwrap();
        assert_eq!(expr.eval(&EmptyScope).unwrap(), 42.0);
    }

    #[test]
    fn constants_resolve() {
        let expr = Expr::parse("= PI + TAU + E").unwrap();
        assert!((expr.eval(&EmptyScope).unwrap() - (PI + TAU + std::f64::consts::E)).abs() < 1e-12);
    }

    #[test]
    fn free_vars_and_is_static() {
        let dynamic = Expr::parse("= $t * 2").unwrap();
        assert_eq!(dynamic.free_vars(), vec!["t".to_string()]);
        assert!(!dynamic.is_static());

        let static_expr = Expr::parse("= $W / 2 + $i").unwrap();
        assert_eq!(
            static_expr.free_vars(),
            vec!["W".to_string(), "i".to_string()]
        );
        assert!(static_expr.is_static());

        let duration_expr = Expr::parse("= $duration").unwrap();
        assert!(!duration_expr.is_static());
    }

    #[test]
    fn node_ref_is_never_static() {
        let expr = Expr::parse("= node(\"badge_0\", \"x\") + 1").unwrap();
        assert!(!expr.is_static());
        assert!(expr.free_vars().is_empty());
    }

    #[test]
    fn node_ref_resolves_through_scope() {
        struct NodeScope;
        impl Scope for NodeScope {
            fn var(&self, _name: &str) -> Option<f64> {
                None
            }
            fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
                if id == "badge_0" && prop == "x" {
                    Some(100.0)
                } else {
                    None
                }
            }
        }
        let expr = Expr::parse("= node(\"badge_0\", \"x\") + 1").unwrap();
        assert_eq!(expr.eval(&NodeScope).unwrap(), 101.0);
    }

    #[test]
    fn unknown_var_errors_at_eval() {
        let expr = Expr::parse("= $nope").unwrap();
        let err = expr.eval(&EmptyScope).unwrap_err();
        assert_eq!(err, ExprError::UnknownIdent("nope".to_string()));
    }

    #[test]
    fn eight_badges_on_a_circle_match_real_cosines() {
        let count = 8;
        for i in 0..count {
            let scope = MapScope(&[("i", i as f64), ("count", count as f64), ("W", 1080.0)]);
            let expr = Expr::parse("= $W/2 + cos($i / $count * TAU - PI/2) * 700").unwrap();
            assert!(expr.is_static());
            let got = expr.eval(&scope).unwrap();
            let want = 1080.0 / 2.0 + (i as f64 / count as f64 * TAU - PI / 2.0).cos() * 700.0;
            assert!(
                (got - want).abs() < 1e-9,
                "badge {i}: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn rand_is_deterministic_across_separate_evaluations() {
        let expr = Expr::parse("= rand(42)").unwrap();
        let a = expr.eval(&EmptyScope).unwrap();
        let b = expr.eval(&EmptyScope).unwrap();
        assert_eq!(a, b);
        let c = Expr::parse("= rand(42)")
            .unwrap()
            .eval(&EmptyScope)
            .unwrap();
        assert_eq!(a, c);
    }

    #[test]
    fn deeply_nested_expression_is_rejected_at_parse() {
        let src = format!("={}1{}", "(".repeat(500), ")".repeat(500));
        let err = Expr::parse(&src).unwrap_err();
        assert!(matches!(err, ExprError::TooDeep(_)), "got {err:?}");
    }

    #[test]
    fn huge_exponent_chain_is_rejected_at_parse() {
        let mut body = "2".to_string();
        for _ in 0..500 {
            body = format!("pow({body}, 2)");
        }
        let src = format!("= {body}");
        let err = Expr::parse(&src).unwrap_err();
        assert!(matches!(err, ExprError::TooDeep(_)), "got {err:?}");
    }

    #[test]
    fn computed_untagged_literal_vs_expr_for_numeric_t() {
        let lit: Computed<f64> = serde_json::from_str("42.5").unwrap();
        assert_eq!(lit, Computed::Literal(42.5));

        let expr: Computed<f64> = serde_json::from_str("\"= $W/2\"").unwrap();
        assert_eq!(expr, Computed::Expr("= $W/2".to_string()));
    }

    #[test]
    fn computed_round_trips_through_serde() {
        let lit: Computed<f64> = Computed::Literal(10.0);
        let json = serde_json::to_string(&lit).unwrap();
        assert_eq!(json, "10.0");

        let expr: Computed<f64> = Computed::Expr("= 1 + 1".to_string());
        let json = serde_json::to_string(&expr).unwrap();
        assert_eq!(json, "\"= 1 + 1\"");
    }

    #[test]
    fn a_static_expression_over_for_each_bindings_folds_at_load() {
        struct Fixed;
        impl Scope for Fixed {
            fn var(&self, name: &str) -> Option<f64> {
                match name {
                    "W" => Some(1080.0),
                    "i" => Some(3.0),
                    "count" => Some(8.0),
                    _ => None,
                }
            }
        }

        let expr = Expr::parse("= $W/2 + cos($i / $count * TAU) * 700").unwrap();
        assert!(expr.is_static());
        let x = expr.eval(&Fixed).unwrap();
        let want = 1080.0 / 2.0 + (3.0_f64 / 8.0 * std::f64::consts::TAU).cos() * 700.0;
        assert!((x - want).abs() < 1e-9);
    }
}
