//! The lemmas that show the segment walk decides what a glob matches.

use super::logic::{
    after_star,
    ends_with,
    glob,
    in_order,
    joined,
    no_star_in,
    offset,
    stars_after,
};
use creusot_std::{
    logic::such_that,
    prelude::{
        Int,
        Seq,
        ensures,
        logic,
        requires,
        variant,
    },
};
use pnpm_creusot_models::{
    str_model::occurs_at,
    utf8::{
        is_boundary,
        to_bytes_concat,
    },
};

// Characters.

/// `text[first..][second..]` is `text[first + second..]`.
#[logic]
#[requires(0 <= first && first <= text.len())]
#[requires(0 <= second && second <= text.len() - first)]
#[ensures(text.subsequence(first, text.len()).subsequence(second, text.len() - first)
    .ext_eq(text.subsequence(first + second, text.len())))]
#[ensures(text.subsequence(first, text.len()).subsequence(second, text.len() - first)
    == text.subsequence(first + second, text.len()))]
pub fn suffix_of_suffix(text: Seq<char>, first: Int, second: Int) {}

/// `text[skip..][at..at + len]` is `text[skip + at..skip + at + len]`.
#[logic]
#[requires(0 <= skip && skip <= text.len() && 0 <= at && 0 <= len)]
#[requires(at + len <= text.len() - skip)]
#[ensures(text.subsequence(skip, text.len()).subsequence(at, at + len)
    .ext_eq(text.subsequence(skip + at, skip + at + len)))]
#[ensures(text.subsequence(skip, text.len()).subsequence(at, at + len)
    == text.subsequence(skip + at, skip + at + len))]
pub fn range_of_suffix(text: Seq<char>, skip: Int, at: Int, len: Int) {}

/// `text[..end][at..at + len]` is `text[at..at + len]`.
#[logic]
#[requires(0 <= at && 0 <= len && at + len <= end && end <= text.len())]
#[ensures(text.subsequence(0, end).subsequence(at, at + len)
    .ext_eq(text.subsequence(at, at + len)))]
#[ensures(text.subsequence(0, end).subsequence(at, at + len) == text.subsequence(at, at + len))]
pub fn range_of_prefix(text: Seq<char>, end: Int, at: Int, len: Int) {}

/// `text[cut..][..len - cut - tail]` and `text[..len - tail][cut..]` are both
/// `text[cut..len - tail]`.
#[logic]
#[requires(0 <= cut && 0 <= tail && cut + tail <= text.len())]
#[ensures(text.subsequence(cut, text.len()).subsequence(0, text.len() - cut - tail)
    .ext_eq(text.subsequence(cut, text.len() - tail)))]
#[ensures(text.subsequence(0, text.len() - tail).subsequence(cut, text.len() - tail)
    .ext_eq(text.subsequence(cut, text.len() - tail)))]
#[ensures(text.subsequence(cut, text.len()).subsequence(0, text.len() - cut - tail)
    == text.subsequence(0, text.len() - tail).subsequence(cut, text.len() - tail))]
pub fn middle_of(text: Seq<char>, cut: Int, tail: Int) {}

/// A text's first character and tail, against a pattern's.
#[logic]
#[requires(part.len() > 0)]
#[ensures(forall<text: Seq<char>> text.len() > 0 ==>
    occurs_at(text, part, 0)
        == (text[0] == part[0] && occurs_at(text.tail(), part.tail(), 0)))]
#[ensures(forall<text: Seq<char>> part.len() <= text.len() ==>
    text.tail().subsequence(part.len() - 1, text.len() - 1)
        == text.subsequence(part.len(), text.len()))]
pub fn head_and_tail(part: Seq<char>) {
    pearlite! { head_and_tail_ext(part) }
}

/// [`head_and_tail`], stated with [`Seq::ext_eq`].
#[logic]
#[requires(part.len() > 0)]
#[ensures(forall<text: Seq<char>> text.len() > 0 && part.len() <= text.len() ==>
    text.subsequence(0, part.len()).ext_eq(part)
        == (text[0] == part[0]
            && text.tail().subsequence(0, part.len() - 1).ext_eq(part.tail())))]
#[ensures(forall<text: Seq<char>> part.len() <= text.len() ==>
    text.tail().subsequence(part.len() - 1, text.len() - 1)
        .ext_eq(text.subsequence(part.len(), text.len())))]
pub fn head_and_tail_ext(part: Seq<char>) {}

/// The empty text occurs at the start of every text, and cutting nothing off
/// leaves the text.
#[logic]
#[ensures(forall<text: Seq<char>> text.subsequence(0, 0).ext_eq(Seq::empty()))]
#[ensures(forall<text: Seq<char>> occurs_at(text, Seq::empty(), 0))]
#[ensures(forall<text: Seq<char>> text.subsequence(0, text.len()).ext_eq(text))]
#[ensures(forall<text: Seq<char>> text.subsequence(0, text.len()) == text)]
pub fn empty_part() {}

/// `[c]` and then `rest` starts with `c`, and its tail is `rest`.
#[logic]
#[ensures(Seq::singleton(first).concat(rest)[0] == first)]
#[ensures(Seq::singleton(first).concat(rest).tail().ext_eq(rest))]
#[ensures(Seq::singleton(first).concat(rest).tail() == rest)]
pub fn cons_tail(first: char, rest: Seq<char>) {}

/// A non-empty `front` and then `rest` starts as `front` does, and its tail
/// is the rest of `front` and then `rest`.
#[logic]
#[requires(front.len() > 0)]
#[ensures(front.concat(rest)[0] == front[0])]
#[ensures(front.concat(rest).tail().ext_eq(front.tail().concat(rest)))]
#[ensures(front.concat(rest).tail() == front.tail().concat(rest))]
pub fn tail_of_concat(front: Seq<char>, rest: Seq<char>) {}

/// An empty text is [`Seq::empty`].
#[logic]
#[requires(text.len() == 0)]
#[ensures(text.ext_eq(Seq::empty()))]
#[ensures(text == Seq::empty())]
pub fn empty_text(text: Seq<char>) {}

// Globs.

/// A `*`-free `part` and then `rest` matches what starts with `part` and
/// goes on to match `rest`.
#[logic]
#[variant(part.len())]
#[requires(!part.contains('*'))]
#[ensures(forall<text: Seq<char>> glob(part.concat(rest), text)
    == (occurs_at(text, part, 0) && glob(rest, text.subsequence(part.len(), text.len()))))]
pub fn prefix_text(part: Seq<char>, rest: Seq<char>) {
    pearlite! {
        empty_part();
        if part.len() > 0 {
            prefix_text(part.tail(), rest);
            tail_of_concat(part, rest);
            head_and_tail(part)
        } else {
            empty_text(part);
            rest.concat_empty()
        }
    }
}

/// A `*`-free text matches exactly itself.
#[logic]
#[requires(!part.contains('*'))]
#[ensures(forall<text: Seq<char>> glob(part, text) == (text == part))]
pub fn exact_text(part: Seq<char>) {
    pearlite! {
        prefix_text(part, Seq::empty());
        part.concat_empty();
        empty_part()
    }
}

/// `*` and then `rest` matches `text` exactly when `rest` matches a suffix of
/// it.
#[logic]
#[ensures(glob(Seq::singleton('*').concat(rest), text) == exists<skip: Int> 0 <= skip
    && skip <= text.len() && glob(rest, text.subsequence(skip, text.len())))]
pub fn star_glob(rest: Seq<char>, text: Seq<char>) {
    pearlite! { cons_tail('*', rest) }
}

/// `*` and then `rest` matches `text` when `rest` matches `text[skip..]`.
#[logic]
#[requires(0 <= skip && skip <= text.len() && glob(rest, text.subsequence(skip, text.len())))]
#[ensures(glob(Seq::singleton('*').concat(rest), text))]
pub fn star_then(rest: Seq<char>, text: Seq<char>, skip: Int) {
    pearlite! { star_glob(rest, text) }
}

/// A suffix of `text` that `pattern` matches.
#[logic]
#[requires(0 <= skip && skip <= text.len() && glob(pattern, text.subsequence(skip, text.len())))]
#[ensures(exists<other: Int> 0 <= other && other <= text.len()
    && glob(pattern, text.subsequence(other, text.len())))]
pub fn glob_witness(pattern: Seq<char>, text: Seq<char>, skip: Int) {}

/// An occurrence of `segments[from]` at `at` with the rest after it.
#[logic]
#[requires(0 <= from && from + 1 < segments.len())]
#[requires(occurs_at(text, segments[from]@, at))]
#[requires(after_star(segments, from + 1, text.subsequence(at + segments[from]@.len(), text.len())))]
#[ensures(after_star(segments, from, text))]
pub fn after_star_witness(segments: Seq<String>, from: Int, text: Seq<char>, at: Int) {}

/// [`after_star`] is `*` and then `segments[from..]` joined by `*`.
#[logic]
#[variant(segments.len() - from)]
#[requires(0 <= from && from < segments.len() && no_star_in(segments, from, segments.len()))]
#[ensures(after_star(segments, from, text) == exists<skip: Int> 0 <= skip && skip <= text.len()
    && glob(joined(segments, from), text.subsequence(skip, text.len())))]
pub fn glob_after_star(segments: Seq<String>, from: Int, text: Seq<char>) {
    pearlite! {
        exact_text(segments[from]@);
        if from + 1 < segments.len() {
            let part = segments[from]@;
            let rest = joined(segments, from + 1);
            prefix_text(part, Seq::singleton('*').concat(rest));
            part.concat_assoc(Seq::singleton('*'), rest);
            if after_star(segments, from, text) {
                let at = such_that(|at: Int| occurs_at(text, part, at)
                    && after_star(segments, from + 1, text.subsequence(at + part.len(), text.len())));
                let after = text.subsequence(at + part.len(), text.len());
                glob_after_star(segments, from + 1, after);
                let skip = such_that(|skip: Int| 0 <= skip && skip <= after.len()
                    && glob(rest, after.subsequence(skip, after.len())));
                star_then(rest, after, skip);
                suffix_of_suffix(text, at, part.len());
                range_of_suffix(text, at, 0, part.len());
                glob_witness(joined(segments, from), text, at)
            } else if exists<skip: Int> 0 <= skip && skip <= text.len()
                && glob(joined(segments, from), text.subsequence(skip, text.len()))
            {
                let skip = such_that(|skip: Int| 0 <= skip && skip <= text.len()
                    && glob(joined(segments, from), text.subsequence(skip, text.len())));
                let after = text.subsequence(skip + part.len(), text.len());
                suffix_of_suffix(text, skip, part.len());
                range_of_suffix(text, skip, 0, part.len());
                star_glob(rest, after);
                glob_after_star(segments, from + 1, after);
                after_star_witness(segments, from, text, skip)
            } else {
                ()
            }
        } else {
            ()
        }
    }
}

/// [`after_star`] is ending with the last segment and holding
/// `segments[from..last]` in order before it.
#[logic]
#[variant(segments.len() - from)]
#[requires(0 <= from && from < segments.len())]
#[ensures(after_star(segments, from, text) == (ends_with(text, segments[segments.len() - 1]@)
    && in_order(segments, from, segments.len() - 1,
        text.subsequence(0, text.len() - segments[segments.len() - 1]@.len()))))]
pub fn after_star_split(segments: Seq<String>, from: Int, text: Seq<char>) {
    pearlite! {
        if from + 1 < segments.len() {
            let part = segments[from]@;
            let last = segments[segments.len() - 1]@;
            let front = text.subsequence(0, text.len() - last.len());
            if after_star(segments, from, text) {
                let at = such_that(|at: Int| occurs_at(text, part, at)
                    && after_star(segments, from + 1, text.subsequence(at + part.len(), text.len())));
                after_star_split(segments, from + 1, text.subsequence(at + part.len(), text.len()));
                middle_of(text, at + part.len(), last.len());
                suffix_of_suffix(text, at + part.len(), text.len() - at - part.len() - last.len());
                range_of_prefix(text, text.len() - last.len(), at, part.len())
            } else if ends_with(text, last) && in_order(segments, from, segments.len() - 1, front) {
                let at = such_that(|at: Int| occurs_at(front, part, at)
                    && in_order(segments, from + 1, segments.len() - 1,
                        front.subsequence(at + part.len(), front.len())));
                after_star_split(segments, from + 1, text.subsequence(at + part.len(), text.len()));
                middle_of(text, at + part.len(), last.len());
                suffix_of_suffix(text, at + part.len(), text.len() - at - part.len() - last.len());
                range_of_prefix(text, text.len() - last.len(), at, part.len())
            } else {
                ()
            }
        } else {
            ()
        }
    }
}

/// Segments in order in a suffix of `text` are in order in `text`.
#[logic]
#[requires(0 <= skip && skip <= text.len())]
#[ensures(in_order(segments, from, to, text.subsequence(skip, text.len()))
    ==> in_order(segments, from, to, text))]
pub fn in_order_of_suffix(segments: Seq<String>, from: Int, to: Int, text: Seq<char>, skip: Int) {
    pearlite! {
        if from < to && in_order(segments, from, to, text.subsequence(skip, text.len())) {
            let part = segments[from]@;
            let suffix = text.subsequence(skip, text.len());
            let at = such_that(|at: Int| occurs_at(suffix, part, at)
                && in_order(segments, from + 1, to, suffix.subsequence(at + part.len(), suffix.len())));
            range_of_suffix(text, skip, at, part.len());
            suffix_of_suffix(text, skip, at + part.len())
        } else {
            ()
        }
    }
}

/// When `segments[index]` first occurs in `text` at `count`, the rest of
/// the segments are in order after that occurrence whenever they are after
/// any occurrence.
#[logic]
#[requires(0 <= index && index < to)]
#[requires(occurs_at(text, segments[index]@, count))]
#[requires(forall<earlier: Int> earlier < count ==> !occurs_at(text, segments[index]@, earlier))]
#[ensures(in_order(segments, index, to, text) == in_order(segments, index + 1, to,
    text.subsequence(count + segments[index]@.len(), text.len())))]
pub fn first_occurrence(segments: Seq<String>, index: Int, to: Int, text: Seq<char>, count: Int) {
    pearlite! {
        if in_order(segments, index, to, text) {
            let part = segments[index]@;
            let at = such_that(|at: Int| occurs_at(text, part, at)
                && in_order(segments, index + 1, to, text.subsequence(at + part.len(), text.len())));
            suffix_of_suffix(text, count + part.len(), at - count);
            in_order_of_suffix(segments, index + 1, to,
                text.subsequence(count + part.len(), text.len()), at - count)
        } else {
            ()
        }
    }
}

/// The whole pattern of two or more segments is the first segment at the
/// start and [`after_star`] of the rest.
#[logic]
#[requires(segments.len() >= 2 && no_star_in(segments, 0, segments.len()))]
#[ensures(glob(joined(segments, 0), text) == (occurs_at(text, segments[0]@, 0)
    && after_star(segments, 1, text.subsequence(segments[0]@.len(), text.len()))))]
pub fn first_and_rest(segments: Seq<String>, text: Seq<char>) {
    pearlite! {
        let part = segments[0]@;
        let rest = joined(segments, 1);
        prefix_text(part, Seq::singleton('*').concat(rest));
        part.concat_assoc(Seq::singleton('*'), rest);
        cons_tail('*', rest);
        glob_after_star(segments, 1, text.subsequence(part.len(), text.len()))
    }
}

// Byte offsets.

/// Cutting `text` `count` characters in: the byte offset there is a
/// boundary within the text, and the bytes after it encode `text[count..]`.
#[logic]
#[requires(0 <= count && count <= text.len())]
#[ensures(offset(text, count) <= text.to_bytes().len())]
#[ensures(is_boundary(text, offset(text, count)))]
#[ensures(forall<rest: Seq<char>>
    rest.to_bytes() == text.to_bytes().subsequence(offset(text, count), text.to_bytes().len())
        ==> rest == text.subsequence(count, text.len()))]
pub fn cut_at(text: Seq<char>, count: Int) {
    pearlite! {
        split_ext(text, count);
        to_bytes_concat(text.subsequence(0, count), text.subsequence(count, text.len()));
        bytes_after(text.subsequence(0, count).to_bytes(), text.subsequence(count, text.len()).to_bytes());
        creusot_std::std::string::injective_to_bytes()
    }
}

/// `text` is its first `count` characters and then the rest.
#[logic]
#[requires(0 <= count && count <= text.len())]
#[ensures(text.subsequence(0, count).concat(text.subsequence(count, text.len())).ext_eq(text))]
#[ensures(text.subsequence(0, count).concat(text.subsequence(count, text.len())) == text)]
pub fn split_ext(text: Seq<char>, count: Int) {}

/// The bytes of `front` and then `back`, after `front`'s, are `back`'s.
#[logic]
#[ensures(front.concat(back).subsequence(front.len(), front.len() + back.len()).ext_eq(back))]
#[ensures(front.concat(back).subsequence(front.len(), front.len() + back.len()) == back)]
pub fn bytes_after(front: Seq<u8>, back: Seq<u8>) {}

/// The byte offset after an occurrence of `part` at `count` is the offset of
/// `count` and then `part`'s bytes.
#[logic]
#[requires(occurs_at(text, part, count))]
#[ensures(offset(text, count + part.len()) == offset(text, count) + part.to_bytes().len())]
pub fn offset_after(text: Seq<char>, part: Seq<char>, count: Int) {
    pearlite! {
        prefix_then_part(text, part, count);
        to_bytes_concat(text.subsequence(0, count), part)
    }
}

/// `text[..count + part.len()]` is `text[..count]` and then `part`, when
/// `part` occurs at `count`.
#[logic]
#[requires(occurs_at(text, part, count))]
#[ensures(text.subsequence(0, count + part.len())
    .ext_eq(text.subsequence(0, count).concat(part)))]
#[ensures(text.subsequence(0, count + part.len()) == text.subsequence(0, count).concat(part))]
pub fn prefix_then_part(text: Seq<char>, part: Seq<char>, count: Int) {}

/// Bounds the byte length of a string.
#[logic]
#[ensures(text@.to_bytes().len() <= isize::MAX@)]
pub fn str_size(text: &str) {
    pearlite! { pnpm_creusot_models::str_model::str_size_bound(text) }
}

// Building the segments.

/// Appending a segment appends it, and then a `*`, to [`stars_after`].
#[logic]
#[variant(segments.len() - from)]
#[requires(0 <= from && from <= segments.len())]
#[ensures(stars_after(segments.snoc(last), from, segments.len() + 1)
    == stars_after(segments, from, segments.len()).concat(last@).concat(Seq::singleton('*')))]
pub fn stars_after_snoc(segments: Seq<String>, last: String, from: Int) {
    pearlite! {
        if from < segments.len() {
            let head = segments[from]@.concat(Seq::singleton('*'));
            let middle = stars_after(segments, from + 1, segments.len());
            stars_after_snoc(segments, last, from + 1);
            head.concat(middle).concat_assoc(last@, Seq::singleton('*'));
            head.concat_assoc(middle, last@.concat(Seq::singleton('*')));
            middle.concat_assoc(last@, Seq::singleton('*'))
        } else {
            last@.concat(Seq::singleton('*')).concat_empty();
            last@.concat_empty()
        }
    }
}

/// Appending a segment with no `*` to segments with none keeps them free of
/// `*`.
#[logic]
#[ensures(no_star_in(segments, 0, segments.len()) && !last@.contains('*') ==>
    no_star_in(segments.snoc(last), 0, segments.len() + 1))]
pub fn no_star_snoc(segments: Seq<String>, last: String) {}

/// Joining segments with one appended is [`stars_after`] of the rest and
/// then the last.
#[logic]
#[variant(segments.len() - from)]
#[requires(0 <= from && from <= segments.len())]
#[ensures(joined(segments.snoc(last), from)
    == stars_after(segments, from, segments.len()).concat(last@))]
pub fn joined_snoc(segments: Seq<String>, last: String, from: Int) {
    pearlite! {
        if from < segments.len() {
            joined_snoc(segments, last, from + 1);
            segments[from]@.concat(Seq::singleton('*'))
                .concat_assoc(stars_after(segments, from + 1, segments.len()), last@)
        } else {
            last@.concat_empty()
        }
    }
}

// The walk.

/// `front` and then `back` holds `front` at its start and `back` after it.
#[logic]
#[ensures(front.concat(back).subsequence(0, front.len()).ext_eq(front))]
#[ensures(front.concat(back).subsequence(front.len(), front.len() + back.len()).ext_eq(back))]
#[ensures(occurs_at(front.concat(back), front, 0))]
#[ensures(occurs_at(front.concat(back), back, front.len()))]
#[ensures(front.concat(back).subsequence(front.len(), front.len() + back.len()) == back)]
#[ensures(front.concat(back).subsequence(0, front.len()) == front)]
pub fn concat_parts(front: Seq<char>, back: Seq<char>) {}

/// Stepping past the first occurrence of `segments[index]`, at `count`
/// characters and `offset(text, count)` bytes into `text`.
#[logic]
#[requires(0 <= index && index < to)]
#[requires(occurs_at(text, segments[index]@, count))]
#[requires(forall<earlier: Int> earlier < count ==> !occurs_at(text, segments[index]@, earlier))]
#[ensures(offset(text, count) + segments[index]@.to_bytes().len()
    == offset(text, count + segments[index]@.len()))]
#[ensures(offset(text, count + segments[index]@.len()) <= text.to_bytes().len())]
#[ensures(is_boundary(text, offset(text, count + segments[index]@.len())))]
#[ensures(forall<rest: Seq<char>> rest.to_bytes() == text.to_bytes()
    .subsequence(offset(text, count + segments[index]@.len()), text.to_bytes().len())
    ==> in_order(segments, index + 1, to, rest) == in_order(segments, index, to, text))]
pub fn advance(segments: Seq<String>, index: Int, to: Int, text: Seq<char>, count: Int) {
    pearlite! {
        offset_after(text, segments[index]@, count);
        cut_at(text, count + segments[index]@.len());
        first_occurrence(segments, index, to, text, count)
    }
}
