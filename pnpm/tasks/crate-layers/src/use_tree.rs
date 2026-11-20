use crate::lexer::Token;

/// One path a `use` item imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsePath {
    pub segments: Vec<String>,
    /// The name the import binds, or `None` for a glob.
    pub binding: Option<String>,
}

/// Parses the `use` tree that starts at `tokens[start]`, the token after
/// `use`. Returns the imported paths and the index after the closing `;`.
pub fn parse(tokens: &[Token], start: usize) -> (Vec<UsePath>, usize) {
    let mut parser = Parser { tokens, pos: start, paths: Vec::new() };
    if parser.peek() == Some(&Token::PathSep) {
        parser.pos += 1;
    }
    parser.tree(Vec::new());
    while parser.pos < tokens.len() && parser.peek() != Some(&Token::Punct(';')) {
        parser.pos += 1;
    }
    (parser.paths, parser.pos + 1)
}

struct Parser<'tokens> {
    tokens: &'tokens [Token],
    pos: usize,
    paths: Vec<UsePath>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn tree(&mut self, mut segments: Vec<String>) {
        loop {
            match self.peek().cloned() {
                Some(Token::Ident(name)) => {
                    self.pos += 1;
                    segments.push(name);
                    if self.peek() == Some(&Token::PathSep) {
                        self.pos += 1;
                        continue;
                    }
                    self.finish(segments);
                }
                Some(Token::Punct('{')) => {
                    self.pos += 1;
                    self.group(&segments);
                }
                Some(Token::Punct('*')) => {
                    self.pos += 1;
                    self.paths.push(UsePath { segments, binding: None });
                }
                _ => {}
            }
            return;
        }
    }

    fn group(&mut self, prefix: &[String]) {
        while let Some(token) = self.peek() {
            match token {
                Token::Punct('}') => {
                    self.pos += 1;
                    return;
                }
                Token::Punct(',') => self.pos += 1,
                _ => {
                    let before = self.pos;
                    self.tree(prefix.to_vec());
                    if self.pos == before {
                        self.pos += 1;
                    }
                }
            }
        }
    }

    /// Records a complete path, applying `as` and a trailing `self`.
    fn finish(&mut self, mut segments: Vec<String>) {
        if segments
            .last()
            .is_some_and(|last| last == "self")
        {
            segments.pop();
        }
        let mut binding = segments.last().cloned();
        if self.peek() == Some(&Token::Ident("as".to_owned())) {
            self.pos += 1;
            if let Some(Token::Ident(alias)) = self.peek().cloned() {
                self.pos += 1;
                binding = Some(alias);
            }
        }
        self.paths.push(UsePath { segments, binding });
    }
}
