use std::{
    collections::BTreeSet,
    sync::{Arc, PoisonError, RwLock},
};

use indexmap::IndexMap;

/// A registry's teams: each team name mapped to its member usernames, in
/// declaration order.
pub type Teams = IndexMap<String, BTreeSet<String>>;

/// The live roster of one registry's teams.
///
/// Every [`crate::AccessToken::Team`] a registry's rules hold shares the
/// registry's directory, so a roster written through [`Self::replace`]
/// reaches every rule at once. A team the roster does not hold admits
/// nobody. Clones share one roster, and two directories are equal only when
/// they share it.
#[derive(Debug, Default, Clone)]
pub struct TeamDirectory(Arc<RwLock<Arc<Teams>>>);

impl TeamDirectory {
    #[must_use]
    pub fn new(teams: Teams) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(teams))))
    }

    /// The roster as it stands now. Later writes do not change it.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Teams> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn replace(&self, teams: Teams) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(teams);
    }

    #[must_use]
    pub fn has_member(&self, team: &str, username: &str) -> bool {
        self.snapshot()
            .get(team)
            .is_some_and(|members| members.contains(username))
    }
}

impl PartialEq for TeamDirectory {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for TeamDirectory {}
