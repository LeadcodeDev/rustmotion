//! Arithmetic expressions for scenario values.
//!
//! Without this module, every number in a scenario is a frozen literal: a
//! generator that wants eight badges evenly spaced on a circle, or a facet
//! shaded by its angle to a light, has no way to say so in JSON and instead
//! has to compute the numbers itself, offline, and paste the results in —
//! unreadable in the studio, and no longer connected to the intent that
//! produced them. This module gives a scenario value an alternative to
//! being a literal: a small, deliberately not Turing-complete expression
//! language, written as a string.
//!
//! # The `=` prefix
//!
//! A JSON string is an expression only when it starts with `=`; anything
//! else is a plain literal string, exactly as before:
//!
//! ```json
//! { "x": "= $W/2 + cos($i / $count * TAU - PI/2) * 700 - 110" }
//! ```
//!
//! [`Expr::parse`] itself is tolerant of either form — a leading `=` (with
//! optional surrounding whitespace) is stripped if present, and the rest is
//! parsed as-is otherwise — so callers can hand it either the raw JSON
//! string value or just the expression body without having to agree in two
//! places on who strips the sigil. The decision of *whether* a given string
//! is even attempted as an expression (i.e. whether it starts with `=` at
//! all) is made by the caller — see `crates/rustmotion/src/loader.rs`'s
//! static-folding pass, and eventually [`Computed`] below for the typed
//! schema fields that wrap this.
//!
//! # Grammar
//!
//! Numeric literals, `+ - * / %`, unary minus, parentheses, the six
//! comparisons (`== != < <= > >=`, each producing `0.0`/`1.0`), a ternary
//! `cond ? a : b`, and calls to a fixed, closed set of builtins (see
//! [`builtins::Builtin`]) plus the constants `PI`, `TAU`, `E`. No loops, no
//! user-defined functions, no recursion — see [`ExprError::TooDeep`] for
//! the nesting cap this buys: a hostile expression fails at *parse*, never
//! hangs a render.
//!
//! A `$name` token is a reference resolved against a [`Scope`] at
//! evaluation time — never at parse time — the one place this grammar looks
//! outside itself. `node("id", "prop")` is the other: a node-reference call
//! resolved via [`Scope::node_prop`], syntactically distinguished from an
//! ordinary builtin call because its two arguments are string literals
//! (an id and a property name), never sub-expressions.
//!
//! # Two evaluation tiers
//!
//! An [`Expr`] is parsed exactly once, in [`Expr::parse`], into a compiled
//! program (see [`eval`] for the compiler and the allocation-free stack
//! machine that runs it) — evaluating it later, however many times, never
//! re-parses the source string. What happens with that compiled `Expr` then
//! splits in two:
//!
//! - **Static fold, at load.** [`Expr::is_static`] is true when
//!   [`Expr::free_vars`] names nothing time-varying (`t`, `T`, `beat`,
//!   `duration` — see [`eval::is_dynamic_var_name`]) and the expression
//!   contains no `node(...)` call. Such an expression can only ever
//!   evaluate to one value for a given document, so it is evaluated once
//!   during loading and the JSON literal it produces replaces it — zero
//!   per-frame cost, and the reason a `for-each` placing eight badges on a
//!   circle costs nothing more at render time than eight literal
//!   coordinates would have. See `crates/rustmotion/src/loader.rs`.
//! - **Per frame, otherwise.** An expression reading `$t`/`$T`/`$beat`, a
//!   [`Scope::var`]-backed animated variable, or a `node(...)` reference is
//!   evaluated once per sampled frame against that frame's [`Scope`] — still
//!   without re-parsing, and without allocating, since [`Expr::eval`] only
//!   ever walks the already-compiled program.
//!
//! # Scope
//!
//! [`Scope::var`] is the single hook a caller implements to expose names to
//! an expression. This module does not itself decide what `$t`, `$W`, or an
//! arbitrary `$myVar` mean — it only defines the two categories
//! [`Expr::is_static`] treats specially (dynamic vs. potentially
//! load-time-known) so that callers building a [`Scope`] for either tier
//! know which names they are expected to answer. The scenario-level names
//! this workstream's issue names are `$t` (scene time), `$T` (absolute
//! time), `$beat`, `$i`/`$index`/`$item`/`$count` (bound by a surrounding
//! `for-each`), `$W`/`$H`/`$fps`/`$duration`, plus arbitrary `$name` for a
//! declared `config` variable or an animated variable (`Scope::var`) — the
//! same `$name` sigil `crates/rustmotion/src/variables.rs` already resolves
//! for whole-value and interpolated substitution, deliberately reused
//! rather than inventing a second one.
//!
//! ```
//! use rustmotion_core::expr::{Expr, Scope};
//!
//! struct Fixed;
//! impl Scope for Fixed {
//!     fn var(&self, name: &str) -> Option<f64> {
//!         match name {
//!             "W" => Some(1080.0),
//!             "i" => Some(3.0),
//!             "count" => Some(8.0),
//!             _ => None,
//!         }
//!     }
//! }
//!
//! let expr = Expr::parse("= $W/2 + cos($i / $count * TAU) * 700").unwrap();
//! assert!(expr.is_static());
//! let x = expr.eval(&Fixed).unwrap();
//! assert!((x - (1080.0 / 2.0 + (3.0_f64 / 8.0 * std::f64::consts::TAU).cos() * 700.0)).abs() < 1e-9);
//! ```

mod ast;
mod builtins;
mod eval;
mod lexer;
mod parser;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What an expression can read from its evaluation context: named
/// variables, and optionally another node's already-resolved property.
///
/// Implemented by whoever is evaluating an [`Expr`] — the load-time static
/// folder (`crates/rustmotion/src/loader.rs`, a handful of reserved names
/// only), and eventually the per-frame engine context (animated variables
/// via [`Scope::var`], node references via [`Scope::node_prop`]). Object
/// safety (`&dyn Scope`) is deliberate: [`Expr::eval`] takes a trait object
/// so callers never have to make the compiled program generic over their
/// own context type.
pub trait Scope {
    /// Resolve a `$name` reference. `None` means "not defined in this
    /// scope" — [`Expr::eval`] turns that into
    /// [`ExprError::UnknownIdent`], the same error a genuinely unknown name
    /// produces, since from the expression's point of view the two are
    /// indistinguishable.
    fn var(&self, name: &str) -> Option<f64>;

    /// Resolve `node("id", "prop")`. Defaulted to always return `None` (not
    /// made a required method) because most [`Scope`] implementations never
    /// need to answer it: an expression containing a node reference is
    /// never [`Expr::is_static`], so the load-time static-fold scope in
    /// particular is never asked.
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

/// A parsed, compiled arithmetic expression.
///
/// Construct with [`Expr::parse`]; the source string is consumed at that
/// point and never touched again — [`Expr::eval`] only ever walks the
/// compiled program built once during parsing. See the module doc for the
/// grammar and the two evaluation tiers.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    ops: Box<[eval::Op]>,
    var_names: Box<[Box<str>]>,
    node_refs: Box<[(Box<str>, Box<str>)]>,
    free_vars: Vec<String>,
    is_static: bool,
}

impl Expr {
    /// Parse (and fully compile) an expression. Accepts either the raw
    /// `"= ..."` JSON string value or just the body after the sigil — see
    /// the module doc's "The `=` prefix" section.
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

    /// Evaluate the compiled program against `scope`. Allocation-free: see
    /// the [`eval`] module doc.
    pub fn eval(&self, scope: &dyn Scope) -> Result<f64, ExprError> {
        eval::run(&self.ops, &self.var_names, &self.node_refs, scope)
    }

    /// Every distinct `$name` this expression references, in first-seen
    /// order. Does not include `node(...)` reference ids/props — those are
    /// string literals resolved through [`Scope::node_prop`], not
    /// `$name` variables.
    pub fn free_vars(&self) -> Vec<String> {
        self.free_vars.clone()
    }

    /// True when [`Expr::free_vars`] names nothing time-varying and the
    /// expression contains no `node(...)` call — see the module doc's
    /// "Two evaluation tiers" section. A caller that gets `true` back may
    /// evaluate once, at load time, and keep the result forever; `false`
    /// means the value can change from frame to frame and must be
    /// re-evaluated each time.
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
        // (2+3)*4 - (10/2 % 3) = 20 - (5.0 % 3.0) = 20 - 2.0 = 18.0
        assert_eq!(expr.eval(&EmptyScope).unwrap(), 18.0);
    }

    #[test]
    fn ternary_short_circuits() {
        // The untaken branch divides by zero; if both were evaluated
        // eagerly this would produce +inf instead of the taken branch's 5.
        let expr = Expr::parse("= 1 > 0 ? 5 : (1 / 0)").unwrap();
        assert_eq!(expr.eval(&EmptyScope).unwrap(), 5.0);
    }

    #[test]
    fn ternary_untaken_branch_errors_are_not_raised() {
        // The untaken branch references an unknown var; must not error.
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
        // And a fresh parse of the same source is identical too.
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
}
