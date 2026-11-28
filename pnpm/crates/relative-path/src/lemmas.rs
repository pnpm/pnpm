//! The lemmas the proofs in the crate root call.

use super::logic::joined;
use creusot_std::prelude::{
    Int,
    Seq,
    ensures,
    logic,
    requires,
};

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
