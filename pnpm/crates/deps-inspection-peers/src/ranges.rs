use super::{Version, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Bound<Value> {
    Inclusive(Value),
    Exclusive(Value),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Interval {
    lower: Bound<Version>,
    upper: Bound<Version>,
}

impl Interval {
    pub(super) fn contains(&self, version: &Version) -> bool {
        let above_lower = match &self.lower {
            Bound::Inclusive(lower) => lower <= version,
            Bound::Exclusive(lower) => lower < version,
            Bound::Unbounded => true,
        };
        let below_upper = match &self.upper {
            Bound::Inclusive(upper) => version <= upper,
            Bound::Exclusive(upper) => version < upper,
            Bound::Unbounded => true,
        };
        above_lower && below_upper
    }
}

/// The bound as a user reads it, without the trailing `-0`.
///
/// For a bound [`derived_upper`] built, the suffix is an implementation
/// detail of prerelease matching. For one the user wrote out it is not
/// — but pnpm drops it too (`semver-range-intersect` renders
/// `intersect("<2.0.0-0", ">=1.0.0")` as `>=1.0.0 <2.0.0`), so telling
/// the two apart here would diverge rather than converge. Only the
/// rendering loses it: matching compares the parsed version, where the
/// suffix still excludes every prerelease of that release.
fn without_derived_suffix(version: &Version) -> String {
    if version.pre_release == [node_semver::Identifier::Numeric(0)] {
        format!("{}.{}.{}", version.major, version.minor, version.patch)
    } else {
        version.to_string()
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let upper = match &self.upper {
            Bound::Inclusive(version) | Bound::Exclusive(version) => {
                without_derived_suffix(version)
            }
            Bound::Unbounded => String::new(),
        };
        match (&self.lower, &self.upper) {
            (Bound::Unbounded, Bound::Unbounded) => write!(formatter, "*"),
            (Bound::Inclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">={version_lower}")
            }
            (Bound::Exclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">{version_lower}")
            }
            (Bound::Unbounded, Bound::Inclusive(_)) => write!(formatter, "<={upper}"),
            (Bound::Unbounded, Bound::Exclusive(_)) => write!(formatter, "<{upper}"),
            (Bound::Inclusive(version_lower), Bound::Inclusive(version_upper)) => {
                if version_lower == version_upper {
                    write!(formatter, "{version_lower}")
                } else {
                    write!(formatter, ">={version_lower} <={upper}")
                }
            }
            (Bound::Inclusive(version_lower), Bound::Exclusive(_)) => {
                write!(formatter, ">={version_lower} <{upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Inclusive(_)) => {
                write!(formatter, ">{version_lower} <={upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Exclusive(_)) => {
                write!(formatter, ">{version_lower} <{upper}")
            }
        }
    }
}

/// The tighter of two lower bounds.
fn max_lower(left_bound: &Bound<Version>, right_bound: &Bound<Version>) -> Bound<Version> {
    match (lower_rank(left_bound), lower_rank(right_bound)) {
        (None, _) => right_bound.clone(),
        (_, None) => left_bound.clone(),
        (left, right) => {
            if left >= right {
                left_bound.clone()
            } else {
                right_bound.clone()
            }
        }
    }
}

/// The tighter of two upper bounds.
fn min_upper(left_bound: &Bound<Version>, right_bound: &Bound<Version>) -> Bound<Version> {
    match (upper_rank(left_bound), upper_rank(right_bound)) {
        (None, _) => right_bound.clone(),
        (_, None) => left_bound.clone(),
        (left, right) => {
            if left <= right {
                left_bound.clone()
            } else {
                right_bound.clone()
            }
        }
    }
}

/// Orders lower bounds by the versions they admit, `None` for unbounded. At
/// the same version the exclusive bound is the higher one: it excludes that
/// version, the inclusive one admits it.
fn lower_rank(bound: &Bound<Version>) -> Option<(&Version, u8)> {
    match bound {
        Bound::Unbounded => None,
        Bound::Inclusive(version) => Some((version, 0)),
        Bound::Exclusive(version) => Some((version, 1)),
    }
}

/// Orders upper bounds by the versions they admit, `None` for unbounded. At
/// the same version the exclusive bound is the lower one.
fn upper_rank(bound: &Bound<Version>) -> Option<(&Version, u8)> {
    match bound {
        Bound::Unbounded => None,
        Bound::Exclusive(version) => Some((version, 0)),
        Bound::Inclusive(version) => Some((version, 1)),
    }
}

fn is_valid_interval(lower: &Bound<Version>, upper: &Bound<Version>) -> bool {
    match (lower, upper) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
        (Bound::Inclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version <= right_version
        }
        (Bound::Inclusive(left_version), Bound::Exclusive(right_version)) => {
            left_version < right_version
        }
        (Bound::Exclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version < right_version
        }
        (Bound::Exclusive(left_version), Bound::Exclusive(right_version)) => {
            left_version < right_version
        }
    }
}

/// Pad a partial version to `major.minor.patch`, reading the `x`, `X` and
/// `*` placeholders as zero. Anything else non-numeric is left untouched for
/// the caller's parser to reject; content past the patch component (a
/// prerelease or build tag) is carried through.
pub(super) fn normalize_version_str(version_raw: &str) -> String {
    let version_raw = version_raw.trim();
    let version_parts: Vec<&str> = version_raw.split('.').collect();
    let numeric: Vec<String> =
        version_parts.iter().take(3).map(|part| part.replace(['x', 'X', '*'], "0")).collect();
    if !numeric.iter().all(|part| part.chars().all(|character| character.is_ascii_digit())) {
        return version_raw.to_string();
    }
    let mut padded = numeric;
    padded.resize(3, "0".to_string());
    let rest = version_parts.get(3..).unwrap_or_default();
    let parts = padded.iter().map(String::as_str).chain(rest.iter().copied());
    parts.collect::<Vec<_>>().join(".")
}

/// How many of `major.minor.patch` a range's version actually pins.
/// `1` and `1.x` pin one, `1.2` pins two, `1.2.3` pins three. npm's
/// comparators widen to the next unpinned level — `~1` reaches `2.0.0`,
/// not `1.1.0` — so the count has to survive the padding
/// [`normalize_version_str`] applies.
fn version_specificity(version_raw: &str) -> usize {
    let mut specificity = 0;
    for part in version_raw.trim().split('.') {
        let head = part.split(['-', '+']).next().unwrap_or(part);
        if head.is_empty() || head.chars().all(|character| matches!(character, 'x' | 'X' | '*')) {
            break;
        }
        specificity += 1;
        if specificity == 3 {
            break;
        }
    }
    specificity
}

pub(super) fn at(major: u64, minor: u64, patch: u64) -> Version {
    Version { major, minor, patch, build: Vec::new(), pre_release: Vec::new() }
}

/// An upper bound npm derived rather than the user writing it out:
/// `^1.2.3` desugars to `<2.0.0-0`, not `<2.0.0`, so that no prerelease
/// of 2.0.0 slips in. A bound the user spelled in full (`<2.0.0`) keeps
/// its plain form and does admit `2.0.0-rc.1`. The suffix is dropped
/// again when the interval is rendered — see [`Interval`]'s `Display`.
fn derived_upper(version: Version) -> Version {
    Version { pre_release: vec![node_semver::Identifier::Numeric(0)], ..version }
}

/// The exclusive upper bound of the level `specificity` leaves
/// unpinned: the next major for a bare major, the next minor for
/// `major.minor`.
fn next_unpinned(version: &Version, specificity: usize) -> Version {
    let next = if specificity >= 2 {
        at(version.major, version.minor + 1, 0)
    } else {
        at(version.major + 1, 0, 0)
    };
    derived_upper(next)
}

fn parse_comparator(comparator: &str) -> Option<Interval> {
    let comparator = comparator.trim();
    if comparator == "*" || comparator.is_empty() {
        return Some(Interval { lower: Bound::Unbounded, upper: Bound::Unbounded });
    }

    let (operator, version_str) = split_operator(comparator);
    let specificity = version_specificity(version_str);
    // Nothing is pinned (`x`, `~x`, `^*`): every comparator over it
    // admits every version.
    if specificity == 0 {
        return Some(Interval { lower: Bound::Unbounded, upper: Bound::Unbounded });
    }
    let version = Version::parse(normalize_version_str(version_str)).ok()?;

    comparator_interval(operator, version, specificity)
}

fn comparator_interval(operator: &str, version: Version, specificity: usize) -> Option<Interval> {
    match operator {
        "=" if specificity == 3 => Some(Interval {
            lower: Bound::Inclusive(version.clone()),
            upper: Bound::Inclusive(version),
        }),
        // A partial bare version is npm's implicit range: `1.2` is
        // every 1.2.x, not the single version 1.2.0.
        "=" | "~" => Some(Interval {
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        "=>" => Some(Interval { lower: Bound::Inclusive(version), upper: Bound::Unbounded }),
        ">" if specificity == 3 => {
            Some(Interval { lower: Bound::Exclusive(version), upper: Bound::Unbounded })
        }
        // `>1.2` excludes all of 1.2.x, so it starts at 1.3.0.
        ">" => Some(Interval {
            lower: Bound::Inclusive(next_unpinned(&version, specificity)),
            upper: Bound::Unbounded,
        }),
        "<=" if specificity == 3 => {
            Some(Interval { lower: Bound::Unbounded, upper: Bound::Inclusive(version) })
        }
        // `<=1.2` admits all of 1.2.x.
        "<=" => Some(Interval {
            lower: Bound::Unbounded,
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
        }),
        "<" if specificity == 3 => {
            Some(Interval { lower: Bound::Unbounded, upper: Bound::Exclusive(version) })
        }
        // `<1.2` excludes all of 1.2.x, prereleases included.
        "<" => Some(Interval {
            lower: Bound::Unbounded,
            upper: Bound::Exclusive(derived_upper(version)),
        }),
        "^" => Some(Interval {
            upper: Bound::Exclusive(caret_upper(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        _ => None,
    }
}

/// The comparator's operator and the version text after it. A version with
/// no operator is npm's implicit `=`.
fn split_operator(comparator: &str) -> (&str, &str) {
    for (prefix, operator) in
        [(">=", "=>"), (">", ">"), ("<=", "<="), ("<", "<"), ("^", "^"), ("~", "~")]
    {
        if let Some(rest) = comparator.strip_prefix(prefix) {
            return (operator, rest);
        }
    }
    ("=", comparator)
}

/// The exclusive upper bound of `^`: the next level up from the leftmost
/// non-zero component, or from the level the range leaves unpinned.
fn caret_upper(version: &Version, specificity: usize) -> Version {
    let upper = if specificity == 1 || version.major > 0 {
        at(version.major + 1, 0, 0)
    } else if specificity == 2 || version.minor > 0 {
        at(0, version.minor + 1, 0)
    } else {
        at(0, 0, version.patch + 1)
    };
    derived_upper(upper)
}

pub(super) fn preprocess_hyphen_ranges(range: &str) -> String {
    let mut parts = Vec::new();
    for part in range.split("||") {
        let part = part.trim();
        if let Some((start, end)) = part.split_once(" - ") {
            parts.push(format!(">={} <={}", start.trim(), end.trim()));
        } else {
            parts.push(part.to_string());
        }
    }
    parts.join(" || ")
}

pub(super) fn parse_range_to_intervals(range: &str) -> Option<Vec<Interval>> {
    let mut intervals = Vec::new();
    for part in range.split("||").map(str::trim).filter(|part| !part.is_empty()) {
        let part_interval = parse_comparator_set(part)?;
        if is_valid_interval(&part_interval.lower, &part_interval.upper) {
            intervals.push(part_interval);
        }
    }
    if intervals.is_empty() { None } else { Some(intervals) }
}

/// Intersect the space-separated comparators of one `||` alternative. An
/// empty intersection is reported as the empty interval, which the caller
/// then drops.
fn parse_comparator_set(part: &str) -> Option<Interval> {
    let mut interval = Interval { lower: Bound::Unbounded, upper: Bound::Unbounded };
    for comparator in part.split_whitespace() {
        let comparator_interval = parse_comparator(comparator)?;
        let lower = max_lower(&interval.lower, &comparator_interval.lower);
        let upper = min_upper(&interval.upper, &comparator_interval.upper);
        if !is_valid_interval(&lower, &upper) {
            let zero = Version::parse("0.0.0").expect("0.0.0 is a valid version");
            return Some(Interval {
                lower: Bound::Inclusive(zero.clone()),
                upper: Bound::Exclusive(zero),
            });
        }
        interval = Interval { lower, upper };
    }
    Some(interval)
}

fn intersect_intervals(left_intervals: &[Interval], right_intervals: &[Interval]) -> Vec<Interval> {
    let mut result = Vec::new();
    for left_interval in left_intervals {
        for right_interval in right_intervals {
            let lower = max_lower(&left_interval.lower, &right_interval.lower);
            let upper = min_upper(&left_interval.upper, &right_interval.upper);
            if is_valid_interval(&lower, &upper) {
                result.push(Interval { lower, upper });
            }
        }
    }
    result
}

pub(super) fn intersect_multiple_ranges(version_ranges: &[String]) -> Option<String> {
    if version_ranges.is_empty() {
        return Some("*".to_string());
    }
    let mut current_intervals =
        parse_range_to_intervals(&preprocess_hyphen_ranges(&version_ranges[0]))?;
    for range in &version_ranges[1..] {
        let next_intervals = parse_range_to_intervals(&preprocess_hyphen_ranges(range))?;
        current_intervals = intersect_intervals(&current_intervals, &next_intervals);
        if current_intervals.is_empty() {
            return None;
        }
    }
    Some(
        current_intervals
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" || "),
    )
}
