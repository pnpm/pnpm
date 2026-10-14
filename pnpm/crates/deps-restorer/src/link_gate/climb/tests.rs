use super::{Climb, INITIAL_PERMITS, MAX_PERMITS, MIN_PERMITS};

/// Calls per second on a volume whose knee is `knee`: linear below it,
/// falling past it the way the measured `clonefile` sweeps did.
fn volume(knee: usize) -> impl Fn(usize) -> u64 {
    move |permits| {
        let knee = knee as f64;
        let permits = permits as f64;
        let rate = if permits <= knee { permits / knee } else { (knee / permits).powf(0.7) };
        (rate * 10_000.0) as u64
    }
}

fn settle(climb: &mut Climb, rate: impl Fn(usize) -> u64, observations: usize) -> Vec<usize> {
    (0..observations)
        .map(|_| climb.observe(rate(climb.permits())))
        .collect()
}

#[track_caller]
fn assert_near(knee: usize, widths: &[usize]) {
    eprintln!("knee {knee}: {widths:?}");
    for width in widths {
        assert!(
            (*width as f64) >= knee as f64 * 0.7 && (*width as f64) <= knee as f64 * 1.3,
            "{width} is not near the knee {knee}",
        );
    }
}

#[test]
fn climbs_down_to_a_knee_of_4() {
    let mut climb = Climb::new(INITIAL_PERMITS);
    let widths = settle(&mut climb, volume(4), 30);
    assert_near(4, &widths[10..]);
}

#[test]
fn climbs_up_to_a_knee_of_16() {
    let mut climb = Climb::new(INITIAL_PERMITS);
    let widths = settle(&mut climb, volume(16), 30);
    assert_near(16, &widths[10..]);
}

#[test]
fn leaves_a_seeded_knee() {
    let mut climb = Climb::seeded(16);
    let widths = settle(&mut climb, volume(16), 20);
    assert_near(16, &widths);
}

#[test]
fn stays_within_the_bounds() {
    let mut climb = Climb::new(MAX_PERMITS);
    let widths = settle(&mut climb, |_| 1, 20);
    assert!(
        widths
            .iter()
            .all(|width| (MIN_PERMITS..=MAX_PERMITS).contains(width)),
        "{widths:?}"
    );
    let mut climb = Climb::new(MIN_PERMITS);
    let widths = settle(&mut climb, |permits| permits as u64 * 1000, 20);
    assert!(
        widths
            .iter()
            .all(|width| (MIN_PERMITS..=MAX_PERMITS).contains(width)),
        "{widths:?}"
    );
    assert!(*widths.last().unwrap() >= MAX_PERMITS - 2, "{widths:?}");
}
