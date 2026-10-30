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
    // Lower the soft limit first, so the raise has somewhere to go. On a
    // host whose soft limit already equals its hard one, every assertion
    // below holds before the raise runs at all.
    let floor = u64::try_from(hard).unwrap_or(u64::MAX).min(256);
    assert!(
        super::set_soft_limit(floor, super::current_limit().expect("readable").rlim_max),
        "lower the soft limit to {floor}",
    );
    raise_open_file_limit(CEILING);
    let soft = soft_open_file_limit().expect("the limit is readable");
    let ceiling = usize::try_from(CEILING).expect("the ceiling fits in a usize");
    // A lower bound, not the value: the raise stops at the lower of the hard
    // limit and the ceiling it was asked for, and on macOS it may stop
    // lower still, at the `OPEN_MAX` it falls back on when the kernel
    // refuses a soft limit the hard limit would allow. Which of the two a
    // macOS host lands on is the kernel's call, so only the smaller is
    // asserted there. The lowering above is what gives this teeth: without
    // it a host whose soft limit already equals its hard one satisfies the
    // bound before the raise runs.
    let expected = if cfg!(target_os = "macos") { hard.min(10_240) } else { hard.min(ceiling) };
    assert!(soft >= expected, "the raise did not reach {expected}, stopping at {soft}");
}

fn current_hard_limit() -> usize {
    usize::try_from(super::current_limit().expect("the limit is readable").rlim_max)
        .unwrap_or(usize::MAX)
}
