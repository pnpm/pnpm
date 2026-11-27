//! The logic the group contracts are stated in. Each counts in bytes of the
//! text.

use super::{
    CLOSE,
    Group,
    OPEN,
};
use creusot_std::prelude::{
    Int,
    Seq,
    logic,
    variant,
};

/// Whether `byte` is `(` or `)`.
#[logic(open(super))]
pub fn is_paren(byte: u8) -> bool {
    pearlite! { byte == OPEN || byte == CLOSE }
}

/// How many more `)` than `(` bytes `from..to` of `bytes` hold.
#[logic(open(super))]
#[variant(to - from)]
pub fn excess(bytes: Seq<u8>, from: Int, to: Int) -> Int {
    pearlite! {
        if from >= to {
            0
        } else if bytes[from] == CLOSE {
            excess(bytes, from + 1, to) + 1
        } else if bytes[from] == OPEN {
            excess(bytes, from + 1, to) - 1
        } else {
            excess(bytes, from + 1, to)
        }
    }
}

/// Whether bytes `start..end` of `bytes` are a group: the `(` at `start` is
/// the one the `)` at `end - 1` closes.
#[logic(open(super))]
pub fn is_group(bytes: Seq<u8>, start: Int, end: Int) -> bool {
    pearlite! {
        0 <= start && start + 2 <= end && end <= bytes.len()
            && bytes[start] == OPEN && bytes[end - 1] == CLOSE
            && excess(bytes, start, end) == 0
            && forall<later> start < later && later < end ==> excess(bytes, later, end) > 0
    }
}

/// Whether `groups`, groups of `text`, follow one another from byte `from`
/// to the end of `text`.
#[logic(open(super), inline)]
pub fn tiles(text: Seq<char>, groups: Seq<Group<'_>>, from: Int) -> bool {
    pearlite! {
        (forall<index> 0 <= index && index < groups.len() ==> groups[index].text@ == text)
            && (groups.len() == 0 ==> from == text.to_bytes().len())
            && (groups.len() > 0 ==> groups[0].start@ == from
                && groups[groups.len() - 1].end@ == text.to_bytes().len())
            && forall<index> 0 <= index && index + 1 < groups.len() ==>
                groups[index].end == groups[index + 1].start
    }
}
