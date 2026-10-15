use super::{Climb, INITIAL_PERMITS, MAX_PERMITS, MIN_PERMITS};

/// Calls per second at `permits` on a volume whose knee is `knee`:
/// rising linearly below it and falling past it about as gently as the
/// measured installs did (4 against 16 permits is about 30%).
fn volume(knee: usize) -> impl Fn(usize) -> f64 {
    move |permits| {
        let (knee, permits) = (knee as f64, permits as f64);
        if permits <= knee { permits / knee } else { (knee / permits).powf(0.25) }
    }
}

/// Run `windows` windows in which the rate also triples from the first
/// to the last, whatever the width, as the call mix of a real install
/// does. Returns the settled width after each window.
fn run(climb: &mut Climb, width_effect: impl Fn(usize) -> f64, windows: usize) -> Vec<usize> {
    (0..windows)
        .map(|window| {
            let drift = 1.0 + 2.0 * window as f64 / windows as f64;
            climb.observe((width_effect(climb.permits()) * drift * 5000.0) as u64);
            climb.settled()
        })
        .collect()
}

#[track_caller]
fn assert_settles_within(settled: &[usize], range: std::ops::RangeInclusive<usize>) {
    eprintln!("{settled:?}");
    let last = *settled.last().unwrap();
    assert!(range.contains(&last), "settled at {last}, outside {range:?}");
}

#[test]
fn finds_a_knee_of_4_despite_a_rising_rate() {
    let mut climb = Climb::new(INITIAL_PERMITS);
    assert_settles_within(&run(&mut climb, volume(4), 40), 3..=5);
}

#[test]
fn finds_a_knee_of_16_despite_a_rising_rate() {
    let mut climb = Climb::new(INITIAL_PERMITS);
    assert_settles_within(&run(&mut climb, volume(16), 40), 13..=16);
}

#[test]
fn leaves_a_seeded_ceiling_when_the_knee_is_4() {
    let mut climb = Climb::new(MAX_PERMITS);
    assert_settles_within(&run(&mut climb, volume(4), 80), 3..=5);
}

#[test]
fn keeps_a_seeded_knee() {
    let mut climb = Climb::new(4);
    let settled = run(&mut climb, volume(4), 40);
    eprintln!("{settled:?}");
    assert!(
        settled
            .iter()
            .all(|width| (3..=5).contains(width)),
        "{settled:?}"
    );
}

#[test]
fn holds_its_width_when_only_the_rate_drifts() {
    let mut climb = Climb::new(INITIAL_PERMITS);
    let settled = run(&mut climb, |_| 1.0, 40);
    assert!(
        settled
            .iter()
            .all(|width| *width == INITIAL_PERMITS),
        "{settled:?}"
    );
}

#[test]
fn probes_stay_within_the_bounds() {
    for start in [MIN_PERMITS, MAX_PERMITS] {
        let mut climb = Climb::new(start);
        for window in 0..40 {
            climb.observe(window);
            assert!((MIN_PERMITS..=MAX_PERMITS).contains(&climb.permits()), "{climb:?}");
        }
    }
}
