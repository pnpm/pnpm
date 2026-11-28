//! The lemmas the split proofs call: where a split's bytes fall, and why
//! each split is unique.

use creusot_std::{
    prelude::{
        Int,
        Seq,
        ensures,
        logic,
        requires,
    },
    std::string::injective_to_bytes,
};
use pnpm_creusot_models::{
    str_model::str_size_bound,
    utf8::{
        to_bytes_concat,
        to_bytes_singleton,
    },
};

/// Two ways of cutting one sequence whose first parts have one length share
/// that first part.
#[logic]
#[requires(first.concat(rest) == other_first.concat(other_rest))]
#[requires(first.len() == other_first.len())]
#[ensures(first.ext_eq(other_first))]
#[ensures(first == other_first)]
pub fn equal_prefixes<T>(first: Seq<T>, rest: Seq<T>, other_first: Seq<T>, other_rest: Seq<T>) {}

/// Two ways of cutting one string whose first parts have as many bytes
/// cut it in the same place.
#[logic]
#[requires(first.concat(rest) == other_first.concat(other_rest))]
#[requires(first.to_bytes().len() == other_first.to_bytes().len())]
#[ensures(first == other_first)]
pub fn equal_byte_prefixes(
    first: Seq<char>,
    rest: Seq<char>,
    other_first: Seq<char>,
    other_rest: Seq<char>,
) {
    pearlite! {
        to_bytes_concat(first, rest);
        to_bytes_concat(other_first, other_rest);
        equal_prefixes(
            first.to_bytes(),
            rest.to_bytes(),
            other_first.to_bytes(),
            other_rest.to_bytes(),
        );
        injective_to_bytes()
    }
}

/// `@{name_without_at}` is one byte longer than `name_without_at`, and a
/// key that holds both and another `@` leaves room to add that byte.
#[logic]
#[requires(key@ == Seq::singleton('@').concat(rest))]
#[requires(rest == name_without_at.concat(Seq::singleton('@')).concat(suffix))]
#[ensures(key@.subsequence(0, name_without_at.len() + 1)
    == Seq::singleton('@').concat(name_without_at))]
#[ensures(Seq::singleton('@').concat(name_without_at).to_bytes().len()
    == name_without_at.to_bytes().len() + 1)]
#[ensures(name_without_at.to_bytes().len() + 1 <= usize::MAX@)]
#[ensures(key@ == Seq::singleton('@').concat(name_without_at)
    .concat(Seq::singleton('@').concat(suffix)))]
pub fn scoped_name_bytes(
    key: &str,
    rest: Seq<char>,
    name_without_at: Seq<char>,
    suffix: Seq<char>,
) {
    pearlite! {
        str_size_bound(key);
        to_bytes_singleton('@');
        to_bytes_concat(Seq::singleton('@'), name_without_at);
        to_bytes_concat(
            Seq::singleton('@').concat(name_without_at),
            Seq::singleton('@').concat(suffix),
        );
        to_bytes_concat(Seq::singleton('@'), suffix);
        name_without_at.concat_assoc(Seq::singleton('@'), suffix);
        Seq::singleton('@').concat_assoc(name_without_at, Seq::singleton('@').concat(suffix))
    }
}

/// `separator` at `index` puts it in `text`.
#[logic]
#[requires(0 <= index && index < text.len() && text[index] == separator)]
#[ensures(text.contains(separator))]
pub fn contains_at(text: Seq<char>, index: Int, separator: char) {}

/// Where two cuts of one string disagree, the shorter first part's
/// separator falls inside the longer one.
#[logic]
#[requires(first.concat(Seq::singleton(separator)).concat(rest)
    == other_first.concat(Seq::singleton(separator)).concat(other_rest))]
#[requires(first.len() < other_first.len())]
#[ensures(other_first[first.len()] == separator)]
pub fn separator_inside(
    first: Seq<char>,
    rest: Seq<char>,
    other_first: Seq<char>,
    other_rest: Seq<char>,
    separator: char,
) {
}

/// Each character of `tail` sits `head.len()` further into `head` and
/// `tail` joined.
#[logic]
#[ensures(forall<index> 0 <= index && index < tail.len() ==>
    head.concat(tail)[head.len() + index] == tail[index])]
pub fn tail_index(head: Seq<char>, tail: Seq<char>) {}

/// Two cuts of one string with first parts of one length are one cut.
#[logic]
#[requires(first.concat(Seq::singleton(separator)).concat(rest)
    == other_first.concat(Seq::singleton(separator)).concat(other_rest))]
#[requires(first.len() == other_first.len())]
#[ensures(first == other_first && rest == other_rest)]
pub fn same_cut(
    first: Seq<char>,
    rest: Seq<char>,
    other_first: Seq<char>,
    other_rest: Seq<char>,
    separator: char,
) {
    pearlite! {
        tail_index(first.concat(Seq::singleton(separator)), rest);
        tail_index(other_first.concat(Seq::singleton(separator)), other_rest)
    }
}

/// A string splits around `separator` in one place at most where the part
/// before it is non-empty and holds no `separator` after its first
/// character.
#[logic]
#[requires(first.len() > 0 && !first.subsequence(1, first.len()).contains(separator))]
#[requires(other_first.len() > 0
    && !other_first.subsequence(1, other_first.len()).contains(separator))]
#[requires(first.concat(Seq::singleton(separator)).concat(rest)
    == other_first.concat(Seq::singleton(separator)).concat(other_rest))]
#[ensures(first == other_first && rest == other_rest)]
pub fn split_is_unique(
    first: Seq<char>,
    rest: Seq<char>,
    other_first: Seq<char>,
    other_rest: Seq<char>,
    separator: char,
) {
    pearlite! {
        if first.len() < other_first.len() {
            separator_inside(first, rest, other_first, other_rest, separator);
            contains_at(other_first.subsequence(1, other_first.len()), first.len() - 1, separator)
        } else if other_first.len() < first.len() {
            separator_inside(other_first, other_rest, first, rest, separator);
            contains_at(first.subsequence(1, first.len()), other_first.len() - 1, separator)
        } else {
            same_cut(first, rest, other_first, other_rest, separator)
        }
    }
}

/// `@{scope}/{bare}` splits in one place at most where the scope holds no
/// `/`.
#[logic]
#[requires(!scope.contains('/') && !other_scope.contains('/'))]
#[requires(Seq::singleton('@').concat(scope).concat(Seq::singleton('/')).concat(bare)
    == Seq::singleton('@').concat(other_scope).concat(Seq::singleton('/')).concat(other_bare))]
#[ensures(scope == other_scope && bare == other_bare)]
pub fn scope_split_is_unique(
    scope: Seq<char>,
    bare: Seq<char>,
    other_scope: Seq<char>,
    other_bare: Seq<char>,
) {
    pearlite! {
        if scope.len() < other_scope.len() {
            separator_inside(
                Seq::singleton('@').concat(scope), bare,
                Seq::singleton('@').concat(other_scope), other_bare, '/',
            );
            contains_at(other_scope, scope.len(), '/')
        } else if other_scope.len() < scope.len() {
            separator_inside(
                Seq::singleton('@').concat(other_scope), other_bare,
                Seq::singleton('@').concat(scope), bare, '/',
            );
            contains_at(scope, other_scope.len(), '/')
        } else {
            same_cut(
                Seq::singleton('@').concat(scope), bare,
                Seq::singleton('@').concat(other_scope), other_bare, '/',
            )
        }
    }
}
