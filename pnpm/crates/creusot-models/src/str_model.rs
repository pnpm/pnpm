//! The [`str`] methods the verified crates call.

use core::str::pattern::Pattern;
use creusot_std::prelude::{
    Seq,
    ensures,
    extern_spec,
    logic,
    trusted,
};

/// A pattern that matches exactly one character: the one contract here
/// that `str::split_once` is specified for.
pub trait CharPattern {
    /// The character the pattern matches.
    #[logic]
    fn matched(self) -> char;
}

impl CharPattern for char {
    #[logic(open)]
    fn matched(self) -> char {
        self
    }
}

extern_spec! {
    impl str {
        #[ensures(match result {
            None => !self@.contains(delimiter.matched()),
            Some((before, after)) =>
                self@ == before@.concat(Seq::singleton(delimiter.matched())).concat(after@)
                && !before@.contains(delimiter.matched()),
        })]
        fn split_once<P: Pattern>(&self, delimiter: P) -> Option<(&str, &str)>
            where P: CharPattern;
    }
}

/// A string holds at most `isize::MAX` bytes. The Rust Reference bounds the
/// size of every value by `isize::MAX`, and no test can reach that size, so
/// this is the one axiom here with nothing to check it against.
#[trusted]
#[logic]
#[ensures(text@.to_bytes().len() <= isize::MAX@)]
pub fn str_size_bound(text: &str) {}
