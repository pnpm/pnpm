//! The logic the contracts in the crate root are stated in, and the lemmas
//! their proofs call.

use creusot_std::prelude::{
    Int,
    Seq,
    ensures,
    logic,
    requires,
    variant,
};
use pnpm_creusot_models::str_model::is_ascii_letter;

/// Whether `text` is `.`.
#[logic(open)]
pub fn is_current(text: Seq<char>) -> bool {
    pearlite! { text == Seq::singleton('.') }
}

/// Whether `text` is `..`.
#[logic(open)]
pub fn is_parent(text: Seq<char>) -> bool {
    pearlite! { text == Seq::singleton('.').concat(Seq::singleton('.')) }
}

/// No character of `text` is `/` or `\`.
#[logic(open)]
pub fn separator_free(text: Seq<char>) -> bool {
    pearlite! {
        forall<index> 0 <= index && index < text.len() ==> text[index] != '/' && text[index] != '\\'
    }
}

/// Whether `text` starts with a Windows drive: an ASCII letter and `:`.
#[logic(open)]
pub fn is_drive_prefixed(text: Seq<char>) -> bool {
    pearlite! { text.len() >= 2 && is_ascii_letter(text[0]) && text[1] == ':' }
}

/// A segment that names an entry inside its directory: non-empty, neither
/// `.` nor `..`, and holding no separator.
#[logic(open)]
pub fn is_name(text: Seq<char>) -> bool {
    pearlite! { text.len() > 0 && !is_current(text) && !is_parent(text) && separator_free(text) }
}

/// A segment `archive_entry_segments` keeps: a name with no drive, or `.`.
#[logic(open)]
pub fn is_entry_segment(text: Seq<char>) -> bool {
    pearlite! { (is_name(text) && !is_drive_prefixed(text)) || is_current(text) }
}

/// Every segment is a name.
#[logic(open)]
pub fn all_names(segments: Seq<&str>) -> bool {
    pearlite! {
        forall<index> 0 <= index && index < segments.len() ==> is_name(segments[index]@)
    }
}

/// Every segment is a name with no drive.
#[logic(open)]
pub fn all_driveless_names(segments: Seq<&str>) -> bool {
    pearlite! {
        forall<index> 0 <= index && index < segments.len() ==>
            is_name(segments[index]@) && !is_drive_prefixed(segments[index]@)
    }
}

/// `segments` joined by `/`.
#[logic(open)]
#[variant(segments.len())]
pub fn joined(segments: Seq<&str>) -> Seq<char> {
    pearlite! {
        if segments.len() == 0 {
            Seq::empty()
        } else if segments.len() == 1 {
            segments[0]@
        } else {
            joined(segments.subsequence(0, segments.len() - 1))
                .concat(Seq::singleton('/'))
                .concat(segments[segments.len() - 1]@)
        }
    }
}

/// The first `count` of the first `count + 1` segments are the first `count`.
#[logic]
#[requires(0 <= count && count < segments.len())]
#[ensures(segments.subsequence(0, count + 1).subsequence(0, count) == segments.subsequence(0, count))]
pub fn prefix_of_prefix(segments: Seq<&str>, count: Int) {}

/// Joining one more segment adds `/` and that segment to the join of the
/// ones before it.
#[logic]
#[requires(0 < count && count < segments.len())]
#[ensures(joined(segments.subsequence(0, count + 1))
    == joined(segments.subsequence(0, count)).concat(Seq::singleton('/')).concat(segments[count]@))]
pub fn joined_step(segments: Seq<&str>, count: Int) {
    pearlite! { prefix_of_prefix(segments, count) }
}
