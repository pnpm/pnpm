//! Per-package access rules: each concrete registry's `packages:` map,
//! keyed by [`PackagePattern`] with `access` / `publish` / `unpublish`
//! permission lists as values. Selection is by **specificity**, not
//! declaration order (an exact name beats `@scope/*` beats `@*/*` beats
//! `**`) because a YAML mapping is formally unordered and key order must
//! not decide which access rule applies. The restricted pattern language
//! makes that selection total: for any one name, at most one key per
//! specificity tier can match, so every name has exactly one winning entry.
//!
//! Each permission is a list of tokens; a request is allowed when the
//! caller's identity satisfies any token in the list. Tokens are the
//! built-in pseudo-groups (`$all`, `$authenticated`, `$anonymous`,
//! `$owners`), a bare account ID, or a `team:<id>` reference to a team of the
//! instance. The caller's [`Identity`] lists its teams, so evaluation needs
//! only the identity.

pub use access::{
    AccessList,
    AccessToken,
    Identity,
};
pub use grants::{
    Action,
    Grant,
    GrantSpec,
};
pub use package_rules::{
    PackageRules,
    RuleOverride,
};

use grants::Grants;

mod access;
mod grants;
mod package_rules;

use pnpr_registry::PackagePattern;

/// One entry of a registry's `packages:` map: a namespace claim
/// ([`PackagePattern`] key) plus optional per-package rules. `access`
/// controls who can read the packument and tarballs; `publish` controls
/// who can publish or change dist-tags; `unpublish` controls destructive
/// writes. An omitted field falls back to the registry-level default
/// carried by the owning [`RuleTable`].
#[derive(Debug, Clone)]
pub struct PackageRule {
    pub pattern: PackagePattern,
    pub access: Option<AccessList>,
    pub publish: Option<AccessList>,
    pub unpublish: Option<AccessList>,
}

/// One registry's claimed package patterns and the grants that cover it, as
/// they stand at one moment. [`PackageRules`] holds the current table.
///
/// A caller may do an action on a package when any grant that covers the
/// package gives the action to a subject the caller matches. Without such a
/// grant the caller may not.
///
/// A table can also be built from rules in code, as an embedder that supplies
/// its routing does: each [`PackageRule`] and the registry defaults become the
/// grants that give the same under most-specific-rule-wins.
#[derive(Debug, Clone)]
pub struct RuleTable {
    rules: Vec<PackageRule>,
    default_access: AccessList,
    default_publish: AccessList,
    default_unpublish: AccessList,
    grants: Grants,
}

/// Who may do one action on one package, under the grants of a [`RuleTable`].
#[derive(Debug, Clone)]
pub struct Permission<'table> {
    table: &'table RuleTable,
    package: String,
    action: Action,
}

impl Permission<'_> {
    /// Whether `identity` has the permission without owning the package.
    #[must_use]
    pub fn allows(&self, identity: &Identity) -> bool {
        self.allows_with_context(identity, false)
    }

    /// Whether `identity` has the permission, given whether it owns the
    /// package.
    #[must_use]
    pub fn allows_with_context(&self, identity: &Identity, is_owner: bool) -> bool {
        self.table.permits(&self.package, self.action, identity, is_owner)
    }

    /// Whether every caller, signed in or not, has the permission.
    #[must_use]
    pub fn admits_every_identity(&self) -> bool {
        self.allows(&Identity::Anonymous) && self.allows(&Identity::account("", ""))
    }

    /// Whether the package's owners have the permission.
    #[must_use]
    pub fn references_owners(&self) -> bool {
        self.table.owners_may(&self.package, self.action)
    }
}

/// The permissions [`RuleTable::for_package`] gives.
#[derive(Debug, Clone)]
pub struct Effective<'table> {
    pub access: Permission<'table>,
    pub publish: Permission<'table>,
    pub unpublish: Permission<'table>,
}

impl Default for RuleTable {
    fn default() -> Self {
        Self::new(Vec::new(), None)
    }
}

impl RuleTable {
    /// A table of `rules` in which reads fall back to `default_access`
    /// (`$all` when `None`), publishing to `$owners`, and unpublishing to
    /// nobody.
    #[must_use]
    pub fn new(rules: Vec<PackageRule>, default_access: Option<AccessList>) -> Self {
        Self {
            rules,
            default_access: default_access.unwrap_or_else(|| AccessList::from_tokens(["$all"])),
            default_publish: AccessList::from_tokens(["$owners"]),
            default_unpublish: AccessList::default(),
            grants: Grants::default(),
        }
        .with_grants_of_rules()
    }

    /// A table that claims `patterns` and is covered by `grants`.
    #[must_use]
    pub fn of_grants(patterns: Vec<PackagePattern>, grants: Vec<Grant>) -> Self {
        let rules = patterns
            .into_iter()
            .map(|pattern| PackageRule { pattern, access: None, publish: None, unpublish: None })
            .collect();
        Self {
            rules,
            default_access: AccessList::from_tokens(["$all"]),
            default_publish: AccessList::from_tokens(["$owners"]),
            default_unpublish: AccessList::default(),
            grants: Grants::new(grants),
        }
    }

    fn with_grants_of_rules(mut self) -> Self {
        let defaults = grants::Defaults {
            access: &self.default_access,
            publish: &self.default_publish,
            unpublish: &self.default_unpublish,
        };
        self.grants = Grants::new(grants::grants_of_rules(&self.rules, &defaults));
        self
    }

    #[must_use]
    pub fn with_default_publish(mut self, publish: AccessList) -> Self {
        self.default_publish = publish;
        self.with_grants_of_rules()
    }

    #[must_use]
    pub fn with_default_unpublish(mut self, unpublish: AccessList) -> Self {
        self.default_unpublish = unpublish;
        self.with_grants_of_rules()
    }

    /// Append a rule. The caller guarantees the pattern does not duplicate
    /// an existing one.
    pub fn push_rule(&mut self, rule: PackageRule) {
        self.rules.push(rule);
        *self = self.clone().with_grants_of_rules();
    }

    /// The claimed patterns.
    #[must_use]
    pub fn patterns(&self) -> Vec<PackagePattern> {
        self.rules
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    /// The grants that cover the registry.
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        self.grants.all()
    }

    /// Whether `identity` may do `action` on `package`, given whether it owns
    /// the package.
    #[must_use]
    pub fn permits(
        &self,
        package: &str,
        action: Action,
        identity: &Identity,
        is_owner: bool,
    ) -> bool {
        self.grants.permits(package, action, identity, is_owner)
    }

    /// Who may do `action` on `package`.
    #[must_use]
    pub fn permission(&self, package: String, action: Action) -> Permission<'_> {
        Permission { table: self, package, action }
    }

    /// Who may read, publish and unpublish `package`.
    #[must_use]
    pub fn for_package(&self, package: &str) -> Effective<'_> {
        Effective {
            access: self.permission(package.to_owned(), Action::Read),
            publish: self.permission(package.to_owned(), Action::Publish),
            unpublish: self.permission(package.to_owned(), Action::Delete),
        }
    }

    /// Whether a grant gives `action` on `package` to its owners, so deciding
    /// for a caller the grants do not otherwise admit takes the owners.
    #[must_use]
    pub fn owners_may(&self, package: &str, action: Action) -> bool {
        self.grants.owners_may(package, action)
    }

    /// Whether `identity` may do `action` on some package.
    #[must_use]
    pub fn any_admits(&self, action: Action, identity: &Identity) -> bool {
        self.grants.any_admits(action, identity)
    }

    /// Whether a grant names the team `team`.
    #[must_use]
    pub fn references_team(&self, team: &str) -> bool {
        self.grants
            .all()
            .iter()
            .any(|grant| grant.names_team(team))
    }

    /// Whether a grant that gives `read` selects packages by pattern, so who
    /// may read differs between the registry's packages.
    #[must_use]
    pub fn refines_access(&self) -> bool {
        self.grants
            .all()
            .iter()
            .any(Grant::refines_read)
    }

    /// Whether `identity` may read some package: the registry is not hidden
    /// from it.
    #[must_use]
    pub fn any_access_admits(&self, identity: &Identity) -> bool {
        self.any_admits(Action::Read, identity)
    }

    /// Whether one grant lets `identity` read every package.
    #[must_use]
    pub fn all_access_admit(&self, identity: &Identity) -> bool {
        self.grants.all_admit(Action::Read, identity)
    }
}

#[cfg(test)]
mod tests;
