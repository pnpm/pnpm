//! The controller that moves the gate's width toward the volume's knee.

/// The fewest calls the gate ever admits at once.
pub(super) const MIN_PERMITS: usize = 2;

/// The most calls the gate ever admits at once: the global rayon pool's
/// ceiling. A wider gate admits every rayon worker.
pub(super) const MAX_PERMITS: usize = 16;

/// Where the climb starts without a saved width: the middle of the
/// measured knees (4 and 16).
pub(super) const INITIAL_PERMITS: usize = 8;

/// The relative difference below which two widths count as equally
/// fast. Covers the run-to-run noise of a window; a smaller margin
/// chases noise.
const MARGIN: f64 = 0.05;

/// Hill-climbs the gate width on the completion rate of the calls it
/// admits.
///
/// The rate drifts through an install as the mix of calls changes, by
/// more than a width change moves it, so consecutive windows cannot be
/// compared. Each step is an ABBA probe instead: one window at the
/// current width, two at a candidate next to it, one more at the
/// current width. A linear drift adds the same amount to both sums.
///
/// The candidate is a quarter of the width away, far enough that a real
/// difference clears [`MARGIN`] on the gentle slopes near the knee. A
/// faster candidate becomes the current width. A slower or equal one
/// leaves the width where it is, the next probe tries the other side,
/// and the climb first rests at the current width for a number of
/// windows that doubles with each probe that did not move it, up to
/// [`MAX_REST`]. A probe spends half its windows at a worse width, so
/// resting keeps the exploration from costing what the climb gains. A
/// flat stretch, where the width does not limit the rate, never moves
/// the width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Climb {
    base: usize,
    direction: Direction,
    probe: Probe,
    rest: usize,
    resting: usize,
}

/// The most windows the climb rests between two probes.
const MAX_REST: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
}

impl Direction {
    const fn reversed(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

/// The windows of the probe in progress: the current width's rates in
/// slots 0 and 3, the candidate's in 1 and 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Probe {
    rates: [u64; 4],
    observed: usize,
}

impl Climb {
    pub(super) fn new(permits: usize) -> Self {
        let base = permits.clamp(MIN_PERMITS, MAX_PERMITS);
        let mut climb =
            Self { base, direction: Direction::Down, probe: Probe::default(), rest: 0, resting: 0 };
        climb.face_inward();
        climb
    }

    /// The width the climb has settled on so far.
    pub(super) const fn settled(&self) -> usize {
        self.base
    }

    /// The width to admit during the current window.
    pub(super) fn permits(&self) -> usize {
        match (self.resting, self.probe.observed) {
            (0, 1 | 2) => self.candidate(),
            _ => self.base,
        }
    }

    /// Record the rate of the window that ran at [`Self::permits`], in
    /// completed calls per second.
    pub(super) fn observe(&mut self, calls_per_second: u64) {
        if self.resting > 0 {
            self.resting -= 1;
            return;
        }
        self.probe.rates[self.probe.observed] = calls_per_second;
        self.probe.observed += 1;
        if self.probe.observed == self.probe.rates.len() {
            self.decide();
        }
    }

    fn decide(&mut self) {
        let [base_before, candidate_first, candidate_second, base_after] = self
            .probe
            .rates
            .map(|rate| rate as f64);
        if candidate_first + candidate_second > (base_before + base_after) * (1.0 + MARGIN) {
            self.base = self.candidate();
            self.rest = 0;
        } else {
            self.direction = self.direction.reversed();
            self.rest = (self.rest * 2).clamp(1, MAX_REST);
            self.resting = self.rest;
        }
        self.probe = Probe::default();
        self.face_inward();
    }

    /// At a bound, probe the side that has room.
    fn face_inward(&mut self) {
        if self.candidate() == self.base {
            self.direction = self.direction.reversed();
        }
    }

    fn candidate(&self) -> usize {
        let step = self.base.div_ceil(4);
        match self.direction {
            Direction::Up => self.base.saturating_add(step).min(MAX_PERMITS),
            Direction::Down => self.base.saturating_sub(step).max(MIN_PERMITS),
        }
    }
}

#[cfg(test)]
mod tests;
