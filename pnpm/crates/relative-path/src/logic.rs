//! The logic the contracts in the crate root are stated in.

use creusot_std::prelude::{
    Seq,
    logic,
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
