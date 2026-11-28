//! The logic the contracts in the crate root are stated in. Each counts in
//! bytes of the text, as the ranges the crate returns do.

use crate::{
    BACKSLASH,
    CLOSE,
    DOLLAR,
    OPEN,
    Placeholder,
};
use creusot_std::prelude::{
    Int,
    Seq,
    logic,
    variant,
};
use pnpm_creusot_models::utf8::is_boundary;

/// Whether `byte` can sit in a placeholder's body: anything but `$`, `{`
/// and `}`.
#[logic(open(crate))]
pub fn is_body_byte(byte: u8) -> bool {
    pearlite! { byte != DOLLAR && byte != OPEN && byte != CLOSE }
}

/// Whether bytes `start..end` of `bytes` are a placeholder: `${`, a
/// non-empty body, and `}`.
#[logic(open(crate))]
pub fn is_placeholder(bytes: Seq<u8>, start: Int, end: Int) -> bool {
    pearlite! {
        0 <= start && start + 3 < end && end <= bytes.len()
            && bytes[start] == DOLLAR && bytes[start + 1] == OPEN && bytes[end - 1] == CLOSE
            && forall<index> start + 2 <= index && index < end - 1 ==> is_body_byte(bytes[index])
    }
}

/// What `placeholders` knows once it has scanned `text` up to byte `index`:
/// the placeholders it found, the text since the last one starting at
/// `literal_start`, and that no placeholder starting before `index` is
/// missing.
#[logic(open(crate), inline)]
pub fn scanned(
    text: Seq<char>,
    placeholders: Seq<Placeholder<'_>>,
    literal_start: Int,
    index: Int,
) -> bool {
    pearlite! {
        literal_start <= index && index <= text.to_bytes().len()
            && literal_start == last_end(placeholders)
            && (literal_start == 0 || text.to_bytes()[literal_start - 1] == CLOSE)
            && is_boundary(text, literal_start)
            && (forall<placeholder> 0 <= placeholder && placeholder < placeholders.len() ==>
                reported(text, placeholders[placeholder], previous_end(placeholders, placeholder))
                    && placeholders[placeholder].end@ <= literal_start)
            && (forall<first, second> 0 <= first && first < second && second < placeholders.len() ==>
                placeholders[first].end@ <= placeholders[second].start@)
            && (forall<start, end> is_placeholder(text.to_bytes(), start, end) && start < index ==>
                exists<placeholder> 0 <= placeholder && placeholder < placeholders.len()
                    && placeholders[placeholder].start@ == start
                    && placeholders[placeholder].end@ == end)
    }
}

/// How many `\` come right before byte `offset` of `bytes`.
#[logic(open(crate))]
#[variant(offset)]
pub fn escapes_before(bytes: Seq<u8>, offset: Int) -> Int {
    pearlite! {
        if 0 < offset && offset <= bytes.len() && bytes[offset - 1] == BACKSLASH {
            escapes_before(bytes, offset - 1) + 1
        } else {
            0
        }
    }
}

/// Where the placeholder before the one at `index` ends, or `0` for the
/// first.
#[logic(open(crate))]
pub fn previous_end(placeholders: Seq<Placeholder<'_>>, index: Int) -> Int {
    pearlite! { if index == 0 { 0 } else { placeholders[index - 1].end@ } }
}

/// Where the last placeholder ends, or `0` when there is none.
#[logic(open(crate))]
pub fn last_end(placeholders: Seq<Placeholder<'_>>) -> Int {
    pearlite! { previous_end(placeholders, placeholders.len()) }
}

/// What `placeholders` states about each placeholder of `text` it reports,
/// given where the one before it ends: the placeholder, its escapes and the
/// text before them are bytes of `text` in that order, cut on character
/// boundaries.
#[logic(open(crate))]
pub fn reported(text: Seq<char>, placeholder: Placeholder<'_>, previous_end: Int) -> bool {
    pearlite! {
        is_placeholder(text.to_bytes(), placeholder.start@, placeholder.end@)
            && is_boundary(text, placeholder.start@ - placeholder.backslashes@)
            && is_boundary(text, placeholder.start@)
            && is_boundary(text, placeholder.end@)
            && previous_end <= placeholder.start@ - placeholder.backslashes@
            && placeholder.backslashes@ == escapes_before(text.to_bytes(), placeholder.start@)
            && (forall<index> placeholder.start@ - placeholder.backslashes@ <= index
                && index < placeholder.start@ ==> text.to_bytes()[index] == BACKSLASH)
            && placeholder.literal@.to_bytes() == text.to_bytes()
                .subsequence(previous_end, placeholder.start@ - placeholder.backslashes@)
            && placeholder.raw@.to_bytes()
                == text.to_bytes().subsequence(placeholder.start@, placeholder.end@)
            && placeholder.body@.to_bytes()
                == text.to_bytes().subsequence(placeholder.start@ + 2, placeholder.end@ - 1)
    }
}
