use crate::lexer::Token;

/// Whether the `cfg` predicate in `predicate` (the tokens inside
/// `cfg( … )`) is false whenever `test` is. Every other condition counts as
/// unknown, so a module is left out only when no build outside tests can
/// enable it.
pub fn requires_test(predicate: &[Token]) -> bool {
    let mut evaluator = Evaluator { tokens: predicate, pos: 0 };
    evaluator.predicate() == Some(false)
}

struct Evaluator<'tokens> {
    tokens: &'tokens [Token],
    pos: usize,
}

impl Evaluator<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Evaluates one predicate with `test` false: `Some(value)` when known,
    /// `None` when it depends on another condition.
    fn predicate(&mut self) -> Option<bool> {
        let Some(Token::Ident(name)) = self.peek().cloned() else { return None };
        self.pos += 1;
        if self.peek() == Some(&Token::Punct('=')) {
            // `key = "value"`: the lexer drops the string literal.
            self.pos += 1;
            return None;
        }
        if self.peek() != Some(&Token::Punct('(')) {
            return (name == "test").then_some(false);
        }
        self.pos += 1;
        let operands = self.operands();
        match name.as_str() {
            "not" => operands
                .first()
                .copied()
                .flatten()
                .map(|value| !value),
            "all" => combine(&operands, false),
            "any" => combine(&operands, true),
            _ => None,
        }
    }

    /// The comma-separated predicates up to the closing `)`, which it consumes.
    fn operands(&mut self) -> Vec<Option<bool>> {
        let mut operands = Vec::new();
        while let Some(token) = self.peek() {
            match token {
                Token::Punct(')') => {
                    self.pos += 1;
                    break;
                }
                Token::Punct(',') => self.pos += 1,
                _ => {
                    let before = self.pos;
                    operands.push(self.predicate());
                    if self.pos == before {
                        self.pos += 1;
                    }
                }
            }
        }
        operands
    }
}

/// `all` when `deciding` is `false`, `any` when it is `true`: one operand
/// equal to `deciding` decides the result, and every operand equal to its
/// opposite decides the opposite.
fn combine(operands: &[Option<bool>], deciding: bool) -> Option<bool> {
    if operands.contains(&Some(deciding)) {
        return Some(deciding);
    }
    operands
        .iter()
        .all(|operand| *operand == Some(!deciding))
        .then_some(!deciding)
}
