//! A glob of literal text and `*`, where `*` matches any run of characters,
//! the empty run included, and every other character matches itself.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that no function here panics
//! and that [`StarGlob::matches`] holds exactly when the input matches the
//! pattern [`StarGlob::new`] was given. The match takes the first occurrence
//! of each segment between two `*`s, and the proof shows that wherever a
//! later occurrence leaves room for the rest of the pattern, the first does
//! too.

#[cfg(creusot)]
pub mod spec;

#[cfg(creusot)]
use creusot_std::{
    logic::such_that,
    prelude::{
        Int,
        Seq,
        Snapshot,
        ensures,
        invariant,
        proof_assert,
        requires,
        snapshot,
    },
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
#[cfg(creusot)]
use pnpm_creusot_models::{
    str_model::occurs_at,
    utf8::is_boundary,
};
#[cfg(creusot)]
use spec::{
    after_star,
    ends_with,
    glob,
    in_order,
    offset,
};

/// A pattern split at each `*`: one more segment than it has `*`s, and no
/// segment holding a `*`.
pub struct StarGlob {
    segments: Vec<String>,
}

impl StarGlob {
    /// `pattern` split at each `*`.
    #[cfg_attr(creusot, ensures(result.pattern() == pattern@))]
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        let mut segments: Vec<String> = Vec::new();
        let mut rest = pattern;
        #[cfg_attr(creusot, invariant(spec::no_star_in(segments@, 0, segments@.len())))]
        #[cfg_attr(creusot, invariant(pattern@
            == spec::stars_after(segments@, 0, segments@.len()).concat(rest@)))]
        loop {
            let Some((segment, after)) = rest.split_once('*') else {
                #[cfg(creusot)]
                let before = snapshot! { segments };
                segments.push(rest.to_owned());
                #[cfg(creusot)]
                proof_assert! {
                    spec::joined_snoc((*before)@, segments@[before@.len()], 0);
                    spec::no_star_snoc((*before)@, segments@[before@.len()]);
                    spec::no_star_in(segments@, 0, segments@.len())
                        && spec::joined(segments@, 0) == pattern@
                };
                return StarGlob { segments };
            };
            #[cfg(creusot)]
            let before = snapshot! { segments };
            segments.push(segment.to_owned());
            #[cfg(creusot)]
            proof_assert! {
                spec::stars_after_snoc((*before)@, segments@[before@.len()], 0);
                spec::no_star_snoc((*before)@, segments@[before@.len()]);
                spec::stars_after(before@, 0, before@.len()).concat(segment@)
                    .concat_assoc(Seq::singleton('*'), after@);
                spec::stars_after(before@, 0, before@.len())
                    .concat_assoc(segment@, Seq::singleton('*').concat(after@));
                spec::no_star_in(segments@, 0, segments@.len())
                    && pattern@ == spec::stars_after(segments@, 0, segments@.len())
                        .concat(after@)
            };
            rest = after;
        }
    }

    /// Whether the whole of `input` matches the pattern.
    #[cfg_attr(creusot, ensures(result == glob(self.pattern(), input@)))]
    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        let last = self.segments.len() - 1;
        let first: &str = &self.segments[0];
        if last == 0 {
            #[cfg(creusot)]
            proof_assert! {
                spec::exact_text(first@);
                glob(self.pattern(), input@) == (input@ == first@)
            };
            return first == input;
        }
        #[cfg(creusot)]
        proof_assert! {
            spec::first_and_rest(self@, input@);
            glob(self.pattern(), input@) == (occurs_at(input@, first@, 0)
                && after_star(self@, 1, input@.subsequence(first@.len(), input@.len())))
        };
        let Some(rest) = input.strip_prefix(first) else {
            return false;
        };
        #[cfg(creusot)]
        proof_assert! {
            spec::concat_parts(first@, rest@);
            spec::after_star_split(self@, 1, rest@);
            glob(self.pattern(), input@) == (ends_with(rest@, self@[last@]@)
                && in_order(self@, 1, last@,
                    rest@.subsequence(0, rest@.len() - self@[last@]@.len())))
        };
        let Some(middle) = rest.strip_suffix(&*self.segments[last]) else {
            return false;
        };
        #[cfg(creusot)]
        proof_assert! {
            spec::concat_parts(middle@, self@[last@]@);
            glob(self.pattern(), input@) == in_order(self@, 1, last@, middle@)
        };
        contains_in_order(middle, &self.segments, 1, last)
    }
}

/// Whether `segments[from..to]` all occur in `input`, in order and without
/// overlap.
#[cfg_attr(creusot, requires(from@ <= to@ && to@ <= segments@.len()))]
#[cfg_attr(creusot, ensures(result == in_order(segments@, from@, to@, input@)))]
fn contains_in_order(mut input: &str, segments: &[String], from: usize, to: usize) -> bool {
    #[cfg(creusot)]
    let original: Snapshot<&str> = snapshot! { input };
    let mut index = from;
    #[cfg_attr(creusot, invariant(from@ <= index@ && index@ <= to@))]
    #[cfg_attr(creusot, invariant(in_order(segments@, index@, to@, input@)
        == in_order(segments@, from@, to@, original@)))]
    while index < to {
        let segment: &str = &segments[index];
        let Some(at) = input.find(segment) else {
            return false;
        };
        #[cfg(creusot)]
        let count: Snapshot<Int> = snapshot! {
            such_that(|count: Int| occurs_at(input@, segment@, count)
                && at@ == offset(input@, count)
                && forall<earlier: Int> earlier < count ==> !occurs_at(input@, segment@, earlier))
        };
        #[cfg(creusot)]
        proof_assert! {
            spec::str_size(input);
            spec::advance(segments@, index@, to@, input@, *count);
            at@ + segment@.to_bytes().len() == offset(input@, *count + segment@.len())
                && offset(input@, *count + segment@.len()) <= input@.to_bytes().len()
                && input@.to_bytes().len() <= isize::MAX@
                && is_boundary(input@, offset(input@, *count + segment@.len()))
                && forall<rest: Seq<char>> rest.to_bytes() == input@.to_bytes()
                    .subsequence(offset(input@, *count + segment@.len()), input@.to_bytes().len())
                    ==> in_order(segments@, index@ + 1, to@, rest)
                        == in_order(segments@, index@, to@, input@)
        };
        input = &input[at + segment.len()..];
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests;
