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

/// Whether the attributes ending right before `tokens[item]`, skipping a
/// visibility, include a `cfg` that names `test`.
pub fn is_test_only(tokens: &[Token], item: usize) -> bool {
    let mut end = skip_visibility_backwards(tokens, item);
    while end > 0 && tokens[end - 1] == Token::Punct(']') {
        let Some(open) = matching_open_bracket(tokens, end - 1) else { return false };
        let attribute = &tokens[open..end];
        if enables_only_tests(attribute) {
            return true;
        }
        end = open.saturating_sub(1);
    }
    false
}

/// Whether `attribute`, the tokens from `[` to `]`, is a `cfg` that no build
/// outside tests can satisfy.
fn enables_only_tests(attribute: &[Token]) -> bool {
    let is_cfg = attribute.get(1) == Some(&Token::Ident("cfg".to_owned()))
        && attribute.get(2) == Some(&Token::Punct('('));
    let predicate = attribute.get(3..attribute.len().saturating_sub(2));
    predicate.filter(|_| is_cfg).is_some_and(requires_test)
}

fn skip_visibility_backwards(tokens: &[Token], item: usize) -> usize {
    let pub_at = |index: usize| tokens.get(index) == Some(&Token::Ident("pub".to_owned()));
    if item >= 1 && pub_at(item - 1) {
        return item - 1;
    }
    if item >= 1 && tokens[item - 1] == Token::Punct(')') {
        let open = (0..item - 1)
            .rev()
            .find(|index| tokens[*index] == Token::Punct('('));
        if let Some(open) = open.filter(|open| *open >= 1 && pub_at(open - 1)) {
            return open - 1;
        }
    }
    item
}

/// The index of the `[` that opens the attribute whose `]` is at `close`.
fn matching_open_bracket(tokens: &[Token], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in (0..=close).rev() {
        match tokens[index] {
            Token::Punct(']') => depth += 1,
            Token::Punct('[') => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Some(index);
        }
    }
    None
}
