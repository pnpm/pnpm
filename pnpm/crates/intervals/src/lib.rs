//! Unions of intervals of a totally ordered value, such as the versions a
//! semver range admits.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that no function here panics,
//! and that for every total order:
//!
//! - [`max_lower`] and [`min_upper`] give the bound that admits exactly what
//!   both bounds admit, so an interval of the two holds exactly what both
//!   intervals hold.
//! - [`is_valid_interval`] is false only for bounds with no value between
//!   them. It can be true for such bounds too: over semver versions,
//!   `>1.0.0 <1.0.1-0` holds none.
//! - [`merge_overlapping_intervals`] returns a union of the same values, in
//!   which no value below one interval's upper bound is above a later
//!   interval's lower bound.
//! - [`intersect_intervals`] returns, for two unions in that shape, the
//!   bounds of a union of exactly the values in both, in that shape too.

#[cfg(creusot)]
pub mod spec;

#[cfg(creusot)]
use creusot_std::prelude::{
    DeepModel,
    Int,
    OrdLogic,
    ensures,
    invariant,
    proof_assert,
    requires,
    snapshot,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
use std::cmp::Ordering;

#[cfg(creusot)]
use spec::{
    above,
    below,
    in_pairs,
    in_union,
    lower_cmp,
    pairs_separated,
    separated,
    sorted_by_lower,
    upper_cmp,
};

/// A value with a total order.
#[cfg(not(creusot))]
pub trait Ordered: Ord {}
#[cfg(not(creusot))]
impl<Value: Ord> Ordered for Value {}

/// A value with a total order, and the order of its deep model.
#[cfg(creusot)]
pub trait Ordered: Ord + DeepModel<DeepModelTy: OrdLogic> {}
#[cfg(creusot)]
impl<Value: Ord + DeepModel<DeepModelTy: OrdLogic>> Ordered for Value {}

/// One end of an interval.
#[cfg_attr(not(creusot), derive(Debug, Clone, PartialEq, Eq))]
pub enum Bound<Value> {
    /// The end, and the value at it.
    Inclusive(Value),
    /// The end, but not the value at it.
    Exclusive(Value),
    /// No end.
    Unbounded,
}

/// The values between a lower and an upper bound.
#[cfg_attr(not(creusot), derive(Debug, Clone, PartialEq, Eq))]
pub struct Interval<Value> {
    /// The bound every value of the interval is above.
    pub lower: Bound<Value>,
    /// The bound every value of the interval is below.
    pub upper: Bound<Value>,
}

impl<Value: Ordered> Interval<Value> {
    /// Whether `value` lies between the bounds.
    #[cfg_attr(creusot, ensures(result == (above(self.lower, value.deep_model())
        && below(self.upper, value.deep_model()))))]
    #[must_use]
    pub fn contains(&self, value: &Value) -> bool {
        let above_lower = match &self.lower {
            Bound::Inclusive(lower) => lower <= value,
            Bound::Exclusive(lower) => lower < value,
            Bound::Unbounded => true,
        };
        let below_upper = match &self.upper {
            Bound::Inclusive(upper) => value <= upper,
            Bound::Exclusive(upper) => value < upper,
            Bound::Unbounded => true,
        };
        above_lower && below_upper
    }
}

/// The tighter of two lower bounds: a value is above it exactly when it is
/// above both.
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy>
    above(*result, value) == (above(*left, value) && above(*right, value))))]
#[cfg_attr(creusot, ensures(result == left || result == right))]
#[must_use]
pub fn max_lower<'bound, Value: Ordered>(
    left: &'bound Bound<Value>,
    right: &'bound Bound<Value>,
) -> &'bound Bound<Value> {
    match compare_lower(left, right) {
        Ordering::Less => right,
        Ordering::Equal | Ordering::Greater => left,
    }
}

/// The tighter of two upper bounds: a value is below it exactly when it is
/// below both.
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy>
    below(*result, value) == (below(*left, value) && below(*right, value))))]
#[cfg_attr(creusot, ensures(result == left || result == right))]
#[must_use]
pub fn min_upper<'bound, Value: Ordered>(
    left: &'bound Bound<Value>,
    right: &'bound Bound<Value>,
) -> &'bound Bound<Value> {
    match compare_upper(left, right) {
        Ordering::Greater => right,
        Ordering::Less | Ordering::Equal => left,
    }
}

/// Whether the bounds could hold a value between them. When they cannot,
/// none lies between them. When they can, every value past the upper bound
/// is above the lower one.
#[cfg_attr(creusot, ensures(result == spec::valid_log(*lower, *upper)))]
#[cfg_attr(creusot, ensures(!result ==> forall<value: Value::DeepModelTy>
    !(above(*lower, value) && below(*upper, value))))]
#[cfg_attr(creusot, ensures(result ==> forall<value: Value::DeepModelTy>
    !below(*upper, value) ==> above(*lower, value)))]
#[must_use]
pub fn is_valid_interval<Value: Ordered>(lower: &Bound<Value>, upper: &Bound<Value>) -> bool {
    #[cfg(creusot)]
    proof_assert! {
        spec::valid_past_upper(*lower, *upper);
        spec::valid_log(*lower, *upper) ==> forall<value: Value::DeepModelTy>
            !below(*upper, value) ==> above(*lower, value)
    };
    match (lower, upper) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
        (Bound::Inclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version <= right_version
        }
        (
            Bound::Inclusive(left_version) | Bound::Exclusive(left_version),
            Bound::Inclusive(right_version) | Bound::Exclusive(right_version),
        ) => left_version < right_version,
    }
}

/// Orders lower bounds from the one admitting the most values. At one value
/// the exclusive bound is the higher: it leaves that value out.
#[cfg_attr(creusot, ensures(result == lower_cmp(*left, *right)))]
#[must_use]
pub fn compare_lower<Value: Ordered>(left: &Bound<Value>, right: &Bound<Value>) -> Ordering {
    match (left, right) {
        (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
        (Bound::Unbounded, _) => Ordering::Less,
        (_, Bound::Unbounded) => Ordering::Greater,
        (
            Bound::Inclusive(left_value) | Bound::Exclusive(left_value),
            Bound::Inclusive(right_value) | Bound::Exclusive(right_value),
        ) => match left_value.cmp(right_value) {
            Ordering::Equal => match (left, right) {
                (Bound::Inclusive(_), Bound::Exclusive(_)) => Ordering::Less,
                (Bound::Exclusive(_), Bound::Inclusive(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            },
            unequal => unequal,
        },
    }
}

/// Orders upper bounds up to the one admitting the most values. At one value
/// the exclusive bound is the lower.
#[cfg_attr(creusot, ensures(result == upper_cmp(*left, *right)))]
#[must_use]
pub fn compare_upper<Value: Ordered>(left: &Bound<Value>, right: &Bound<Value>) -> Ordering {
    match (left, right) {
        (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
        (Bound::Unbounded, _) => Ordering::Greater,
        (_, Bound::Unbounded) => Ordering::Less,
        (
            Bound::Inclusive(left_value) | Bound::Exclusive(left_value),
            Bound::Inclusive(right_value) | Bound::Exclusive(right_value),
        ) => match left_value.cmp(right_value) {
            Ordering::Equal => match (left, right) {
                (Bound::Exclusive(_), Bound::Inclusive(_)) => Ordering::Less,
                (Bound::Inclusive(_), Bound::Exclusive(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            },
            unequal => unequal,
        },
    }
}

/// `intervals` sorted by lower bound. Intervals with equal lower bounds keep
/// their order.
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy>
    in_union(result@, value) == in_union(intervals@, value)))]
#[cfg_attr(creusot, ensures(sorted_by_lower(result@)))]
fn sort_by_lower<Value: Ordered>(mut intervals: Vec<Interval<Value>>) -> Vec<Interval<Value>> {
    #[cfg(creusot)]
    let original = snapshot! { intervals };
    let mut sorted: Vec<Interval<Value>> = Vec::with_capacity(intervals.len());
    #[cfg_attr(creusot, invariant(forall<value: Value::DeepModelTy>
        (in_union(sorted@, value) || in_union(intervals@, value)) == in_union(original@, value)))]
    #[cfg_attr(creusot, invariant(sorted_by_lower(sorted@)))]
    while let Some(interval) = intervals.pop() {
        let mut index = 0;
        #[cfg_attr(creusot, invariant(index@ <= sorted@.len()))]
        #[cfg_attr(creusot, invariant(forall<earlier: Int> 0 <= earlier && earlier < index@
            ==> lower_cmp(sorted@[earlier].lower, interval.lower) == Ordering::Less))]
        while index < sorted.len() {
            match compare_lower(&sorted[index].lower, &interval.lower) {
                Ordering::Less => index += 1,
                Ordering::Equal | Ordering::Greater => break,
            }
        }
        #[cfg(creusot)]
        let before = snapshot! { sorted };
        #[cfg(creusot)]
        let inserted = snapshot! { interval };
        sorted.insert(index, interval);
        #[cfg(creusot)]
        proof_assert! {
            spec::insert_union((*before)@, index@, *inserted, sorted@);
            spec::insert_sorted((*before)@, index@, *inserted, sorted@);
            sorted_by_lower(sorted@) && forall<value: Value::DeepModelTy>
                in_union(sorted@, value) == (in_union(before@, value) || spec::holds(*inserted, value))
        };
    }
    sorted
}

/// The values of `intervals` as a union sorted by lower bound in which no
/// value below one interval's upper bound is above a later interval's lower
/// bound.
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy>
    in_union(result@, value) == in_union(intervals@, value)))]
#[cfg_attr(creusot, ensures(separated(result@)))]
#[must_use]
pub fn merge_overlapping_intervals<Value: Ordered>(
    intervals: Vec<Interval<Value>>,
) -> Vec<Interval<Value>> {
    let mut remaining = sort_by_lower(intervals);
    #[cfg(creusot)]
    let original = snapshot! { remaining };
    let mut merged: Vec<Interval<Value>> = Vec::with_capacity(remaining.len());
    #[cfg_attr(creusot, invariant(forall<value: Value::DeepModelTy>
        (in_union(merged@, value) || in_union(remaining@, value)) == in_union(original@, value)))]
    #[cfg_attr(creusot, invariant(sorted_by_lower(remaining@)))]
    #[cfg_attr(creusot, invariant(separated(merged@)))]
    #[cfg_attr(creusot, invariant(forall<done: Int, next: Int> 0 <= done && done < merged@.len()
        && 0 <= next && next < remaining@.len()
        ==> lower_cmp(merged@[done].lower, remaining@[next].lower) != Ordering::Greater))]
    while !remaining.is_empty() {
        #[cfg(creusot)]
        let unmerged = snapshot! { remaining };
        let interval = remaining.remove(0);
        #[cfg(creusot)]
        proof_assert! {
            spec::front_union(unmerged@);
            spec::drop_first(unmerged@);
            forall<value: Value::DeepModelTy> in_union(unmerged@, value)
                == (spec::holds(interval, value) || in_union(remaining@, value))
        };
        merge_one(&mut merged, interval);
    }
    merged
}

/// Add `interval` to `merged`: widen the last interval to it when the two
/// overlap, or append it.
#[cfg_attr(creusot, requires(separated(merged@)))]
#[cfg_attr(creusot, requires(forall<done: Int> 0 <= done && done < merged@.len()
    ==> lower_cmp(merged@[done].lower, interval.lower) != Ordering::Greater))]
#[cfg_attr(creusot, ensures(separated((^merged)@)))]
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy> in_union((^merged)@, value)
    == (in_union(merged@, value) || spec::holds(interval, value))))]
#[cfg_attr(creusot, ensures(forall<done: Int> 0 <= done && done < (^merged)@.len()
    ==> (done < merged@.len() && (^merged)@[done].lower == merged@[done].lower)
        || (^merged)@[done].lower == interval.lower))]
fn merge_one<Value: Ordered>(merged: &mut Vec<Interval<Value>>, interval: Interval<Value>) {
    #[cfg(creusot)]
    let taken = snapshot! { interval };
    #[cfg(creusot)]
    let before = snapshot! { *merged };
    let count = merged.len();
    if count > 0 && is_valid_interval(&interval.lower, &merged[count - 1].upper) {
        if compare_upper(&interval.upper, &merged[count - 1].upper) == Ordering::Greater {
            merged[count - 1].upper = interval.upper;
            #[cfg(creusot)]
            proof_assert! {
                spec::widened_holds(before@[count@ - 1], *taken, merged@[count@ - 1]);
                spec::merge_into_last(before@, merged@, *taken);
                spec::widen_separated(before@, merged@);
                separated(merged@) && forall<value: Value::DeepModelTy> in_union(merged@, value)
                    == (in_union(before@, value) || spec::holds(*taken, value))
            };
        } else {
            #[cfg(creusot)]
            proof_assert! {
                spec::absorbed(merged@[count@ - 1], *taken);
                forall<value: Value::DeepModelTy>
                    spec::holds(*taken, value) ==> in_union(merged@, value)
            };
        }
    } else {
        merged.push(interval);
        #[cfg(creusot)]
        proof_assert! {
            spec::snoc_union(before@, *taken);
            spec::push_separated(before@, *taken);
            separated(merged@) && forall<value: Value::DeepModelTy> in_union(merged@, value)
                == (in_union(before@, value) || spec::holds(*taken, value))
        };
    }
}

/// The intersection of two unions in the shape [`merge_overlapping_intervals`]
/// leaves them, as the bounds of each part, in that shape too. A single sweep
/// pairs only the intervals that can overlap, so the work is linear in the
/// two unions.
#[cfg_attr(creusot, requires(separated(left@) && separated(right@)))]
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy> in_pairs(result@, value)
    == (in_union(left@, value) && in_union(right@, value))))]
#[cfg_attr(creusot, ensures(pairs_separated(result@)))]
#[must_use]
pub fn intersect_intervals<'interval, Value: Ordered>(
    left: &'interval [Interval<Value>],
    right: &'interval [Interval<Value>],
) -> Vec<(&'interval Bound<Value>, &'interval Bound<Value>)> {
    let mut result = Vec::new();
    let mut left_index: usize = 0;
    let mut right_index: usize = 0;
    #[cfg_attr(creusot, invariant(left_index@ <= left@.len() && right_index@ <= right@.len()))]
    #[cfg_attr(creusot, invariant(forall<value: Value::DeepModelTy> in_pairs(result@, value)
        ==> in_union(left@, value) && in_union(right@, value)))]
    #[cfg_attr(creusot, invariant(spec::covered(result@, left@, left_index@, right@, right_index@)))]
    #[cfg_attr(creusot, invariant(pairs_separated(result@)))]
    #[cfg_attr(creusot, invariant(
        spec::pairs_end_before(result@, left@, left_index@, right@, right_index@)))]
    while left_index < left.len() && right_index < right.len() {
        #[cfg(creusot)]
        let before = snapshot! { result };
        push_overlap(&mut result, left, left_index, right, right_index);
        if compare_upper(&left[left_index].upper, &right[right_index].upper) == Ordering::Less {
            #[cfg(creusot)]
            proof_assert! {
                spec::step_left(before@, result@, left@, left_index@, right@, right_index@);
                spec::covered(result@, left@, left_index@ + 1, right@, right_index@)
            };
            left_index += 1;
        } else {
            #[cfg(creusot)]
            proof_assert! {
                spec::step_right(before@, result@, left@, left_index@, right@, right_index@);
                spec::covered(result@, left@, left_index@, right@, right_index@ + 1)
            };
            right_index += 1;
        }
    }
    #[cfg(creusot)]
    proof_assert! {
        spec::whole_prefix(left@);
        spec::whole_prefix(right@);
        forall<value: Value::DeepModelTy> in_union(left@, value) && in_union(right@, value)
            ==> in_pairs(result@, value)
    };
    result
}

/// Append the bounds of what `left[left_index]` and `right[right_index]`
/// both hold, unless they hold nothing in common.
#[cfg_attr(creusot, requires(separated(left@) && separated(right@)))]
#[cfg_attr(creusot, requires(left_index@ < left@.len() && right_index@ < right@.len()))]
#[cfg_attr(creusot, requires(pairs_separated(pairs@)))]
#[cfg_attr(creusot, requires(
    spec::pairs_end_before(pairs@, left@, left_index@, right@, right_index@)))]
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy> in_pairs((^pairs)@, value)
    == (in_pairs(pairs@, value)
        || (spec::holds(left@[left_index@], value) && spec::holds(right@[right_index@], value)))))]
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy>
    spec::holds(left@[left_index@], value) ==> in_union(left@, value)))]
#[cfg_attr(creusot, ensures(forall<value: Value::DeepModelTy>
    spec::holds(right@[right_index@], value) ==> in_union(right@, value)))]
#[cfg_attr(creusot, ensures(pairs_separated((^pairs)@)))]
#[cfg_attr(creusot, ensures(
    upper_cmp(left@[left_index@].upper, right@[right_index@].upper) == Ordering::Less
        ==> spec::pairs_end_before((^pairs)@, left@, left_index@ + 1, right@, right_index@)))]
#[cfg_attr(creusot, ensures(
    upper_cmp(left@[left_index@].upper, right@[right_index@].upper) != Ordering::Less
        ==> spec::pairs_end_before((^pairs)@, left@, left_index@, right@, right_index@ + 1)))]
fn push_overlap<'interval, Value: Ordered>(
    pairs: &mut Vec<(&'interval Bound<Value>, &'interval Bound<Value>)>,
    left: &'interval [Interval<Value>],
    left_index: usize,
    right: &'interval [Interval<Value>],
    right_index: usize,
) {
    let lower = max_lower(&left[left_index].lower, &right[right_index].lower);
    let upper = min_upper(&left[left_index].upper, &right[right_index].upper);
    #[cfg(creusot)]
    let before = snapshot! { *pairs };
    if is_valid_interval(lower, upper) {
        pairs.push((lower, upper));
    }
    #[cfg(creusot)]
    proof_assert! {
        spec::pair_added(before@, pairs@, lower, upper, left@, left_index@, right@, right_index@);
        spec::prefix_grows(left@, left_index@);
        spec::prefix_grows(right@, right_index@);
        spec::pairs_grow(before@, pairs@, lower, upper, left@, left_index@, right@, right_index@);
        (forall<value: Value::DeepModelTy> in_pairs(pairs@, value) == (in_pairs(before@, value)
            || (spec::holds(left@[left_index@], value) && spec::holds(right@[right_index@], value))))
            && (forall<value: Value::DeepModelTy>
                spec::holds(left@[left_index@], value) ==> in_union(left@, value))
            && (forall<value: Value::DeepModelTy>
                spec::holds(right@[right_index@], value) ==> in_union(right@, value))
            && pairs_separated(pairs@)
            && (upper_cmp(left@[left_index@].upper, right@[right_index@].upper) == Ordering::Less
                ==> spec::pairs_end_before(pairs@, left@, left_index@ + 1, right@, right_index@))
            && (upper_cmp(left@[left_index@].upper, right@[right_index@].upper) != Ordering::Less
                ==> spec::pairs_end_before(pairs@, left@, left_index@, right@, right_index@ + 1))
    };
}

#[cfg(test)]
mod tests;
