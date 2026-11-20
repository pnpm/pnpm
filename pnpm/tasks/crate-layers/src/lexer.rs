//! A token stream of Rust source that is good enough to find module
//! declarations, `use` trees, and paths. Comments, string literals, and
//! character literals are skipped, so a path written inside one is not read.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Ident(String),
    PathSep,
    Punct(char),
}

#[derive(Debug, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    /// Lines on which at least one token starts.
    pub code_lines: usize,
}

pub fn lex(source: &str) -> Lexed {
    let mut lexer = Lexer { chars: source.chars().collect(), pos: 0, line: 0, ..Lexer::default() };
    while lexer.pos < lexer.chars.len() {
        lexer.step();
    }
    Lexed { tokens: lexer.tokens, code_lines: lexer.code_lines }
}

#[derive(Default)]
struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    last_token_line: Option<usize>,
    code_lines: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars
            .get(self.pos + offset)
            .copied()
    }

    fn step(&mut self) {
        let current = self.chars[self.pos];
        match current {
            '\n' => {
                self.line += 1;
                self.pos += 1;
            }
            _ if current.is_whitespace() => self.pos += 1,
            '/' if self.peek(1) == Some('/') => self.skip_line(),
            '/' if self.peek(1) == Some('*') => self.skip_block_comment(),
            '"' => self.skip_string(),
            '\'' => self.skip_char_or_lifetime(),
            ':' if self.peek(1) == Some(':') => {
                self.push(Token::PathSep);
                self.pos += 2;
            }
            _ if is_ident_start(current) => self.ident(),
            _ => {
                self.push(Token::Punct(current));
                self.pos += 1;
            }
        }
    }

    fn push(&mut self, token: Token) {
        if self.last_token_line != Some(self.line) {
            self.last_token_line = Some(self.line);
            self.code_lines += 1;
        }
        self.tokens.push(token);
    }

    /// Moves past one character, keeping the line count.
    fn advance(&mut self) {
        if self.peek(0) == Some('\n') {
            self.line += 1;
        }
        self.pos += 1;
    }

    fn skip_line(&mut self) {
        while self
            .peek(0)
            .is_some_and(|current| current != '\n')
        {
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        let mut depth = 0usize;
        while self.pos < self.chars.len() {
            match (self.peek(0), self.peek(1)) {
                (Some('/'), Some('*')) => depth += 1,
                (Some('*'), Some('/')) => depth -= 1,
                _ => {
                    self.advance();
                    continue;
                }
            }
            self.pos += 2;
            if depth == 0 {
                return;
            }
        }
    }

    /// Skips a quoted literal whose opening quote is at the cursor.
    fn skip_string(&mut self) {
        self.pos += 1;
        while let Some(current) = self.peek(0) {
            match current {
                '\\' => self.pos += 2,
                '"' => {
                    self.pos += 1;
                    return;
                }
                _ => self.advance(),
            }
        }
    }

    /// Skips `"…"#…` after an `r`, `br`, or `cr` prefix with `hashes` hashes.
    fn skip_raw_string(&mut self, hashes: usize) {
        self.pos += hashes + 1;
        while self.pos < self.chars.len() {
            let closes = self.peek(0) == Some('"')
                && (1..=hashes).all(|offset| self.peek(offset) == Some('#'));
            if closes {
                self.pos += hashes + 1;
                return;
            }
            self.advance();
        }
    }

    /// A `'` starts a character literal (`'a'`, `'\n'`) or a lifetime (`'a`).
    fn skip_char_or_lifetime(&mut self) {
        if self.peek(1) == Some('\\') {
            self.pos += 2;
            while self
                .peek(0)
                .is_some_and(|current| current != '\'')
            {
                self.pos += 1;
            }
            self.pos += 1;
        } else if self.peek(2) == Some('\'') {
            self.pos += 3;
        } else {
            self.pos += 1;
        }
    }

    fn ident(&mut self) {
        let start = self.pos;
        while self.peek(0).is_some_and(is_ident_continue) {
            self.pos += 1;
        }
        let word = self.chars[start..self.pos].iter().collect::<String>();
        if let Some(hashes) = self.raw_string_hashes(&word) {
            self.skip_raw_string(hashes);
        } else if matches!(word.as_str(), "b" | "c") && self.peek(0) == Some('"') {
            self.skip_string();
        } else if word == "b" && self.peek(0) == Some('\'') {
            self.skip_char_or_lifetime();
        } else if word == "r"
            && self.peek(0) == Some('#')
            && self.peek(1).is_some_and(is_ident_start)
        {
            self.raw_ident();
        } else {
            self.push(Token::Ident(word));
        }
    }

    /// Reads `name` after the `r#` of a raw identifier as a plain identifier.
    fn raw_ident(&mut self) {
        self.pos += 1;
        let start = self.pos;
        while self.peek(0).is_some_and(is_ident_continue) {
            self.pos += 1;
        }
        self.push(Token::Ident(self.chars[start..self.pos].iter().collect()));
    }

    /// The number of `#` after a raw string prefix, when `word` is one.
    fn raw_string_hashes(&self, word: &str) -> Option<usize> {
        if !matches!(word, "r" | "br" | "cr") {
            return None;
        }
        let hashes = (0..self.chars.len())
            .take_while(|offset| self.peek(*offset) == Some('#'))
            .count();
        (self.peek(hashes) == Some('"')).then_some(hashes)
    }
}

fn is_ident_start(current: char) -> bool {
    current == '_' || current.is_alphabetic()
}

fn is_ident_continue(current: char) -> bool {
    current == '_' || current.is_alphanumeric()
}
