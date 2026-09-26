use super::ExprError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    Num(f64),
    Var(String),
    Ident(String),
    Str(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
}

const MAX_SOURCE_LEN: usize = 8192;

pub(crate) fn tokenize(src: &str) -> Result<Vec<Token>, ExprError> {
    if src.len() > MAX_SOURCE_LEN {
        return Err(ExprError::Parse {
            src: src.to_string(),
            reason: format!(
                "expression is {} bytes long, exceeding the {MAX_SOURCE_LEN}-byte cap",
                src.len()
            ),
        });
    }

    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    let err = |reason: String| ExprError::Parse {
        src: src.to_string(),
        reason,
    };

    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '?' => {
                tokens.push(Token::Question);
                i += 1;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
            }
            '=' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::EqEq);
                    i += 2;
                } else {
                    return Err(err(format!(
                        "unexpected '=' at byte {i} (did you mean '=='?)"
                    )));
                }
            }
            '!' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::NotEq);
                    i += 2;
                } else {
                    return Err(err(format!("unexpected '!' at byte {i}")));
                }
            }
            '<' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::Le);
                    i += 2;
                } else {
                    tokens.push(Token::Lt);
                    i += 1;
                }
            }
            '>' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::Ge);
                    i += 2;
                } else {
                    tokens.push(Token::Gt);
                    i += 1;
                }
            }
            '$' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && is_ident_byte(bytes[j]) {
                    j += 1;
                }
                if j == start {
                    return Err(err(format!("'$' at byte {i} is not followed by a name")));
                }
                tokens.push(Token::Var(src[start..j].to_string()));
                i = j;
            }
            '"' => {
                let mut j = i + 1;
                let mut s = String::new();
                loop {
                    match bytes.get(j) {
                        None => {
                            return Err(err(format!("unterminated string starting at byte {i}")))
                        }
                        Some(b'"') => {
                            j += 1;
                            break;
                        }
                        Some(b'\\') if bytes.get(j + 1) == Some(&b'"') => {
                            s.push('"');
                            j += 2;
                        }
                        Some(b'\\') if bytes.get(j + 1) == Some(&b'\\') => {
                            s.push('\\');
                            j += 2;
                        }
                        Some(_) => {
                            let ch = src[j..].chars().next().unwrap();
                            s.push(ch);
                            j += ch.len_utf8();
                        }
                    }
                }
                tokens.push(Token::Str(s));
                i = j;
            }
            c if c.is_ascii_digit()
                || (c == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                let start = i;
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'.') {
                    j += 1;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                if matches!(bytes.get(j), Some(b'e') | Some(b'E')) {
                    let mut k = j + 1;
                    if matches!(bytes.get(k), Some(b'+') | Some(b'-')) {
                        k += 1;
                    }
                    if bytes.get(k).is_some_and(u8::is_ascii_digit) {
                        k += 1;
                        while k < bytes.len() && bytes[k].is_ascii_digit() {
                            k += 1;
                        }
                        j = k;
                    }
                }
                let text = &src[start..j];
                let n: f64 = text
                    .parse()
                    .map_err(|_| err(format!("'{text}' at byte {start} is not a valid number")))?;
                tokens.push(Token::Num(n));
                i = j;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                let mut j = i;
                while j < bytes.len() && is_ident_byte(bytes[j]) {
                    j += 1;
                }
                tokens.push(Token::Ident(src[start..j].to_string()));
                i = j;
            }
            other => {
                return Err(err(format!("unexpected character '{other}' at byte {i}")));
            }
        }
    }

    Ok(tokens)
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_arithmetic() {
        let toks = tokenize("1 + 2 * 3").unwrap();
        assert_eq!(
            toks,
            vec![
                Token::Num(1.0),
                Token::Plus,
                Token::Num(2.0),
                Token::Star,
                Token::Num(3.0),
            ]
        );
    }

    #[test]
    fn tokenizes_var_and_ident_and_comparisons() {
        let toks = tokenize("$W >= cos(PI)").unwrap();
        assert_eq!(
            toks,
            vec![
                Token::Var("W".to_string()),
                Token::Ge,
                Token::Ident("cos".to_string()),
                Token::LParen,
                Token::Ident("PI".to_string()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn tokenizes_string_literal_with_escapes() {
        let toks = tokenize(r#"node("a\"b", "y")"#).unwrap();
        assert_eq!(
            toks,
            vec![
                Token::Ident("node".to_string()),
                Token::LParen,
                Token::Str("a\"b".to_string()),
                Token::Comma,
                Token::Str("y".to_string()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn tokenizes_scientific_notation() {
        let toks = tokenize("1.5e-3").unwrap();
        assert_eq!(toks, vec![Token::Num(1.5e-3)]);
    }

    #[test]
    fn rejects_lone_dollar() {
        assert!(tokenize("$ + 1").is_err());
    }

    #[test]
    fn rejects_unterminated_string() {
        assert!(tokenize("node(\"a, \"b\")").is_err());
    }

    #[test]
    fn rejects_oversized_source() {
        let huge = "1+".repeat(MAX_SOURCE_LEN);
        assert!(tokenize(&huge).is_err());
    }
}
