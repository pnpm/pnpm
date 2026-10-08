//! Per-package access rules: each concrete registry's `packages:` map,
//! keyed by [`PackagePattern`] with `access` / `publish` / `unpublish`
//! permission lists as values. Selection is by **specificity**, not
//! declaration order — an exact name beats `@scope/*` beats `@*/*` beats
//! `**` — because a YAML mapping is formally unordered and key order must
//! not decide which access rule applies. The restricted pattern language
//! makes that selection total: for any one name, at most one key per
//! specificity tier can match, so every name has exactly one winning entry.
//!
//! Each permission is a list of tokens; a request is allowed when the
//! caller's identity satisfies any token in the list. Tokens are the
//! built-in pseudo-groups (`$all`, `$authenticated`, `$anonymous`), a
//! bare *username*, or a `team:<name>` reference to a team the owning
//! registry declares. Teams are registry-scoped — the registry is the
//! tenant, so it owns its principal sets — and a `team:` token reads the
//! registry's live [`TeamDirectory`], so evaluation needs only the caller's
//! identity.

pub use access::{AccessList, AccessToken, Identity, Membership};
pub use package_rules::{PackageRules, RuleOverride};
pub use teams::{TeamDirectory, Teams};

mod access;
mod package_rules;
mod teams;

use std::collections::BTreeMap;

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

/// One concrete registry's `packages:` map as it stands at one moment: its
/// namespace (the key set) and its per-package rules (the values), with the
/// registry-level defaults an entry's omitted fields fall back to.
/// [`PackageRules`] holds the current table.
///
/// Selection is by **specificity** — the most specific matching key wins,
/// and key order carries no meaning (see the module docs). No entry can be
/// dead: an exact key carves its name out of a scope key, which still
/// serves the rest, so there is no shadowed-entry validation inside a
/// registry; a duplicate key is the only error (rejected at config load).
#[derive(Debug, Clone)]
pub struct RuleTable {
    rules: Vec<PackageRule>,
    /// Winner lookup by specificity tier, rebuilt whenever the rule set
    /// changes: at most one key per tier can match a given name, so the
    /// most specific match resolves with map lookups instead of a scan of
    /// every rule.
    index: RuleIndex,
    /// Fallbacks for fields the winning entry omits (and for every name
    /// when the map itself is empty = the registry claims every name).
    default_access: AccessList,
    default_publish: AccessList,
    default_unpublish: AccessList,
}

/// Positions of the rules by pattern shape, mirroring the specificity
/// chain: an exact key beats the name's scope key beats `@*/*` beats `**`.
/// Duplicate keys never coexist here — YAML loading and the routing-graph
/// validation both reject them — so each slot holds the one possible rule.
#[derive(Debug, Default, Clone)]
struct RuleIndex {
    exact: BTreeMap<String, usize>,
    scopes: BTreeMap<String, usize>,
    namespaces: BTreeMap<String, usize>,
    any_scoped: Option<usize>,
    all: Option<usize>,
}

impl RuleIndex {
    fn build(rules: &[PackageRule]) -> Self {
        let mut index = Self::default();
        for (position, rule) in rules.iter().enumerate() {
            match &rule.pattern {
                PackagePattern::Exact(name) => {
                    index.exact.insert(name.clone(), position);
                }
                PackagePattern::Scope(scope) => {
                    index.scopes.insert(scope.clone(), position);
                }
                PackagePattern::Namespace(namespace) => {
                    index.namespaces.insert(namespace.clone(), position);
                }
                PackagePattern::AnyScoped => index.any_scoped = Some(position),
                PackagePattern::All => index.all = Some(position),
            }
        }
        index
    }

    /// The winning rule's position for `package`: the most specific tier
    /// with a matching key.
    fn winner(&self, package: &str) -> Option<usize> {
        if let Some(&position) = self.exact.get(package) {
            return Some(position);
        }
        // An npm scope carries a leading `@`, which no image repository name
        // may, so the two tier-two keyspaces cannot collide.
        if let Some(namespace) = PackagePattern::namespace_of(package)
            && let Some(&position) = self.namespaces.get(namespace)
        {
            return Some(position);
        }
        if let Some(scope) = PackagePattern::scope_of(package) {
            if let Some(&position) = self.scopes.get(scope) {
                return Some(position);
            }
            if let Some(position) = self.any_scoped {
                return Some(position);
            }
        }
        self.all
    }
}

impl Default for RuleTable {
    /// The safe defaults with no rules: every name claimed, reads open,
    /// publishes require auth, destructive writes denied.
    fn default() -> Self {
        Self::new(Vec::new(), None)
    }
}

/// Effective permissions for one package, borrowed from the winning
/// entry (each field falling back to the registry-level default).
#[derive(Debug, Clone, Copy)]
pub struct Effective<'a> {
    pub access: &'a AccessList,
    pub publish: &'a AccessList,
    pub unpublish: &'a AccessList,
    /// Whether `access` came from an explicit `packages:` entry rather than
    /// the registry-level default. Drives how a hosted denial answers: an
    /// explicitly gated name is declared, discoverable config and rejects
    /// loudly (401/403, so a client can prompt for auth), while a
    /// default-gated name is masked as not-found — a blanket-private
    /// registry never reveals which names exist.
    pub access_is_explicit: bool,
}

impl RuleTable {
    /// Build a registry's rules. `default_access` is the registry-level
    /// `access:` (its omission = `$all`); publish defaults to
    /// `$authenticated` and unpublish to nobody, the safe defaults.
    #[must_use]
    pub fn new(rules: Vec<PackageRule>, default_access: Option<AccessList>) -> Self {
        Self {
            index: RuleIndex::build(&rules),
            rules,
            default_access: default_access.unwrap_or_else(|| AccessList::from_tokens(["$all"])),
            default_publish: AccessList::from_tokens(["$authenticated"]),
            default_unpublish: AccessList::default(),
        }
    }

    /// Override the registry-level publish default (`$authenticated`).
    #[must_use]
    pub fn with_default_publish(mut self, publish: AccessList) -> Self {
        self.default_publish = publish;
        self
    }

    /// Override the registry-level unpublish default (nobody).
    #[must_use]
    pub fn with_default_unpublish(mut self, unpublish: AccessList) -> Self {
        self.default_unpublish = unpublish;
        self
    }

    /// Add one entry to the map. Selection stays order-free (specificity);
    /// duplicate keys are the caller's to avoid — YAML loading rejects them.
    /// For tests and embedders that build rules programmatically.
    pub fn push_rule(&mut self, rule: PackageRule) {
        self.rules.push(rule);
        self.index = RuleIndex::build(&self.rules);
    }

    /// The namespace this registry declares: the map's key set. Empty =
    /// every name. Feeds the routing graph, which enforces the claim on
    /// every path to the registry.
    #[must_use]
    pub fn patterns(&self) -> Vec<PackagePattern> {
        self.rules
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    /// Whether any rule carries the given field, i.e. the map refines that
    /// permission somewhere. Lets config validation reject `publish:` /
    /// `unpublish:` values on an upstream registry, where no write can land.
    #[must_use]
    pub fn refines_writes(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.publish.is_some() || rule.unpublish.is_some())
    }

    /// Whether any permission list, the registry defaults included, holds a
    /// `team:<team>` token.
    #[must_use]
    pub fn references_team(&self, team: &str) -> bool {
        let defaults = [&self.default_access, &self.default_publish, &self.default_unpublish];
        defaults
            .into_iter()
            .any(|list| list.references_team(team))
            || self.rules
                .iter()
                .flat_map(|rule| [&rule.access, &rule.publish, &rule.unpublish])
                .flatten()
                .any(|list| list.references_team(team))
    }

    /// Whether any package carries an explicit access policy.
    #[must_use]
    pub fn refines_access(&self) -> bool {
        self.rules.iter().any(|rule| rule.access.is_some())
    }

    /// The effective permissions for `package`: the **most specific**
    /// matching entry's fields, each falling back to the registry-level
    /// default.
    #[must_use]
    pub fn for_package(&self, package: &str) -> Effective<'_> {
        let winner = self.index
            .winner(package)
            .map(|position| &self.rules[position]);
        let explicit_access = winner.and_then(|rule| rule.access.as_ref());
        Effective {
            access: explicit_access.unwrap_or(&self.default_access),
            publish: winner.and_then(|rule| rule.publish.as_ref()).unwrap_or(&self.default_publish),
            unpublish: winner
                .and_then(|rule| rule.unpublish.as_ref())
                .unwrap_or(&self.default_unpublish),
            access_is_explicit: explicit_access.is_some(),
        }
    }

    /// The registry-level default `access:` — who may reach the registry
    /// when no per-package entry refines it. Write-path masking uses this
    /// for names the registry does not claim (there is no entry to consult).
    #[must_use]
    pub fn default_access(&self) -> &AccessList {
        &self.default_access
    }

    /// Whether *any* name this registry serves could admit `identity`: the
    /// registry-level default does, or some explicit entry's `access` does.
    /// The search scan's fast path — a caller no rule could ever admit gets
    /// the empty result without enumerating the registry's storage, so a
    /// blanket-masked registry leaks neither package names nor its size
    /// through scan timing.
    #[must_use]
    pub fn any_access_admits(&self, identity: &Identity) -> bool {
        self.default_access.allows(identity)
            || self.rules
                .iter()
                .any(|rule| {
                    rule.access
                        .as_ref()
                        .is_some_and(|access| access.allows(identity))
                })
    }

    /// Whether every package-specific access refinement and the registry
    /// default admit `identity`.
    #[must_use]
    pub fn all_access_admit(&self, identity: &Identity) -> bool {
        self.default_access.allows(identity)
            && self.rules
                .iter()
                .all(|rule| {
                    rule.access
                        .as_ref()
                        .is_none_or(|access| access.allows(identity))
                })
    }
}

#[cfg(test)]
mod tests;
