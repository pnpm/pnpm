use crate::{Config, NetworkPolicy};

impl Config {
    /// See [`NetworkPolicy::from_settings`].
    #[must_use]
    pub fn network_policy(&self) -> NetworkPolicy {
        NetworkPolicy::from_settings(self.offline, self.prefer_offline)
    }
}
