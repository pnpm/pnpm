//! How a string's UTF-8 bytes follow its characters, for contracts that
//! count in bytes as `str` indices do.

use creusot_std::{
    prelude::{
        Int,
        Seq,
        ensures,
        logic,
        requires,
        trusted,
        variant,
    },
    std::char::CharExt,
};

/// Whether `offset` bytes into `text`'s encoding is the start or the end of
/// a character, which is where a `str` can be cut.
#[logic(open)]
pub fn is_boundary(text: Seq<char>, offset: Int) -> bool {
    pearlite! {
        exists<count> 0 <= count && count <= text.len()
            && text.subsequence(0, count).to_bytes().len() == offset
    }
}

/// The start and the end of a string are boundaries.
#[logic]
#[ensures(text.subsequence(0, 0).ext_eq(Seq::empty()))]
#[ensures(text.subsequence(0, 0).to_bytes().len() == 0)]
#[ensures(text.subsequence(0, text.len()).ext_eq(text))]
#[ensures(is_boundary(text, 0) && is_boundary(text, text.to_bytes().len()))]
pub fn ends_are_boundaries(text: Seq<char>) {}

/// The bytes of two strings in a row are the bytes of each in turn.
#[logic]
#[ensures(left.concat(right).to_bytes() == left.to_bytes().concat(right.to_bytes()))]
pub fn to_bytes_concat(left: Seq<char>, right: Seq<char>) {
    pearlite! { left.concat_flat_map(right, |character: char| character.to_utf8()) }
}

/// A string of one character is that character's encoding.
#[logic]
#[ensures(Seq::singleton(character).to_bytes() == character.to_utf8())]
pub fn to_bytes_singleton(character: char) {
    pearlite! { Seq::singleton_flat_map(character, |character: char| character.to_utf8()) }
}

/// Every byte of a non-ASCII character's encoding is at least `0x80`, so an
/// ASCII byte in a string is always a whole character. UTF-8 is defined that
/// way, and `tests.rs` checks it for every `char`.
#[trusted]
#[logic]
#[ensures(character@ > 127 ==> forall<index> 0 <= index && index < character.to_utf8().len()
    ==> character.to_utf8()[index]@ >= 128)]
pub fn non_ascii_bytes(character: char) {}

/// How many of `text`'s characters are encoded before byte `offset` ends.
#[logic(open)]
#[variant(text.len())]
pub fn chars_before(text: Seq<char>, offset: Int) -> Int {
    pearlite! {
        if text.len() == 0 || offset < text[0].to_utf8().len() {
            0
        } else {
            chars_before(text.tail(), offset - text[0].to_utf8().len()) + 1
        }
    }
}

/// An ASCII byte of `text`'s encoding is the whole of the character
/// `chars_before` counts to, so the offsets before and after it are
/// boundaries.
#[logic]
#[variant(text.len())]
#[requires(0 <= offset && offset < text.to_bytes().len())]
#[requires(text.to_bytes()[offset]@ < 128)]
#[ensures(0 <= chars_before(text, offset) && chars_before(text, offset) < text.len())]
#[ensures(text[chars_before(text, offset)]@ == text.to_bytes()[offset]@)]
#[ensures(text.subsequence(0, chars_before(text, offset)).to_bytes().len() == offset)]
#[ensures(text.subsequence(0, chars_before(text, offset) + 1).to_bytes().len() == offset + 1)]
#[ensures(is_boundary(text, offset) && is_boundary(text, offset + 1))]
pub fn ascii_byte(text: Seq<char>, offset: Int) {
    pearlite! {
        first_and_rest(text);
        if offset < text[0].to_utf8().len() {
            non_ascii_bytes(text[0])
        } else {
            ascii_byte(text.tail(), offset - text[0].to_utf8().len());
            prefix_bytes_after_first(text, chars_before(text.tail(), offset - text[0].to_utf8().len()));
            prefix_bytes_after_first(
                text,
                chars_before(text.tail(), offset - text[0].to_utf8().len()) + 1,
            )
        }
    }
}

/// A non-empty string is its first character's bytes and then the rest's.
#[logic]
#[requires(text.len() > 0)]
#[ensures(text.ext_eq(Seq::singleton(text[0]).concat(text.tail())))]
#[ensures(text.to_bytes() == text[0].to_utf8().concat(text.tail().to_bytes()))]
#[ensures(text.subsequence(0, 0).ext_eq(Seq::empty()))]
#[ensures(text.subsequence(0, 0).to_bytes().len() == 0)]
#[ensures(text.subsequence(0, 1).ext_eq(Seq::singleton(text[0])))]
#[ensures(text.subsequence(0, 1).to_bytes() == text[0].to_utf8())]
pub fn first_and_rest(text: Seq<char>) {
    pearlite! {
        to_bytes_concat(Seq::singleton(text[0]), text.tail());
        to_bytes_singleton(text[0])
    }
}

/// The first `count + 1` characters of `text` are its first character and
/// then the first `count` of the rest.
#[logic]
#[requires(text.len() > 0 && 0 <= count && count < text.len())]
#[ensures(text.subsequence(0, count + 1)
    .ext_eq(Seq::singleton(text[0]).concat(text.tail().subsequence(0, count))))]
#[ensures(text.subsequence(0, count + 1)
    == Seq::singleton(text[0]).concat(text.tail().subsequence(0, count)))]
pub fn prefix_after_first(text: Seq<char>, count: Int) {}

/// The bytes of the first `count + 1` characters of `text` are its first
/// character's and then those of the first `count` of the rest.
#[logic]
#[requires(text.len() > 0 && 0 <= count && count < text.len())]
#[ensures(text.subsequence(0, count + 1).to_bytes()
    == text[0].to_utf8().concat(text.tail().subsequence(0, count).to_bytes()))]
pub fn prefix_bytes_after_first(text: Seq<char>, count: Int) {
    pearlite! {
        prefix_after_first(text, count);
        to_bytes_concat(Seq::singleton(text[0]), text.tail().subsequence(0, count));
        to_bytes_singleton(text[0])
    }
}
