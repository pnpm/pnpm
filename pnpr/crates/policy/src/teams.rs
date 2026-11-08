use std::{
    collections::BTreeSet,
    sync::{Arc, PoisonError, RwLock},
};

use indexmap::IndexMap;

/// A registry's teams indexed by immutable ID, in declaration order.
pub type Teams = IndexMap<String, Team>;

/// A team identity with a mutable display name and account-ID memberships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub name: String,
    pub members: BTreeSet<String>,
}

impl Team {
    #[must_use]
    pub fn new(name: impl Into<String>, members: BTreeSet<String>) -> Self {
        Self { name: name.into(), members }
    }
}

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
            .is_some_and(|members| members.members.contains(username))
    }
}

impl PartialEq for TeamDirectory {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for TeamDirectory {}
