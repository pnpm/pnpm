use super::{AccessList, Identity, PackagePattern, PackageRule, RuleIndex, RuleTable};
use std::sync::{Arc, PoisonError, RwLock};

/// A registry's live rules: a handle on its current [`RuleTable`].
///
/// Clones share one table, so a table written through [`Self::replace`]
/// reaches every holder at once, including the ones built before the write.
/// Read [`Self::snapshot`] once per decision: two reads may see two tables.
#[derive(Debug, Default, Clone)]
pub struct PackageRules(Arc<RwLock<Arc<RuleTable>>>);

/// Replacement permission lists for one declared pattern. A `None` field
/// keeps the table's value.
#[derive(Debug, Default, Clone)]
pub struct RuleOverride {
    pub access: Option<AccessList>,
    pub publish: Option<AccessList>,
    pub unpublish: Option<AccessList>,
}

impl PackageRules {
    /// See [`RuleTable::new`].
    #[must_use]
    pub fn new(rules: Vec<PackageRule>, default_access: Option<AccessList>) -> Self {
        Self::from_table(RuleTable::new(rules, default_access))
    }

    #[must_use]
    pub fn from_table(table: RuleTable) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(table))))
    }

    /// Override the registry-level unpublish default (nobody).
    #[must_use]
    pub fn with_default_unpublish(self, unpublish: AccessList) -> Self {
        let table = RuleTable::clone(&self.snapshot()).with_default_unpublish(unpublish);
        Self::from_table(table)
    }

    /// Add one entry to the map. For tests and embedders that build rules
    /// programmatically; see [`RuleTable::push_rule`].
    pub fn push_rule(&mut self, rule: PackageRule) {
        let mut table = RuleTable::clone(&self.snapshot());
        table.push_rule(rule);
        self.replace(table);
    }

    /// The table as it stands now. Later writes do not change it.
    #[must_use]
    pub fn snapshot(&self) -> Arc<RuleTable> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn replace(&self, table: RuleTable) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(table);
    }

    /// See [`RuleTable::patterns`].
    #[must_use]
    pub fn patterns(&self) -> Vec<PackagePattern> {
        self.snapshot().patterns()
    }

    /// See [`RuleTable::refines_writes`].
    #[must_use]
    pub fn refines_writes(&self) -> bool {
        self.snapshot().refines_writes()
    }

    /// See [`RuleTable::refines_access`].
    #[must_use]
    pub fn refines_access(&self) -> bool {
        self.snapshot().refines_access()
    }

    /// See [`RuleTable::references_team`].
    #[must_use]
    pub fn references_team(&self, team: &str) -> bool {
        self.snapshot().references_team(team)
    }

    /// See [`RuleTable::any_access_admits`].
    #[must_use]
    pub fn any_access_admits(&self, identity: &Identity) -> bool {
        self.snapshot().any_access_admits(identity)
    }

    /// See [`RuleTable::all_access_admit`].
    #[must_use]
    pub fn all_access_admit(&self, identity: &Identity) -> bool {
        self.snapshot().all_access_admit(identity)
    }

    /// Whether `package`'s effective `access` admits `identity`.
    #[must_use]
    pub fn access_admits(&self, package: &str, identity: &Identity) -> bool {
        self.snapshot()
            .for_package(package)
            .access
            .allows(identity)
    }

    /// See [`crate::Effective::access_is_explicit`].
    #[must_use]
    pub fn access_is_explicit(&self, package: &str) -> bool {
        self.snapshot().for_package(package).access_is_explicit
    }

    /// Whether the registry-level default `access` admits `identity`.
    #[must_use]
    pub fn default_access_admits(&self, identity: &Identity) -> bool {
        self.snapshot()
            .default_access()
            .allows(identity)
    }
}

impl RuleTable {
    /// This table with `default_access` and the per-pattern `overrides`
    /// applied. Fails with the first pattern the table does not declare:
    /// an override can change who may do what, never which names the
    /// registry serves.
    pub fn with_overrides(
        &self,
        default_access: Option<AccessList>,
        overrides: Vec<(PackagePattern, RuleOverride)>,
    ) -> Result<Self, PackagePattern> {
        let mut table = self.clone();
        if let Some(access) = default_access {
            table.default_access = access;
        }
        for (pattern, lists) in overrides {
            let Some(rule) = table.rules
                .iter_mut()
                .find(|rule| rule.pattern == pattern)
            else {
                return Err(pattern);
            };
            rule.access = lists.access.or_else(|| rule.access.take());
            rule.publish = lists.publish.or_else(|| rule.publish.take());
            rule.unpublish = lists.unpublish.or_else(|| rule.unpublish.take());
        }
        table.index = RuleIndex::build(&table.rules);
        Ok(table)
    }

    /// The declared entries, in declaration order.
    #[must_use]
    pub fn rules(&self) -> &[PackageRule] {
        &self.rules
    }
}
