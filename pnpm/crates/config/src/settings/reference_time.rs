//! The `referenceTime` setting.

use crate::{
    Config,
    TrustPolicy,
};
use jiff::Timestamp;
use pnpm_resolving_resolver_base::lag_minutes;

impl Config {
    /// The time `minimumReleaseAge` and `trustPolicyIgnoreAfter` count back
    /// from: [`Self::reference_time`] when it is set, the current
    /// time otherwise.
    #[must_use]
    pub fn resolved_reference_time(&self) -> Timestamp {
        self.reference_time.unwrap_or_else(Timestamp::now)
    }

    /// Whether [`Self::reference_time`] changes anything: it is set,
    /// and a `minimumReleaseAge` cutoff or, under `trustPolicy: no-downgrade`,
    /// a `trustPolicyIgnoreAfter` window counts back from it.
    #[must_use]
    pub fn reference_time_applies(&self) -> bool {
        let trust_window = self.trust_policy == TrustPolicy::NoDowngrade
            && self.trust_policy_ignore_after.is_some();
        self.reference_time.is_some()
            && (self.resolved_minimum_release_age().is_some() || trust_window)
    }

    /// [`Self::resolved_minimum_release_age`] measured back from the current
    /// time: the window plus how far [`Self::reference_time`] lags
    /// the clock. A pnpr package document dates versions only within a
    /// window measured from its last publish, so this is what it must cover.
    #[must_use]
    pub fn release_age_window_from_now(&self) -> Option<u64> {
        let lag = match self.reference_time {
            Some(time) => lag_minutes(time, Timestamp::now()),
            None => 0,
        };
        self.resolved_minimum_release_age()
            .map(|minutes| minutes.saturating_add(lag))
    }
}
