//! The controller that moves the gate's width toward the volume's knee.

/// The fewest calls the gate ever admits at once.
pub(super) const MIN_PERMITS: usize = 2;

/// The most calls the gate ever admits at once: the global rayon pool's
/// ceiling. A wider gate admits every rayon worker, so the climb would
/// only wander there on noise.
pub(super) const MAX_PERMITS: usize = 16;

/// Where the climb starts without a saved width: the middle of the
/// measured knees (4 and 16).
pub(super) const INITIAL_PERMITS: usize = 8;

/// The relative rate change below which two widths count as equal.
/// Covers the run-to-run noise of a window; a smaller margin chases
/// noise.
const MARGIN: f64 = 0.05;

/// Hill-climbs the gate width on the completion rate of the calls it
/// admits.
///
/// The rate as a function of the width is unimodal on APFS: it rises
/// to the knee and falls past it. Each observation compares the rate
/// at the current width with the rate at the previous one. A higher
/// rate keeps the direction, a lower one reverses it, and an equal one
/// moves down, since the narrower of two equally fast widths burns
/// less CPU in the kernel. The step starts at a quarter of the width
/// and halves on every reversal, so the climb crosses the measured
/// range in a few observations and then settles next to the knee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Climb {
    permits: usize,
    direction: Direction,
    step: usize,
    last_rate: Option<u64>,
}

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

impl Climb {
    pub(super) fn new(permits: usize) -> Self {
        let permits = permits.clamp(MIN_PERMITS, MAX_PERMITS);
        Self { permits, direction: Direction::Up, step: coarse_step(permits), last_rate: None }
    }

    /// A climb that starts at a width a previous install settled on, so
    /// it probes the neighbours first and only strides out if the knee
    /// moved.
    pub(super) fn seeded(permits: usize) -> Self {
        Self { step: 1, ..Self::new(permits) }
    }

    pub(super) const fn permits(&self) -> usize {
        self.permits
    }

    /// Record the rate measured at [`Self::permits`], in completed calls
    /// per second, and move to the next width to try.
    pub(super) fn observe(&mut self, calls_per_second: u64) -> usize {
        if let Some(last) = self.last_rate {
            let last = last as f64;
            let now = calls_per_second as f64;
            if now > last * (1.0 + MARGIN) {
                self.step = self.step
                    .saturating_mul(2)
                    .min(coarse_step(self.permits));
            } else {
                self.direction = if now < last * (1.0 - MARGIN) {
                    self.direction.reversed()
                } else {
                    Direction::Down
                };
                self.step = (self.step / 2).max(1);
            }
        }
        self.last_rate = Some(calls_per_second);
        self.permits = match self.direction {
            Direction::Up => self.permits.saturating_add(self.step).min(MAX_PERMITS),
            Direction::Down => self.permits.saturating_sub(self.step).max(MIN_PERMITS),
        };
        self.permits
    }
}

/// The step that crosses the measured range in a few observations.
/// Reversals halve the step down to 1, and improvements double it
/// back up to this.
fn coarse_step(permits: usize) -> usize {
    (permits / 4).max(1)
}

#[cfg(test)]
mod tests;
