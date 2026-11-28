//! The lemmas the interval proofs call.

use super::logic::{
    above,
    below,
    covered,
    holds,
    in_pairs,
    in_prefix,
    in_union,
    lower_cmp,
    pairs_end_before,
    pairs_separated,
    separated,
    sorted_by_lower,
    upper_cmp,
    valid_log,
};
use crate::{
    Bound,
    Interval,
    Ordered,
};
use creusot_std::prelude::{
    Int,
    OrdLogic,
    Seq,
    ensures,
    logic,
    requires,
};
use std::cmp::Ordering;

// The bound orders.

/// A lower bound ordered no higher than another is above every value the
/// other is above.
#[logic]
#[ensures(lower_cmp(left, right) != Ordering::Greater
    ==> forall<value: Value::DeepModelTy> above(right, value) ==> above(left, value))]
pub fn lower_monotone<Value: Ordered>(left: Bound<Value>, right: Bound<Value>) {}

/// An upper bound ordered no higher than another is below no value the
/// other is not below.
#[logic]
#[ensures(upper_cmp(left, right) != Ordering::Greater
    ==> forall<value: Value::DeepModelTy> below(left, value) ==> below(right, value))]
pub fn upper_monotone<Value: Ordered>(left: Bound<Value>, right: Bound<Value>) {}

/// Swapping the bounds a comparison is given swaps its answer.
#[logic]
#[ensures(lower_cmp(left, right) != Ordering::Less
    ==> lower_cmp(right, left) != Ordering::Greater)]
#[ensures(upper_cmp(left, right) != Ordering::Less
    ==> upper_cmp(right, left) != Ordering::Greater)]
pub fn flip<Value: Ordered>(left: Bound<Value>, right: Bound<Value>) {}

/// The lower-bound order is transitive.
#[logic]
#[ensures(lower_cmp(first, second) != Ordering::Greater
    && lower_cmp(second, third) != Ordering::Greater
    ==> lower_cmp(first, third) != Ordering::Greater)]
pub fn lower_transitive<Value: Ordered>(
    first: Bound<Value>,
    second: Bound<Value>,
    third: Bound<Value>,
) {
}

// Unions.

/// Inserting `inserted` at `index` adds what it holds to the union.
#[logic]
#[requires(0 <= index && index <= before.len() && after.len() == before.len() + 1)]
#[requires(forall<other: Int> 0 <= other && other < index ==> after[other] == before[other])]
#[requires(after[index] == inserted)]
#[requires(forall<other: Int> index < other && other < after.len()
    ==> after[other] == before[other - 1])]
#[ensures(forall<value: Value::DeepModelTy>
    in_union(after, value) == (in_union(before, value) || holds(inserted, value)))]
pub fn insert_union<Value: Ordered>(
    before: Seq<Interval<Value>>,
    index: Int,
    inserted: Interval<Value>,
    after: Seq<Interval<Value>>,
) {
}

/// [`insert_union`]'s insertion keeps [`sorted_by_lower`] when every lower
/// bound before `index` orders below `inserted`'s and the one at `index`
/// does not.
#[logic]
#[requires(0 <= index && index <= before.len() && after.len() == before.len() + 1)]
#[requires(forall<other: Int> 0 <= other && other < index ==> after[other] == before[other])]
#[requires(after[index] == inserted)]
#[requires(forall<other: Int> index < other && other < after.len()
    ==> after[other] == before[other - 1])]
#[requires(sorted_by_lower(before))]
#[requires(forall<other: Int> 0 <= other && other < index
    ==> lower_cmp(before[other].lower, inserted.lower) == Ordering::Less)]
#[requires(index < before.len()
    ==> lower_cmp(before[index].lower, inserted.lower) != Ordering::Less)]
#[ensures(sorted_by_lower(after))]
pub fn insert_sorted<Value: Ordered>(
    before: Seq<Interval<Value>>,
    index: Int,
    inserted: Interval<Value>,
    after: Seq<Interval<Value>>,
) {
    pearlite! {
        if index < before.len() {
            flip(before[index].lower, inserted.lower);
            lemma_insert_sorted_tail(before, index, inserted)
        }
    }
}

/// Everything after `index` in a sorted `before` orders no lower than
/// `inserted` when `before[index]` does not.
#[logic]
#[requires(0 <= index && index < before.len() && sorted_by_lower(before))]
#[requires(lower_cmp(inserted.lower, before[index].lower) != Ordering::Greater)]
#[ensures(forall<other: Int> index <= other && other < before.len()
    ==> lower_cmp(inserted.lower, before[other].lower) != Ordering::Greater)]
pub fn lemma_insert_sorted_tail<Value: Ordered>(
    before: Seq<Interval<Value>>,
    index: Int,
    inserted: Interval<Value>,
) {
    pearlite! { lower_transitive_all(inserted.lower, before, index) }
}

/// [`lower_transitive`] through every bound of `intervals` from `index` on.
#[logic]
#[requires(0 <= index && index < intervals.len() && sorted_by_lower(intervals))]
#[ensures(lower_cmp(first, intervals[index].lower) != Ordering::Greater
    ==> forall<other: Int> index <= other && other < intervals.len()
        ==> lower_cmp(first, intervals[other].lower) != Ordering::Greater)]
pub fn lower_transitive_all<Value: Ordered>(
    first: Bound<Value>,
    intervals: Seq<Interval<Value>>,
    index: Int,
) {
}

/// Appending `last` adds what it holds to the union.
#[logic]
#[ensures(forall<value: Value::DeepModelTy>
    in_union(intervals.snoc(last), value) == (in_union(intervals, value) || holds(last, value)))]
pub fn snoc_union<Value: Ordered>(intervals: Seq<Interval<Value>>, last: Interval<Value>) {}

/// A non-empty union is its first interval and the rest.
#[logic]
#[requires(intervals.len() > 0)]
#[ensures(forall<value: Value::DeepModelTy> in_union(intervals, value)
    == (holds(intervals[0], value)
        || in_union(intervals.subsequence(1, intervals.len()), value)))]
pub fn front_union<Value: Ordered>(intervals: Seq<Interval<Value>>) {}

/// Appending a pair adds what its bounds hold to [`in_pairs`].
#[logic]
#[ensures(forall<value: Value::DeepModelTy> in_pairs(pairs.snoc(last), value)
    == (in_pairs(pairs, value) || (above(*last.0, value) && below(*last.1, value))))]
pub fn snoc_pairs<'bound, Value: Ordered>(
    pairs: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    last: (&'bound Bound<Value>, &'bound Bound<Value>),
) {
}

// The sweep.

/// One more interval in the prefix holds what it holds.
#[logic]
#[requires(0 <= end && end < intervals.len())]
#[ensures(forall<value: Value::DeepModelTy> in_prefix(intervals, end + 1, value)
    == (in_prefix(intervals, end, value) || holds(intervals[end], value)))]
#[ensures(forall<value: Value::DeepModelTy> holds(intervals[end], value) ==> in_union(intervals, value))]
pub fn prefix_grows<Value: Ordered>(intervals: Seq<Interval<Value>>, end: Int) {}

/// The whole of a union is its prefix up to its length.
#[logic]
#[ensures(forall<value: Value::DeepModelTy>
    in_prefix(intervals, intervals.len(), value) == in_union(intervals, value))]
pub fn whole_prefix<Value: Ordered>(intervals: Seq<Interval<Value>>) {}

/// A value in `left[left_index]` and in `right`, where `left[left_index]`'s
/// upper bound orders no higher than `right[right_index]`'s, is in
/// `right[..=right_index]`.
#[logic]
#[requires(separated(right))]
#[requires(0 <= left_index && left_index < left.len())]
#[requires(0 <= right_index && right_index < right.len())]
#[requires(upper_cmp(left[left_index].upper, right[right_index].upper) != Ordering::Greater)]
#[ensures(forall<value: Value::DeepModelTy> holds(left[left_index], value) && in_union(right, value)
    ==> holds(right[right_index], value) || in_prefix(right, right_index, value))]
pub fn left_meets_right_prefix<Value: Ordered>(
    left: Seq<Interval<Value>>,
    left_index: Int,
    right: Seq<Interval<Value>>,
    right_index: Int,
) {
    pearlite! { upper_monotone(left[left_index].upper, right[right_index].upper) }
}

/// Advancing the left index keeps [`covered`].
#[logic]
#[requires(separated(right))]
#[requires(0 <= left_index && left_index < left.len())]
#[requires(0 <= right_index && right_index < right.len())]
#[requires(upper_cmp(left[left_index].upper, right[right_index].upper) == Ordering::Less)]
#[requires(covered(before, left, left_index, right, right_index))]
#[requires(forall<value: Value::DeepModelTy> in_pairs(after, value) == (in_pairs(before, value)
    || (holds(left[left_index], value) && holds(right[right_index], value))))]
#[ensures(covered(after, left, left_index + 1, right, right_index))]
pub fn step_left<'bound, Value: Ordered>(
    before: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    after: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    left: Seq<Interval<Value>>,
    left_index: Int,
    right: Seq<Interval<Value>>,
    right_index: Int,
) {
    pearlite! {
        prefix_grows(left, left_index);
        left_meets_right_prefix(left, left_index, right, right_index)
    }
}

/// Advancing the right index keeps [`covered`].
#[logic]
#[requires(separated(left))]
#[requires(0 <= left_index && left_index < left.len())]
#[requires(0 <= right_index && right_index < right.len())]
#[requires(upper_cmp(left[left_index].upper, right[right_index].upper) != Ordering::Less)]
#[requires(covered(before, left, left_index, right, right_index))]
#[requires(forall<value: Value::DeepModelTy> in_pairs(after, value) == (in_pairs(before, value)
    || (holds(left[left_index], value) && holds(right[right_index], value))))]
#[ensures(covered(after, left, left_index, right, right_index + 1))]
pub fn step_right<'bound, Value: Ordered>(
    before: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    after: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    left: Seq<Interval<Value>>,
    left_index: Int,
    right: Seq<Interval<Value>>,
    right_index: Int,
) {
    pearlite! {
        prefix_grows(right, right_index);
        flip(left[left_index].upper, right[right_index].upper);
        left_meets_right_prefix(right, right_index, left, left_index)
    }
}

/// The pair the sweep adds for `left[left_index]` and `right[right_index]`,
/// if it adds one, holds what both hold.
#[logic]
#[requires(0 <= left_index && left_index < left.len())]
#[requires(0 <= right_index && right_index < right.len())]
#[requires(forall<value: Value::DeepModelTy> above(*lower, value)
    == (above(left[left_index].lower, value) && above(right[right_index].lower, value)))]
#[requires(forall<value: Value::DeepModelTy> below(*upper, value)
    == (below(left[left_index].upper, value) && below(right[right_index].upper, value)))]
#[requires(after == before.snoc((lower, upper))
    || (after == before && forall<value: Value::DeepModelTy>
        !(above(*lower, value) && below(*upper, value))))]
#[ensures(forall<value: Value::DeepModelTy> in_pairs(after, value) == (in_pairs(before, value)
    || (holds(left[left_index], value) && holds(right[right_index], value))))]
pub fn pair_added<'bound, Value: Ordered>(
    before: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    after: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    lower: &'bound Bound<Value>,
    upper: &'bound Bound<Value>,
    left: Seq<Interval<Value>>,
    left_index: Int,
    right: Seq<Interval<Value>>,
    right_index: Int,
) {
    pearlite! { snoc_pairs(before, (lower, upper)) }
}

/// The pair the sweep adds keeps [`pairs_separated`], and the index it
/// advances keeps [`pairs_end_before`].
#[logic]
#[requires(separated(left) && separated(right))]
#[requires(0 <= left_index && left_index < left.len())]
#[requires(0 <= right_index && right_index < right.len())]
#[requires(forall<value: Value::DeepModelTy> above(*lower, value)
    == (above(left[left_index].lower, value) && above(right[right_index].lower, value)))]
#[requires(forall<value: Value::DeepModelTy> below(*upper, value)
    == (below(left[left_index].upper, value) && below(right[right_index].upper, value)))]
#[requires(after == before.snoc((lower, upper)) || after == before)]
#[requires(pairs_separated(before))]
#[requires(pairs_end_before(before, left, left_index, right, right_index))]
#[ensures(pairs_separated(after))]
#[ensures(upper_cmp(left[left_index].upper, right[right_index].upper) == Ordering::Less
    ==> pairs_end_before(after, left, left_index + 1, right, right_index))]
#[ensures(upper_cmp(left[left_index].upper, right[right_index].upper) != Ordering::Less
    ==> pairs_end_before(after, left, left_index, right, right_index + 1))]
pub fn pairs_grow<'bound, Value: Ordered>(
    before: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    after: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    lower: &'bound Bound<Value>,
    upper: &'bound Bound<Value>,
    left: Seq<Interval<Value>>,
    left_index: Int,
    right: Seq<Interval<Value>>,
    right_index: Int,
) {
}

// The merge.

/// A value `interval` holds is in `last` once `last`'s upper bound orders
/// no lower than `interval`'s and its lower bound no higher.
#[logic]
#[requires(lower_cmp(last.lower, interval.lower) != Ordering::Greater)]
#[requires(upper_cmp(interval.upper, last.upper) != Ordering::Greater)]
#[ensures(forall<value: Value::DeepModelTy> holds(interval, value) ==> holds(last, value))]
pub fn absorbed<Value: Ordered>(last: Interval<Value>, interval: Interval<Value>) {
    pearlite! {
        lower_monotone(last.lower, interval.lower);
        upper_monotone(interval.upper, last.upper)
    }
}

/// Widening `last` to `interval`'s upper bound holds what either held, when
/// `interval` starts at or after `last` and no value past `last`'s upper
/// bound is below `interval`'s lower bound.
#[logic]
#[requires(lower_cmp(last.lower, interval.lower) != Ordering::Greater)]
#[requires(upper_cmp(interval.upper, last.upper) == Ordering::Greater)]
#[requires(forall<value: Value::DeepModelTy> !below(last.upper, value) ==> above(interval.lower, value))]
#[requires(widened.lower == last.lower && widened.upper == interval.upper)]
#[ensures(forall<value: Value::DeepModelTy> holds(widened, value)
    == (holds(last, value) || holds(interval, value)))]
pub fn widened_holds<Value: Ordered>(
    last: Interval<Value>,
    interval: Interval<Value>,
    widened: Interval<Value>,
) {
    pearlite! {
        lower_monotone(last.lower, interval.lower);
        flip(interval.upper, last.upper);
        upper_monotone(last.upper, interval.upper)
    }
}

/// Replacing the last interval by one that holds what it and `interval` held
/// adds `interval` to the union.
#[logic]
#[requires(before.len() > 0 && after.len() == before.len())]
#[requires(forall<index: Int> 0 <= index && index < before.len() - 1 ==> after[index] == before[index])]
#[requires(forall<value: Value::DeepModelTy> holds(after[after.len() - 1], value)
    == (holds(before[before.len() - 1], value) || holds(interval, value)))]
#[ensures(forall<value: Value::DeepModelTy> in_union(after, value)
    == (in_union(before, value) || holds(interval, value)))]
pub fn merge_into_last<Value: Ordered>(
    before: Seq<Interval<Value>>,
    after: Seq<Interval<Value>>,
    interval: Interval<Value>,
) {
}

/// Removing the first interval leaves the rest.
#[logic]
#[requires(intervals.len() > 0)]
#[ensures(intervals.subsequence(0, 0).ext_eq(Seq::empty()))]
#[ensures(intervals.subsequence(0, 0).concat(intervals.subsequence(1, intervals.len()))
    == intervals.subsequence(1, intervals.len()))]
pub fn drop_first<Value: Ordered>(intervals: Seq<Interval<Value>>) {
    pearlite! { intervals.subsequence(1, intervals.len()).concat_empty() }
}

/// Appending `interval` keeps [`separated`] when nothing below the last
/// interval's upper bound is above `interval`'s lower bound, which orders no
/// lower than the last one's.
#[logic]
#[requires(separated(merged))]
#[requires(merged.len() > 0 ==> forall<value: Value::DeepModelTy>
    !(above(interval.lower, value) && below(merged[merged.len() - 1].upper, value)))]
#[requires(merged.len() > 0
    ==> lower_cmp(merged[merged.len() - 1].lower, interval.lower) != Ordering::Greater)]
#[ensures(separated(merged.snoc(interval)))]
pub fn push_separated<Value: Ordered>(merged: Seq<Interval<Value>>, interval: Interval<Value>) {
    pearlite! {
        if merged.len() > 0 {
            lower_monotone(merged[merged.len() - 1].lower, interval.lower)
        }
    }
}

/// Changing the last interval's upper bound keeps [`separated`].
#[logic]
#[requires(separated(before) && before.len() > 0 && after.len() == before.len())]
#[requires(forall<index: Int> 0 <= index && index < before.len() - 1 ==> after[index] == before[index])]
#[requires(after[after.len() - 1].lower == before[before.len() - 1].lower)]
#[ensures(separated(after))]
pub fn widen_separated<Value: Ordered>(before: Seq<Interval<Value>>, after: Seq<Interval<Value>>) {}

/// A value that is not at or below `high` is above it. Naming `cmp_log`
/// brings in the laws of [`OrdLogic`], totality among them.
#[logic]
#[ensures(high.cmp_log(high) == Ordering::Equal)]
#[ensures(forall<value: Order> !(value <= high) ==> high < value)]
#[ensures(forall<value: Order> !(value < high) ==> high <= value)]
pub fn past<Order: OrdLogic>(high: Order) {}

/// Above a value at or above `low` is above `low`.
#[logic]
#[ensures(forall<value: Order> low <= high && high < value ==> low < value)]
#[ensures(forall<value: Order> low < high && high <= value ==> low < value)]
pub fn above_through<Order: OrdLogic>(low: Order, high: Order) {}

/// Past a value at or above `low`, a value is above `low`.
#[logic]
#[ensures(forall<value: Order> low <= high && !(value <= high) ==> low < value)]
#[ensures(forall<value: Order> low < high && !(value < high) ==> low < value)]
pub fn order_after<Order: OrdLogic>(low: Order, high: Order) {
    pearlite! {
        past(high);
        above_through(low, high)
    }
}

/// When the bounds are valid, every value past `upper` is above `lower`.
#[logic]
#[ensures(valid_log(lower, upper)
    ==> forall<value: Value::DeepModelTy> !below(upper, value) ==> above(lower, value))]
pub fn valid_past_upper<Value: Ordered>(lower: Bound<Value>, upper: Bound<Value>) {
    pearlite! {
        match (lower, upper) {
            (Bound::Inclusive(low), Bound::Inclusive(high))
            | (Bound::Inclusive(low), Bound::Exclusive(high))
            | (Bound::Exclusive(low), Bound::Inclusive(high))
            | (Bound::Exclusive(low), Bound::Exclusive(high)) =>
                order_after(low.deep_model(), high.deep_model()),
            _ => (),
        }
    }
}
