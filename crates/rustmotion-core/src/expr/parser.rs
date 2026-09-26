//! Recursive-descent parser: `Vec<Token>` in, [`Ast`] out.
//!
//! Precedence, low to high: ternary (`?:`, right-associative) → comparison
//! (`== != < <= > >=`, left-associative chaining) → additive (`+ -`) →
//! multiplicative (`* / %`) → unary minus → primary (literal, `$var`,
//! `(expr)`, builtin call, `node("id","prop")`, constant).
//!
//! ## The nesting cap
//!
//! [`MAX_DEPTH`] bounds how deep the *recursive descent itself* is allowed
//! to go, checked at every point the grammar re-enters "parse one full
//! sub-expression": parenthesised groups, each function/`node()` argument,
//! both ternary branches, and each link of a chained unary minus. Because
//! every other production in this grammar (comparison, additive,
//! multiplicative) is an iterative precedence-climb — a `while` loop, not a
//! function calling itself — those four sites are the *only* ways an
//! adversarial input can make the parser recurse, so guarding them is
//! sufficient to guarantee the whole parse (and the `Box<Ast>` tree it
//! produces) is bounded, without having to thread the check through every
//! grammar rule individually. A pathological input like 5000 nested parens
//! or a `pow(pow(pow(...)))` chain hits [`ExprError::TooDeep`] here, at
//! parse time, rather than blowing the native call stack or building an
//! unbounded tree that a later pass would have to walk.

use super::ast::{Ast, BinOp, UnOp};
use super::builtins::{constant, Builtin};
use super::lexer::{tokenize, Token};
use super::ExprError;

/// Recursion budget for the four self-recursive grammar entry points (see
/// module doc). 64 is far beyond any nesting a hand- or LLM-written
/// expression plausibly needs, and comfortably inside the native stack —
/// the point is to fail long before that becomes a concern either way.
const MAX_DEPTH: usize = 64;

pub(crate) fn parse(src: &str) -> Result<Ast, ExprError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        depth: 0,
        src,
    };
    let ast = parser.parse_expr()?;
    parser.expect_end(src)?;
    Ok(ast)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    src: &'a str,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn err(&self, reason: impl Into<String>) -> ExprError {
        ExprError::Parse {
            src: self.src.to_string(),
            reason: reason.into(),
        }
    }

    fn expect_end(&self, src: &str) -> Result<(), ExprError> {
        if self.pos != self.tokens.len() {
            return Err(ExprError::Parse {
                src: src.to_string(),
                reason: format!(
                    "unexpected trailing input after a complete expression (token {})",
                    self.pos
                ),
            });
        }
        Ok(())
    }

    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(ExprError::TooDeep(MAX_DEPTH));
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.depth -= 1;
    }

    /// Top-level rule and the guarded re-entry point for parenthesised
    /// groups, each ternary branch, and every call argument.
    fn parse_expr(&mut self) -> Result<Ast, ExprError> {
        self.enter()?;
        let result = self.parse_ternary();
        self.exit();
        result
    }

    fn parse_ternary(&mut self) -> Result<Ast, ExprError> {
        let cond = self.parse_comparison()?;
        if matches!(self.peek(), Some(Token::Question)) {
            self.advance();
            let then_branch = self.parse_expr()?;
            self.expect(&Token::Colon)?;
            let else_branch = self.parse_expr()?;
            return Ok(Ast::Ternary(
                Box::new(cond),
                Box::new(then_branch),
                Box::new(else_branch),
            ));
        }
        Ok(cond)
    }

    fn parse_comparison(&mut self) -> Result<Ast, ExprError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Some(Token::EqEq) => BinOp::Eq,
                Some(Token::NotEq) => BinOp::Ne,
                Some(Token::Lt) => BinOp::Lt,
                Some(Token::Le) => BinOp::Le,
                Some(Token::Gt) => BinOp::Gt,
                Some(Token::Ge) => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Ast::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Ast, ExprError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Ast::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Ast, ExprError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                Some(Token::Percent) => BinOp::Rem,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Ast::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// Guarded separately from [`Parser::parse_expr`] because a chain of
    /// unary minuses (`----1`) recurses into itself directly, never passing
    /// back through `parse_expr` — see the module doc.
    fn parse_unary(&mut self) -> Result<Ast, ExprError> {
        if matches!(self.peek(), Some(Token::Minus)) {
            self.advance();
            self.enter()?;
            let inner = self.parse_unary();
            self.exit();
            return Ok(Ast::Unary(UnOp::Neg, Box::new(inner?)));
        }
        // No unary `+`: the frozen grammar names only unary minus. Adding
        // a second unguarded-by-default recursive entry point here for a
        // no-op sign isn't worth the extra surface, so `+1` is a parse
        // error rather than a silent alias for `1`.
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Ast, ExprError> {
        match self.advance() {
            Some(Token::Num(n)) => Ok(Ast::Num(n)),
            Some(Token::Var(name)) => Ok(Ast::Var(name)),
            Some(Token::LParen) => {
                let inner = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(inner)
            }
            Some(Token::Ident(name)) => self.parse_ident(name),
            Some(Token::Str(_)) => {
                Err(self.err("a string literal is only valid as a node(\"id\", \"prop\") argument"))
            }
            Some(other) => Err(self.err(format!("unexpected token {other:?}"))),
            None => Err(self.err("unexpected end of expression")),
        }
    }

    fn parse_ident(&mut self, name: String) -> Result<Ast, ExprError> {
        if name == "node" {
            return self.parse_node_ref();
        }
        if let Some(builtin) = Builtin::from_name(&name) {
            return self.parse_call(builtin);
        }
        if let Some(value) = constant(&name) {
            if matches!(self.peek(), Some(Token::LParen)) {
                return Err(self.err(format!("'{name}' is a constant, not a function")));
            }
            return Ok(Ast::Num(value));
        }
        Err(ExprError::UnknownIdent(name))
    }

    fn parse_call(&mut self, builtin: Builtin) -> Result<Ast, ExprError> {
        self.expect(&Token::LParen)?;
        let mut args = Vec::new();
        if !matches!(self.peek(), Some(Token::RParen)) {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        self.expect(&Token::RParen)?;
        let expected = builtin.arity();
        if args.len() != expected {
            return Err(ExprError::Arity {
                name: builtin.name().to_string(),
                expected,
                got: args.len(),
            });
        }
        Ok(Ast::Call(builtin, args))
    }

    fn parse_node_ref(&mut self) -> Result<Ast, ExprError> {
        self.expect(&Token::LParen)?;
        let id = self.expect_string("node")?;
        self.expect(&Token::Comma)?;
        let prop = self.expect_string("node")?;
        self.expect(&Token::RParen)?;
        Ok(Ast::NodeRef(id, prop))
    }

    fn expect_string(&mut self, ctx: &str) -> Result<String, ExprError> {
        match self.advance() {
            Some(Token::Str(s)) => Ok(s),
            Some(other) => Err(self.err(format!(
                "{ctx}(...) expects a string literal argument, found {other:?}"
            ))),
            None => Err(self.err(format!("{ctx}(...) expects a string literal argument"))),
        }
    }

    fn expect(&mut self, want: &Token) -> Result<(), ExprError> {
        match self.advance() {
            Some(ref t) if t == want => Ok(()),
            Some(other) => Err(self.err(format!("expected {want:?}, found {other:?}"))),
            None => Err(self.err(format!("expected {want:?}, found end of expression"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_precedence() {
        // 1 + 2 * 3 == 1 + (2 * 3)
        let ast = parse("1 + 2 * 3").unwrap();
        assert_eq!(
            ast,
            Ast::Bin(
                BinOp::Add,
                Box::new(Ast::Num(1.0)),
                Box::new(Ast::Bin(
                    BinOp::Mul,
                    Box::new(Ast::Num(2.0)),
                    Box::new(Ast::Num(3.0))
                ))
            )
        );
    }

    #[test]
    fn parses_ternary_and_comparison() {
        let ast = parse("$x > 0 ? 1 : -1").unwrap();
        match ast {
            Ast::Ternary(cond, then_b, else_b) => {
                assert_eq!(
                    *cond,
                    Ast::Bin(
                        BinOp::Gt,
                        Box::new(Ast::Var("x".to_string())),
                        Box::new(Ast::Num(0.0))
                    )
                );
                assert_eq!(*then_b, Ast::Num(1.0));
                assert_eq!(*else_b, Ast::Unary(UnOp::Neg, Box::new(Ast::Num(1.0))));
            }
            other => panic!("expected ternary, got {other:?}"),
        }
    }

    #[test]
    fn parses_node_ref() {
        let ast = parse("node(\"badge_3\", \"x\") + 1").unwrap();
        assert_eq!(
            ast,
            Ast::Bin(
                BinOp::Add,
                Box::new(Ast::NodeRef("badge_3".to_string(), "x".to_string())),
                Box::new(Ast::Num(1.0))
            )
        );
    }

    #[test]
    fn unknown_function_name_is_unknown_ident() {
        let err = parse("sni(1)").unwrap_err();
        assert_eq!(err, ExprError::UnknownIdent("sni".to_string()));
    }

    #[test]
    fn wrong_arity_is_reported() {
        let err = parse("sin(1, 2)").unwrap_err();
        assert_eq!(
            err,
            ExprError::Arity {
                name: "sin".to_string(),
                expected: 1,
                got: 2,
            }
        );
    }

    #[test]
    fn constant_is_not_callable() {
        assert!(parse("PI(1)").is_err());
    }

    #[test]
    fn trailing_garbage_is_a_parse_error() {
        assert!(parse("1 + 1 )").is_err());
    }

    #[test]
    fn deeply_nested_parens_hit_the_depth_cap() {
        let src = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        let err = parse(&src).unwrap_err();
        assert_eq!(err, ExprError::TooDeep(MAX_DEPTH));
    }

    #[test]
    fn deeply_chained_unary_minus_hits_the_depth_cap() {
        let src = format!("{}1", "-".repeat(200));
        let err = parse(&src).unwrap_err();
        assert_eq!(err, ExprError::TooDeep(MAX_DEPTH));
    }

    #[test]
    fn huge_nested_pow_chain_hits_the_depth_cap() {
        // A hostile "huge exponent" shape: pow(pow(pow(...2...))), nested
        // deep enough to be the kind of input that should never reach eval.
        let mut src = "2".to_string();
        for _ in 0..200 {
            src = format!("pow({src}, 2)");
        }
        let err = parse(&src).unwrap_err();
        assert_eq!(err, ExprError::TooDeep(MAX_DEPTH));
    }

    #[test]
    fn reasonable_nesting_is_accepted() {
        let src = format!("{}1{}", "(".repeat(10), ")".repeat(10));
        assert!(parse(&src).is_ok());
    }
}
