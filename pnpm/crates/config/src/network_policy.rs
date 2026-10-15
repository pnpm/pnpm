use crate::Config;

/// Which sources a resolve may read registry metadata from, and in which
/// order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// The registry settles every pick whose answer fresh metadata could
    /// change. The mirror answers the rest.
    #[default]
    Online,
    /// The mirror answers first and its pick is taken unverified. The
    /// registry is the fallback.
    PreferCache,
    /// The registry is not a source. A pick the mirror and the store cannot
    /// settle fails instead.
    Offline,
}

impl NetworkPolicy {
    /// The policy the `offline` and `preferOffline` settings describe
    /// between them.
    ///
    /// `offline` withdraws the registry that `preferOffline` only reorders,
    /// so it decides alone and the pair describes three states, not four.
    #[must_use]
    pub fn from_settings(offline: bool, prefer_offline: bool) -> Self {
        match (offline, prefer_offline) {
            (true, _) => Self::Offline,
            (false, true) => Self::PreferCache,
            (false, false) => Self::Online,
        }
    }

    /// Whether the registry is out of reach, leaving the mirror and the
    /// store as the only sources.
    #[must_use]
    pub fn forbids_registry(self) -> bool {
        match self {
            Self::Online | Self::PreferCache => false,
            Self::Offline => true,
        }
    }

    /// Whether the mirror is read before the registry. Its negation is the
    /// pick the registry decides, which may not be settled from the mirror
    /// at all when fresh metadata could change the answer.
    #[must_use]
    pub fn reads_cache_first(self) -> bool {
        match self {
            Self::Online => false,
            Self::PreferCache | Self::Offline => true,
        }
    }

    /// Whether the mirror is preferred while the registry remains
    /// reachable, which is the one state in which a cached pick is taken
    /// unverified rather than for want of an alternative.
    #[must_use]
    pub fn prefers_reachable_cache(self) -> bool {
        match self {
            Self::Online | Self::Offline => false,
            Self::PreferCache => true,
        }
    }
}

impl Config {
    /// See [`NetworkPolicy::from_settings`].
    #[must_use]
    pub fn network_policy(&self) -> NetworkPolicy {
        NetworkPolicy::from_settings(self.offline, self.prefer_offline)
    }
}

#[cfg(test)]
mod tests;
