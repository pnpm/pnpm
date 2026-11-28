//! The lemmas that prove each shell reads the original text back.

use super::logic::{
    cmd_escape_char,
    cmd_escaped,
    cmd_read,
    sh_escape,
    sh_escaped,
    sh_read,
};
use creusot_std::prelude::{
    Seq,
    ensures,
    logic,
    requires,
    variant,
};

/// A character in front of `rest` is the first character, and `rest` the
/// tail.
#[logic]
#[ensures(Seq::singleton(first).concat(rest)[0] == first)]
#[ensures(Seq::singleton(first).concat(rest).len() == rest.len() + 1)]
#[ensures(Seq::singleton(first).concat(rest).tail().ext_eq(rest))]
#[ensures(Seq::singleton(first).concat(rest).tail() == rest)]
pub fn cons_tail(first: char, rest: Seq<char>) {}

/// An empty text is `Seq::empty()`.
#[logic]
#[requires(text.len() == 0)]
#[ensures(text.ext_eq(Seq::empty()))]
#[ensures(text == Seq::empty())]
pub fn empty_text(text: Seq<char>) {}

/// A non-empty text is its first character in front of its tail.
#[logic]
#[requires(text.len() > 0)]
#[ensures(text.ext_eq(Seq::singleton(text[0]).concat(text.tail())))]
#[ensures(text == Seq::singleton(text[0]).concat(text.tail()))]
pub fn first_and_tail(text: Seq<char>) {}

/// Escaping one more character for `sh` appends that character's escape.
#[logic]
#[ensures(sh_escaped(done.concat(Seq::singleton(character)))
    == sh_escaped(done).concat(sh_escape(character)))]
pub fn sh_escaped_push(done: Seq<char>, character: char) {
    pearlite! {
        done.concat_flat_map(Seq::singleton(character), |character: char| sh_escape(character));
        Seq::singleton_flat_map(character, |character: char| sh_escape(character))
    }
}

/// The `sh` escape of a non-empty text is its first character's escape and
/// then its tail's.
#[logic]
#[requires(text.len() > 0)]
#[ensures(sh_escaped(text) == sh_escape(text[0]).concat(sh_escaped(text.tail())))]
pub fn sh_escaped_first(text: Seq<char>) {
    pearlite! {
        first_and_tail(text);
        Seq::singleton(text[0])
            .concat_flat_map(text.tail(), |character: char| sh_escape(character));
        Seq::singleton_flat_map(text[0], |character: char| sh_escape(character))
    }
}

/// Inside quotes, `sh` reads the escape of `text` and a closing `'` as
/// `text`.
#[logic]
#[variant(text.len())]
#[ensures(sh_read(sh_escaped(text).concat(Seq::singleton('\'')), true) == Some(text))]
pub fn sh_reads_escaped(text: Seq<char>) {
    pearlite! {
        if text.len() == 0 {
            empty_text(text);
            Seq::singleton('\'').concat_empty();
            cons_tail('\'', Seq::empty())
        } else {
            sh_reads_escaped(text.tail());
            sh_escaped_first(text);
            first_and_tail(text);
            sh_escape(text[0]).concat_assoc(sh_escaped(text.tail()), Seq::singleton('\''));
            if text[0] == '\'' {
                sh_quote_steps(sh_escaped(text.tail()).concat(Seq::singleton('\'')))
            } else {
                cons_tail(text[0], sh_escaped(text.tail()).concat(Seq::singleton('\'')))
            }
        }
    }
}

/// Inside quotes, `sh` reads `'\''` in front of `rest` as `'` in front of
/// what it reads from `rest`.
#[logic]
#[ensures(sh_read(sh_escape('\'').concat(rest), true) == match sh_read(rest, true) {
    Some(read) => Some(Seq::singleton('\'').concat(read)),
    None => None,
})]
pub fn sh_quote_steps(rest: Seq<char>) {
    pearlite! {
        Seq::singleton('\'').concat(Seq::singleton('\\'))
            .concat_assoc(Seq::singleton('\''), Seq::singleton('\''));
        Seq::singleton('\'').concat_assoc(
            Seq::singleton('\\'),
            Seq::singleton('\'').concat(Seq::singleton('\'')),
        );
        Seq::singleton('\'').concat(
            Seq::singleton('\\').concat(Seq::singleton('\'').concat(Seq::singleton('\''))),
        ).concat_assoc(Seq::empty(), rest);
        cons_tail('\'', rest);
        cons_tail('\'', Seq::singleton('\'').concat(rest));
        cons_tail('\\', Seq::singleton('\'').concat(Seq::singleton('\'').concat(rest)));
        cons_tail('\'', Seq::singleton('\\').concat(Seq::singleton('\'').concat(Seq::singleton('\'').concat(rest))));
        Seq::singleton('\'').concat_assoc(Seq::singleton('\''), rest);
        Seq::singleton('\\').concat_assoc(Seq::singleton('\'').concat(Seq::singleton('\'')), rest);
        Seq::singleton('\'').concat_assoc(
            Seq::singleton('\\').concat(Seq::singleton('\'').concat(Seq::singleton('\''))),
            rest,
        )
    }
}

/// `sh` reads the result of [`crate::sh_single_quote`] as the text.
#[logic]
#[ensures(sh_read(Seq::singleton('\'').concat(sh_escaped(text)).concat(Seq::singleton('\'')), false)
    == Some(text))]
pub fn sh_reads_quoted(text: Seq<char>) {
    pearlite! {
        sh_reads_escaped(text);
        Seq::singleton('\'').concat_assoc(sh_escaped(text), Seq::singleton('\''));
        cons_tail('\'', sh_escaped(text).concat(Seq::singleton('\'')))
    }
}

/// Escaping one more character for `cmd` appends that character's escape.
#[logic]
#[ensures(cmd_escaped(done.concat(Seq::singleton(character)))
    == cmd_escaped(done).concat(cmd_escape_char(character)))]
pub fn cmd_escaped_push(done: Seq<char>, character: char) {
    pearlite! {
        done.concat_flat_map(Seq::singleton(character), |character: char| cmd_escape_char(character));
        Seq::singleton_flat_map(character, |character: char| cmd_escape_char(character))
    }
}

/// `cmd` reads the escape of `text` as `text`.
#[logic]
#[variant(text.len())]
#[ensures(cmd_read(cmd_escaped(text)) == Some(text))]
pub fn cmd_reads_escaped(text: Seq<char>) {
    pearlite! {
        if text.len() == 0 {
            empty_text(text)
        } else {
            cmd_reads_escaped(text.tail());
            first_and_tail(text);
            Seq::singleton(text[0])
                .concat_flat_map(text.tail(), |character: char| cmd_escape_char(character));
            Seq::singleton_flat_map(text[0], |character: char| cmd_escape_char(character));
            if text[0] == '%' {
                Seq::singleton('%').concat_assoc(Seq::singleton('%'), cmd_escaped(text.tail()));
                cons_tail('%', cmd_escaped(text.tail()));
                cons_tail('%', Seq::singleton('%').concat(cmd_escaped(text.tail())))
            } else {
                cons_tail(text[0], cmd_escaped(text.tail()))
            }
        }
    }
}
