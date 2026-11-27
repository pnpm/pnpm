//! The [`str`], [`String`] and [`char`] methods the verified crates call.

use core::str::pattern::Pattern;
use creusot_std::prelude::{
    Seq,
    ensures,
    extern_spec,
    logic,
    trusted,
};

/// A pattern that matches a set of characters: the patterns the [`str`]
/// contracts here are specified for.
pub trait CharPattern {
    /// Whether the pattern matches `character`.
    #[logic]
    fn matches(self, character: char) -> bool;
}

impl CharPattern for char {
    #[logic(open)]
    fn matches(self, character: char) -> bool {
        pearlite! { self == character }
    }
}

impl CharPattern for [char; 2] {
    #[logic(open)]
    fn matches(self, character: char) -> bool {
        pearlite! { self@[0] == character || self@[1] == character }
    }
}

/// No character of `text` matches `pattern`.
#[logic(open)]
pub fn free_of<Pattern: CharPattern>(text: Seq<char>, pattern: Pattern) -> bool {
    pearlite! {
        forall<index> 0 <= index && index < text.len() ==> !pattern.matches(text[index])
    }
}

/// Whether `character` is an ASCII letter.
#[logic(open)]
pub fn is_ascii_letter(character: char) -> bool {
    pearlite! {
        (65 <= character@ && character@ <= 90) || (97 <= character@ && character@ <= 122)
    }
}

extern_spec! {
    impl str {
        #[ensures(match result {
            None => free_of(self@, delimiter),
            Some((before, after)) => before@.len() < self@.len()
                && delimiter.matches(self@[before@.len()])
                && self@ == before@.concat(Seq::singleton(self@[before@.len()])).concat(after@)
                && free_of(before@, delimiter),
        })]
        fn split_once<P: Pattern>(&self, delimiter: P) -> Option<(&str, &str)>
            where P: CharPattern;

        #[ensures(result == (self@.len() > 0 && pat.matches(self@[0])))]
        fn starts_with<P: Pattern>(&self, pat: P) -> bool
            where P: CharPattern;

        #[ensures(result == !free_of(self@, pat))]
        fn contains<P: Pattern>(&self, pat: P) -> bool
            where P: CharPattern;
    }

    impl String {
        #[ensures(result@ == Seq::empty())]
        fn new() -> String;

        #[ensures((^self)@ == (*self)@.concat(Seq::singleton(ch)))]
        fn push(&mut self, ch: char);

        #[ensures((^self)@ == (*self)@.concat(string@))]
        fn push_str(&mut self, string: &str);
    }

    impl char {
        #[ensures(result == is_ascii_letter(*self))]
        fn is_ascii_alphabetic(&self) -> bool;
    }
}

/// A string holds at most `isize::MAX` bytes. The Rust Reference bounds the
/// size of every value by `isize::MAX`, and no test can reach that size, so
/// this is the one axiom here with nothing to check it against.
#[trusted]
#[logic]
#[ensures(text@.to_bytes().len() <= isize::MAX@)]
pub fn str_size_bound(text: &str) {}
