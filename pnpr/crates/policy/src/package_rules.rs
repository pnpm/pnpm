use super::{AccessList, Action, Grant, Identity, PackagePattern, PackageRule, RuleTable};
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

    /// See [`RuleTable::with_default_publish`].
    #[must_use]
    pub fn with_default_publish(self, publish: AccessList) -> Self {
        let table = RuleTable::clone(&self.snapshot()).with_default_publish(publish);
        Self::from_table(table)
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

    /// Cover the registry with `grants` in place of the ones it had, keeping
    /// its claimed patterns.
    pub fn set_grants(&self, grants: Vec<Grant>) {
        self.replace(RuleTable::of_grants(self.patterns(), grants));
    }

    /// See [`RuleTable::patterns`].
    #[must_use]
    pub fn patterns(&self) -> Vec<PackagePattern> {
        self.snapshot().patterns()
    }

    /// See [`RuleTable::references_team`].
    #[must_use]
    pub fn references_team(&self, team: &str) -> bool {
        self.snapshot().references_team(team)
    }

    /// See [`RuleTable::any_admits`].
    #[must_use]
    pub fn any_admits(&self, action: Action, identity: &Identity) -> bool {
        self.snapshot().any_admits(action, identity)
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

    /// Whether `identity` may read `package`, without being its owner.
    #[must_use]
    pub fn access_admits(&self, package: &str, identity: &Identity) -> bool {
        self.snapshot().permits(package, Action::Read, identity, false)
    }

    /// See [`RuleTable::refines_access`].
    #[must_use]
    pub fn refines_access(&self) -> bool {
        self.snapshot().refines_access()
    }

    /// See [`RuleTable::refines_read_of`].
    #[must_use]
    pub fn refines_read_of(&self, package: &str) -> bool {
        self.snapshot().refines_read_of(package)
    }
}

impl RuleTable {
    /// This table with `default_access` and the per-pattern `overrides`
    /// applied to its rules. Fails with the first pattern the table does not
    /// declare.
    pub fn with_overrides(
        &self,
        default_access: Option<AccessList>,
        overrides: Vec<(PackagePattern, RuleOverride)>,
    ) -> Result<Self, PackagePattern> {
        let mut rules = self.rules().to_vec();
        for (pattern, lists) in overrides {
            let Some(rule) = rules
                .iter_mut()
                .find(|rule| rule.pattern == pattern)
            else {
                return Err(pattern);
            };
            rule.access = lists.access.or_else(|| rule.access.take());
            rule.publish = lists.publish.or_else(|| rule.publish.take());
            rule.unpublish = lists.unpublish.or_else(|| rule.unpublish.take());
        }
        let access = default_access.unwrap_or_else(|| self.default_access().clone());
        Ok(Self::new(rules, Some(access))
            .with_default_publish(self.default_publish().clone())
            .with_default_unpublish(self.default_unpublish().clone()))
    }

    /// The rules the table was built from, in declaration order.
    #[must_use]
    pub fn rules(&self) -> &[PackageRule] {
        &self.rules
    }

    #[must_use]
    pub fn default_access(&self) -> &AccessList {
        &self.default_access
    }

    #[must_use]
    pub fn default_publish(&self) -> &AccessList {
        &self.default_publish
    }

    #[must_use]
    pub fn default_unpublish(&self) -> &AccessList {
        &self.default_unpublish
    }
}
