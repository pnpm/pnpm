//! What each shell reads from the text the crate root writes.

use creusot_std::prelude::{
    Seq,
    logic,
    variant,
};

/// What `sh` reads from `word`, a run of single-quoted strings and `\'`:
/// inside quotes every character but `'` is itself, and `'` ends them;
/// outside quotes `'` opens them and `\'` is `'`. `quoted` says whether the
/// word starts inside quotes. `None` for an unterminated quote and for any
/// other character outside quotes, which this reading does not cover.
#[logic(open)]
#[variant(word.len())]
pub fn sh_read(word: Seq<char>, quoted: bool) -> Option<Seq<char>> {
    pearlite! {
        if word.len() == 0 {
            if quoted { None } else { Some(Seq::empty()) }
        } else if quoted {
            if word[0] == '\'' {
                sh_read(word.tail(), false)
            } else {
                match sh_read(word.tail(), true) {
                    Some(rest) => Some(Seq::singleton(word[0]).concat(rest)),
                    None => None,
                }
            }
        } else if word[0] == '\'' {
            sh_read(word.tail(), true)
        } else if word[0] == '\\' && word.len() >= 2 && word[1] == '\'' {
            match sh_read(word.tail().tail(), false) {
                Some(rest) => Some(Seq::singleton('\'').concat(rest)),
                None => None,
            }
        } else {
            None
        }
    }
}

/// How [`crate::sh_single_quote`] writes `character` inside its quotes.
#[logic(open)]
pub fn sh_escape(character: char) -> Seq<char> {
    pearlite! {
        if character == '\'' {
            Seq::singleton('\'')
                .concat(Seq::singleton('\\'))
                .concat(Seq::singleton('\''))
                .concat(Seq::singleton('\''))
        } else {
            Seq::singleton(character)
        }
    }
}

/// How [`crate::sh_single_quote`] writes `text` inside its quotes.
#[logic(open)]
pub fn sh_escaped(text: Seq<char>) -> Seq<char> {
    pearlite! { text.flat_map(|character: char| sh_escape(character)) }
}

/// What `cmd` reads from `text`: `%%` is `%`, and every other character is
/// itself. `None` for a `%` that starts anything but `%%`, which `cmd` would
/// read as a variable reference.
#[logic(open)]
#[variant(text.len())]
pub fn cmd_read(text: Seq<char>) -> Option<Seq<char>> {
    pearlite! {
        if text.len() == 0 {
            Some(Seq::empty())
        } else if text[0] == '%' {
            if text.len() >= 2 && text[1] == '%' {
                match cmd_read(text.tail().tail()) {
                    Some(rest) => Some(Seq::singleton('%').concat(rest)),
                    None => None,
                }
            } else {
                None
            }
        } else {
            match cmd_read(text.tail()) {
                Some(rest) => Some(Seq::singleton(text[0]).concat(rest)),
                None => None,
            }
        }
    }
}

/// How [`crate::cmd_escape`] writes `character`.
#[logic(open)]
pub fn cmd_escape_char(character: char) -> Seq<char> {
    pearlite! {
        if character == '%' {
            Seq::singleton('%').concat(Seq::singleton('%'))
        } else {
            Seq::singleton(character)
        }
    }
}

/// How [`crate::cmd_escape`] writes `text`.
#[logic(open)]
pub fn cmd_escaped(text: Seq<char>) -> Seq<char> {
    pearlite! { text.flat_map(|character: char| cmd_escape_char(character)) }
}
