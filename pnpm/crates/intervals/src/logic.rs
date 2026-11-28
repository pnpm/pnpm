//! Which values an interval holds, stated over the values' deep models.

use crate::{
    Bound,
    Interval,
    Ordered,
};
use creusot_std::prelude::{
    Int,
    OrdLogic,
    Seq,
    logic,
};
use std::cmp::Ordering;

/// Whether `value` is above `lower`.
#[logic(open)]
pub fn above<Value: Ordered>(lower: Bound<Value>, value: Value::DeepModelTy) -> bool {
    pearlite! {
        match lower {
            Bound::Inclusive(bound) => bound.deep_model() <= value,
            Bound::Exclusive(bound) => bound.deep_model() < value,
            Bound::Unbounded => true,
        }
    }
}

/// Whether `value` is below `upper`.
#[logic(open)]
pub fn below<Value: Ordered>(upper: Bound<Value>, value: Value::DeepModelTy) -> bool {
    pearlite! {
        match upper {
            Bound::Inclusive(bound) => value <= bound.deep_model(),
            Bound::Exclusive(bound) => value < bound.deep_model(),
            Bound::Unbounded => true,
        }
    }
}

/// [`crate::compare_lower`], in the logic.
#[logic(open)]
pub fn lower_cmp<Value: Ordered>(left: Bound<Value>, right: Bound<Value>) -> Ordering {
    pearlite! {
        match (left, right) {
            (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
            (Bound::Unbounded, _) => Ordering::Less,
            (_, Bound::Unbounded) => Ordering::Greater,
            (Bound::Inclusive(left_value), Bound::Inclusive(right_value))
            | (Bound::Exclusive(left_value), Bound::Exclusive(right_value)) =>
                left_value.deep_model().cmp_log(right_value.deep_model()),
            (Bound::Inclusive(left_value), Bound::Exclusive(right_value)) =>
                if left_value.deep_model() == right_value.deep_model() { Ordering::Less }
                else { left_value.deep_model().cmp_log(right_value.deep_model()) },
            (Bound::Exclusive(left_value), Bound::Inclusive(right_value)) =>
                if left_value.deep_model() == right_value.deep_model() { Ordering::Greater }
                else { left_value.deep_model().cmp_log(right_value.deep_model()) },
        }
    }
}

/// [`crate::compare_upper`], in the logic.
#[logic(open)]
pub fn upper_cmp<Value: Ordered>(left: Bound<Value>, right: Bound<Value>) -> Ordering {
    pearlite! {
        match (left, right) {
            (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
            (Bound::Unbounded, _) => Ordering::Greater,
            (_, Bound::Unbounded) => Ordering::Less,
            (Bound::Inclusive(left_value), Bound::Inclusive(right_value))
            | (Bound::Exclusive(left_value), Bound::Exclusive(right_value)) =>
                left_value.deep_model().cmp_log(right_value.deep_model()),
            (Bound::Exclusive(left_value), Bound::Inclusive(right_value)) =>
                if left_value.deep_model() == right_value.deep_model() { Ordering::Less }
                else { left_value.deep_model().cmp_log(right_value.deep_model()) },
            (Bound::Inclusive(left_value), Bound::Exclusive(right_value)) =>
                if left_value.deep_model() == right_value.deep_model() { Ordering::Greater }
                else { left_value.deep_model().cmp_log(right_value.deep_model()) },
        }
    }
}

/// Whether `interval` holds `value`.
#[logic(open)]
pub fn holds<Value: Ordered>(interval: Interval<Value>, value: Value::DeepModelTy) -> bool {
    pearlite! { above(interval.lower, value) && below(interval.upper, value) }
}

/// Whether one of `intervals` holds `value`.
#[logic(open)]
pub fn in_union<Value: Ordered>(
    intervals: Seq<Interval<Value>>,
    value: Value::DeepModelTy,
) -> bool {
    pearlite! {
        exists<index: Int> 0 <= index && index < intervals.len() && holds(intervals[index], value)
    }
}

/// Whether one of `intervals[..end]` holds `value`.
#[logic(open)]
pub fn in_prefix<Value: Ordered>(
    intervals: Seq<Interval<Value>>,
    end: Int,
    value: Value::DeepModelTy,
) -> bool {
    pearlite! {
        exists<index: Int> 0 <= index && index < end && index < intervals.len()
            && holds(intervals[index], value)
    }
}

/// Whether the bounds of one of `pairs` hold `value` between them.
#[logic(open)]
pub fn in_pairs<'bound, Value: Ordered>(
    pairs: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    value: Value::DeepModelTy,
) -> bool {
    pearlite! {
        exists<index: Int> 0 <= index && index < pairs.len()
            && above(*pairs[index].0, value) && below(*pairs[index].1, value)
    }
}

/// Whether no value below the upper bound of one of `intervals` is above the
/// lower bound of a later one.
#[logic(open)]
pub fn separated<Value: Ordered>(intervals: Seq<Interval<Value>>) -> bool {
    pearlite! {
        forall<first: Int, second: Int, value: Value::DeepModelTy>
            0 <= first && first < second && second < intervals.len()
                && below(intervals[first].upper, value)
                ==> !above(intervals[second].lower, value)
    }
}

/// [`separated`] for the bounds in `pairs`.
#[logic(open)]
pub fn pairs_separated<'bound, Value: Ordered>(
    pairs: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
) -> bool {
    pearlite! {
        forall<first: Int, second: Int, value: Value::DeepModelTy>
            0 <= first && first < second && second < pairs.len() && below(*pairs[first].1, value)
                ==> !above(*pairs[second].0, value)
    }
}

/// Whether `intervals` are in the order [`crate::compare_lower`] gives their
/// lower bounds.
#[logic(open)]
pub fn sorted_by_lower<Value: Ordered>(intervals: Seq<Interval<Value>>) -> bool {
    pearlite! {
        forall<first: Int, second: Int> 0 <= first && first < second && second < intervals.len()
            ==> lower_cmp(intervals[first].lower, intervals[second].lower) != Ordering::Greater
    }
}

/// Whether every value below the upper bound of one of `pairs` is above no
/// lower bound in `left[left_from..]`, or none in `right[right_from..]`.
#[logic(open)]
pub fn pairs_end_before<'bound, Value: Ordered>(
    pairs: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    left: Seq<Interval<Value>>,
    left_from: Int,
    right: Seq<Interval<Value>>,
    right_from: Int,
) -> bool {
    pearlite! {
        forall<index: Int, value: Value::DeepModelTy>
            0 <= index && index < pairs.len() && below(*pairs[index].1, value) ==>
                (forall<other: Int> left_from <= other && other < left.len()
                    ==> !above(left[other].lower, value))
                || (forall<other: Int> right_from <= other && other < right.len()
                    ==> !above(right[other].lower, value))
    }
}

// The sweep.

/// Whether every value in both unions that one of `left[..left_end]` or
/// `right[..right_end]` holds is between the bounds of one of `pairs`.
#[logic(open)]
pub fn covered<'bound, Value: Ordered>(
    pairs: Seq<(&'bound Bound<Value>, &'bound Bound<Value>)>,
    left: Seq<Interval<Value>>,
    left_end: Int,
    right: Seq<Interval<Value>>,
    right_end: Int,
) -> bool {
    pearlite! {
        forall<value: Value::DeepModelTy> in_union(left, value) && in_union(right, value)
            && (in_prefix(left, left_end, value) || in_prefix(right, right_end, value))
            ==> in_pairs(pairs, value)
    }
}

/// [`crate::is_valid_interval`], in the logic.
#[logic(open)]
pub fn valid_log<Value: Ordered>(lower: Bound<Value>, upper: Bound<Value>) -> bool {
    pearlite! {
        match (lower, upper) {
            (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
            (Bound::Inclusive(low), Bound::Inclusive(high)) => low.deep_model() <= high.deep_model(),
            (Bound::Inclusive(low), Bound::Exclusive(high))
            | (Bound::Exclusive(low), Bound::Inclusive(high))
            | (Bound::Exclusive(low), Bound::Exclusive(high)) =>
                low.deep_model() < high.deep_model(),
        }
    }
}
