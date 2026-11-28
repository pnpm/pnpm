//! What a glob matches, stated over characters.

use crate::StarGlob;
use creusot_std::prelude::{
    Int,
    Invariant,
    Seq,
    View,
    logic,
    variant,
};
use pnpm_creusot_models::str_model::occurs_at;

impl View for StarGlob {
    type ViewTy = Seq<String>;
    #[logic(open(crate))]
    fn view(self) -> Seq<String> {
        pearlite! { self.segments@ }
    }
}

impl Invariant for StarGlob {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { self@.len() > 0 && no_star_in(self@, 0, self@.len()) }
    }
}

impl StarGlob {
    /// The pattern the glob was made from.
    #[logic(open)]
    pub fn pattern(self) -> Seq<char> {
        pearlite! { joined(self@, 0) }
    }
}

/// Whether `text` matches `pattern`, where `*` matches any run of characters
/// and every other character matches itself.
#[logic(open)]
#[variant(pattern.len())]
pub fn glob(pattern: Seq<char>, text: Seq<char>) -> bool {
    pearlite! {
        if pattern.len() == 0 {
            text.len() == 0
        } else if pattern[0] == '*' {
            exists<skip: Int> 0 <= skip && skip <= text.len()
                && glob(pattern.tail(), text.subsequence(skip, text.len()))
        } else {
            text.len() > 0 && text[0] == pattern[0] && glob(pattern.tail(), text.tail())
        }
    }
}

/// Whether no segment in `segments[from..to]` holds a `*`.
#[logic(open)]
pub fn no_star_in(segments: Seq<String>, from: Int, to: Int) -> bool {
    pearlite! {
        forall<index: Int> from <= index && index < to ==> !segments[index]@.contains('*')
    }
}

/// `segments[from..]` joined by `*`.
#[logic(open)]
#[variant(segments.len() - from)]
pub fn joined(segments: Seq<String>, from: Int) -> Seq<char> {
    pearlite! {
        if from + 1 >= segments.len() {
            segments[from]@
        } else {
            segments[from]@.concat(Seq::singleton('*')).concat(joined(segments, from + 1))
        }
    }
}

/// `segments[from..to]`, each followed by `*`.
#[logic(open)]
#[variant(to - from)]
pub fn stars_after(segments: Seq<String>, from: Int, to: Int) -> Seq<char> {
    pearlite! {
        if from >= to {
            Seq::empty()
        } else {
            segments[from]@.concat(Seq::singleton('*')).concat(stars_after(segments, from + 1, to))
        }
    }
}

/// Whether `text` ends with `part`.
#[logic(open)]
pub fn ends_with(text: Seq<char>, part: Seq<char>) -> bool {
    pearlite! { part.len() <= text.len() && occurs_at(text, part, text.len() - part.len()) }
}

/// Whether `segments[from..to]` all occur in `text`, in order and without
/// overlap.
#[logic(open)]
#[variant(to - from)]
pub fn in_order(segments: Seq<String>, from: Int, to: Int, text: Seq<char>) -> bool {
    pearlite! {
        if from >= to {
            true
        } else {
            exists<at: Int> occurs_at(text, segments[from]@, at)
                && in_order(segments, from + 1, to,
                    text.subsequence(at + segments[from]@.len(), text.len()))
        }
    }
}

/// Whether `text` matches `*` and then `segments[from..]` joined by `*`,
/// stated segment by segment.
#[logic(open)]
#[variant(segments.len() - from)]
pub fn after_star(segments: Seq<String>, from: Int, text: Seq<char>) -> bool {
    pearlite! {
        if from + 1 >= segments.len() {
            ends_with(text, segments[from]@)
        } else {
            exists<at: Int> occurs_at(text, segments[from]@, at)
                && after_star(segments, from + 1,
                    text.subsequence(at + segments[from]@.len(), text.len()))
        }
    }
}

// Byte offsets.

/// The byte offset of `count` characters into `text`.
#[logic(open)]
pub fn offset(text: Seq<char>, count: Int) -> Int {
    pearlite! { text.subsequence(0, count).to_bytes().len() }
}
