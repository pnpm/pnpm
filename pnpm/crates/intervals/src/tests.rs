use super::{
    Bound,
    Interval,
    intersect_intervals,
    is_valid_interval,
    max_lower,
    merge_overlapping_intervals,
    min_upper,
};
use pretty_assertions::assert_eq;
use std::cmp::Ordering;

/// The values the tests probe: every bound value, and one on each side.
const PROBES: [i32; 4] = [-1, 0, 1, 2];

fn bounds() -> Vec<Bound<i32>> {
    let mut all = vec![Bound::Unbounded];
    for value in 0..=1 {
        all.push(Bound::Inclusive(value));
        all.push(Bound::Exclusive(value));
    }
    all
}

fn intervals() -> Vec<Interval<i32>> {
    let mut all = Vec::new();
    for lower in bounds() {
        for upper in bounds() {
            all.push(Interval { lower: lower.clone(), upper });
        }
    }
    all
}

/// Every list of up to two intervals.
fn unions() -> Vec<Vec<Interval<i32>>> {
    let mut all = vec![Vec::new()];
    for first in intervals() {
        all.push(vec![first.clone()]);
        for second in intervals() {
            all.push(vec![first.clone(), second]);
        }
    }
    all
}

fn in_union(intervals: &[Interval<i32>], value: i32) -> bool {
    intervals
        .iter()
        .any(|interval| interval.contains(&value))
}

fn is_separated(intervals: &[Interval<i32>]) -> bool {
    let below = |upper: &Bound<i32>, value: i32| {
        Interval { lower: Bound::Unbounded, upper: upper.clone() }.contains(&value)
    };
    let above = |lower: &Bound<i32>, value: i32| {
        Interval { lower: lower.clone(), upper: Bound::Unbounded }.contains(&value)
    };
    intervals
        .iter()
        .enumerate()
        .all(|(index, earlier)| {
            intervals[index + 1..]
                .iter()
                .all(|later| {
                    PROBES
                        .iter()
                        .all(|&value| !(below(&earlier.upper, value) && above(&later.lower, value)))
                })
        })
}

#[test]
fn an_interval_of_the_tighter_bounds_holds_what_both_hold() {
    for left in intervals() {
        for right in intervals() {
            let both = Interval {
                lower: max_lower(&left.lower, &right.lower).clone(),
                upper: min_upper(&left.upper, &right.upper).clone(),
            };
            for value in PROBES {
                assert_eq!(
                    both.contains(&value),
                    left.contains(&value) && right.contains(&value),
                    "{left:?} and {right:?} at {value}",
                );
            }
        }
    }
}

#[test]
fn invalid_bounds_hold_nothing() {
    for interval in intervals() {
        if !is_valid_interval(&interval.lower, &interval.upper) {
            assert!(
                !PROBES
                    .iter()
                    .any(|value| interval.contains(value)),
                "{interval:?}",
            );
        }
    }
}

#[test]
fn merging_keeps_the_values_and_separates_the_intervals() {
    for union in unions() {
        let merged = merge_overlapping_intervals(union.clone());
        assert!(is_separated(&merged), "{union:?} merged to {merged:?}");
        for value in PROBES {
            assert_eq!(in_union(&merged, value), in_union(&union, value), "{union:?} at {value}");
        }
    }
}

#[test]
fn the_sweep_holds_the_values_in_both_unions() {
    let merged: Vec<Vec<Interval<i32>>> = unions()
        .into_iter()
        .map(merge_overlapping_intervals)
        .collect();
    for left in &merged {
        for right in &merged {
            let both: Vec<Interval<i32>> = intersect_intervals(left, right)
                .into_iter()
                .map(|(lower, upper)| Interval { lower: lower.clone(), upper: upper.clone() })
                .collect();
            assert!(is_separated(&both), "{left:?} and {right:?} gave {both:?}");
            for value in PROBES {
                assert_eq!(
                    in_union(&both, value),
                    in_union(left, value) && in_union(right, value),
                    "{left:?} and {right:?} at {value}",
                );
            }
        }
    }
}

/// A value ordered by `key` alone, so two values can order equal and still
/// differ.
#[derive(Debug, Clone)]
struct Tagged {
    key: i32,
    tag: char,
}

impl PartialEq for Tagged {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Tagged {}

impl PartialOrd for Tagged {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tagged {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

#[test]
fn intervals_with_equal_lower_bounds_merge_in_their_given_order() {
    let tagged = |key, tag| Tagged { key, tag };
    let first = Interval {
        lower: Bound::Inclusive(tagged(1, 'a')),
        upper: Bound::Exclusive(tagged(2, 'a')),
    };
    let second = Interval {
        lower: Bound::Inclusive(tagged(1, 'b')),
        upper: Bound::Inclusive(tagged(3, 'b')),
    };
    for (union, kept) in [(vec![first.clone(), second.clone()], 'a'), (vec![second, first], 'b')] {
        let merged = merge_overlapping_intervals(union);
        assert_eq!(merged.len(), 1, "{merged:?}");
        assert!(
            matches!(&merged[0].lower, Bound::Inclusive(lower) if lower.tag == kept),
            "{merged:?}",
        );
        assert!(
            matches!(&merged[0].upper, Bound::Inclusive(upper) if upper.key == 3),
            "{merged:?}",
        );
    }
}
