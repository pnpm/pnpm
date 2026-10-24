use super::{raise_open_file_limit, soft_open_file_limit};

#[test]
fn the_soft_limit_is_readable() {
    assert!(soft_open_file_limit().is_some_and(|limit| limit > 0));
}

/// Pins that a raise never lowers the limit, which is what lets a caller
/// raise unconditionally at startup without checking what it already has.
#[test]
fn raising_never_lowers_the_limit() {
    let before = soft_open_file_limit().expect("the limit is readable");
    raise_open_file_limit(1);
    let after = soft_open_file_limit().expect("the limit is readable");
    assert!(after >= before, "limit fell from {before} to {after}");
}

/// Pins that the raise reaches the hard limit, which is the capacity the
/// registry mirror sizes its descriptor pool against.
#[test]
fn raising_reaches_the_hard_limit() {
    const CEILING: u64 = 1 << 20;
    let hard = current_hard_limit();
    raise_open_file_limit(CEILING);
    let soft = soft_open_file_limit().expect("the limit is readable");
    let ceiling = usize::try_from(CEILING).expect("the ceiling fits in a usize");
    // The raise stops at the lower of the hard limit and the ceiling it was
    // asked for, and macOS caps it again below an unlimited hard limit at
    // the `OPEN_MAX` it falls back on. A host whose hard limit is unlimited
    // reports one far above the ceiling, so the ceiling is what it reaches.
    let expected = if cfg!(target_os = "macos") { hard.min(10_240) } else { hard.min(ceiling) };
    assert!(soft >= expected, "soft {soft} did not reach {expected}");
}

fn current_hard_limit() -> usize {
    usize::try_from(super::current_limit().expect("the limit is readable").rlim_max)
        .unwrap_or(usize::MAX)
}
