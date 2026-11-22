use super::{
    escape::DoubleQuoteScope,
    scan,
};

/// Where the text being rewritten ends up in the finished script, which is
/// what decides whether an operator in it is syntax or word text.
#[derive(Clone, Copy)]
pub(super) enum Landing {
    Script,
    /// POSIX makes an operator here part of the word, so it is escaped rather
    /// than spliced in as the command, pipeline, or redirection the parser
    /// would otherwise read.
    UnquotedWord,
    /// The surrounding double quotes already make an operator here literal.
    QuotedWord,
}

impl Landing {
    pub(super) fn initial_mode(self) -> Mode {
        match self {
            Landing::Script => Mode::Script(Quote::Unquoted),
            Landing::UnquotedWord => Mode::UnquotedWord(Quote::Unquoted),
            Landing::QuotedWord => Mode::QuotedWord,
        }
    }

    pub(super) fn parameter_quoting(self) -> scan::ParameterQuoting {
        match self {
            Landing::QuotedWord => scan::ParameterQuoting::DoubleQuoted,
            Landing::Script | Landing::UnquotedWord => scan::ParameterQuoting::Unquoted,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Quote {
    Unquoted,
    Single,
    Double,
}

/// Active lexical context inside [`super::Cursor`]. [`Mode::QuotedWord`] stays inside
/// its enclosing double quotes for the entire `word`, so only [`Mode::Script`]
/// and [`Mode::UnquotedWord`] transition between quotes.
#[derive(Clone, Copy)]
pub(super) enum Mode {
    Script(Quote),
    UnquotedWord(Quote),
    QuotedWord,
}

impl Mode {
    pub(super) fn word_landing(self) -> Landing {
        match self {
            Mode::Script(Quote::Double) | Mode::UnquotedWord(Quote::Double) | Mode::QuotedWord => {
                Landing::QuotedWord
            }
            Mode::Script(Quote::Unquoted | Quote::Single)
            | Mode::UnquotedWord(Quote::Unquoted | Quote::Single) => Landing::UnquotedWord,
        }
    }

    pub(super) fn with_quote(self, quote: Quote) -> Self {
        match self {
            Mode::Script(Quote::Unquoted | Quote::Single | Quote::Double) => Mode::Script(quote),
            Mode::UnquotedWord(Quote::Unquoted | Quote::Single | Quote::Double) => {
                Mode::UnquotedWord(quote)
            }
            Mode::QuotedWord => Mode::QuotedWord,
        }
    }

    pub(super) fn double_quote_scope(self) -> DoubleQuoteScope {
        match self {
            Mode::QuotedWord => DoubleQuoteScope::ParameterWord,
            Mode::Script(Quote::Unquoted | Quote::Single | Quote::Double)
            | Mode::UnquotedWord(Quote::Unquoted | Quote::Single | Quote::Double) => {
                DoubleQuoteScope::Literal
            }
        }
    }
}
