//! The [`str`], [`String`] and [`char`] methods the verified crates call.

use crate::utf8::is_boundary;
use core::{
    ops::{
        Index,
        Range,
        RangeFrom,
        RangeTo,
    },
    slice::SliceIndex,
    str::pattern::Pattern,
};
use creusot_std::prelude::{
    Seq,
    ensures,
    extern_spec,
    logic,
    requires,
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

/// A byte range a [`str`] is indexed by, as in `&text[start..end]`.
pub trait StrIndex: SliceIndex<str, Output = str> {
    /// Whether the range lies in `text` and starts and ends on character
    /// boundaries, so indexing by it does not panic.
    #[logic]
    fn in_bounds(self, text: Seq<char>) -> bool;

    /// Whether `part` is what the range covers in `text`.
    #[logic]
    fn has_value(self, text: Seq<char>, part: Seq<char>) -> bool;
}

impl StrIndex for Range<usize> {
    #[logic(open)]
    fn in_bounds(self, text: Seq<char>) -> bool {
        pearlite! {
            self.start@ <= self.end@ && self.end@ <= text.to_bytes().len()
                && is_boundary(text, self.start@) && is_boundary(text, self.end@)
        }
    }

    #[logic(open)]
    fn has_value(self, text: Seq<char>, part: Seq<char>) -> bool {
        pearlite! { part.to_bytes() == text.to_bytes().subsequence(self.start@, self.end@) }
    }
}

impl StrIndex for RangeFrom<usize> {
    #[logic(open)]
    fn in_bounds(self, text: Seq<char>) -> bool {
        pearlite! { self.start@ <= text.to_bytes().len() && is_boundary(text, self.start@) }
    }

    #[logic(open)]
    fn has_value(self, text: Seq<char>, part: Seq<char>) -> bool {
        pearlite! {
            part.to_bytes() == text.to_bytes().subsequence(self.start@, text.to_bytes().len())
        }
    }
}

impl StrIndex for RangeTo<usize> {
    #[logic(open)]
    fn in_bounds(self, text: Seq<char>) -> bool {
        pearlite! { self.end@ <= text.to_bytes().len() && is_boundary(text, self.end@) }
    }

    #[logic(open)]
    fn has_value(self, text: Seq<char>, part: Seq<char>) -> bool {
        pearlite! { part.to_bytes() == text.to_bytes().subsequence(0, self.end@) }
    }
}

extern_spec! {
    impl<I: StrIndex> Index<I> for str {
        #[requires(index.in_bounds(self@))]
        #[ensures(index.has_value(self@, (*result)@))]
        fn index(&self, index: I) -> &<str as Index<I>>::Output;
    }
}

/// A string holds at most `isize::MAX` bytes. The Rust Reference bounds the
/// size of every value by `isize::MAX`, and no test can reach that size, so
/// this is the one axiom here with nothing to check it against.
#[trusted]
#[logic]
#[ensures(text@.to_bytes().len() <= isize::MAX@)]
pub fn str_size_bound(text: &str) {}
