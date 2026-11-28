//! The lemmas the proofs in the crate root call.

use super::logic::is_placeholder;
use creusot_std::prelude::{
    Int,
    Seq,
    ensures,
    logic,
    requires,
};

/// No other placeholder starts inside a placeholder, and one that starts
/// where it does ends where it does: its body holds neither the `$` that
/// would start one nor the `}` that would end one.
#[logic]
#[requires(is_placeholder(bytes, start, end))]
#[ensures(forall<other_start, other_end> is_placeholder(bytes, other_start, other_end)
    && start <= other_start && other_start < end ==> other_start == start && other_end == end)]
pub fn no_placeholder_inside(bytes: Seq<u8>, start: Int, end: Int) {}
