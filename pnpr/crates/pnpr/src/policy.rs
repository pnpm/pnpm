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
//! tenant, so it owns its principal sets — and a `team:` token is
//! resolved to the declared member set when the config is loaded, so
//! evaluation needs only the caller's identity.

use std::collections::{BTreeMap, BTreeSet};

use crate::registry::PackagePattern;

/// A single token in an access list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessToken {
    /// `$all` — anyone, authenticated or not.
    All,
    /// `$authenticated` — any caller carrying valid Bearer or Basic
    /// credentials.
    Authenticated,
    /// `$anonymous` — only callers *without* valid credentials.
    Anonymous,
    /// A bare token: a username. Matches an authenticated caller whose
    /// username equals it — never a team name; teams are referenced with
    /// the explicit `team:` form.
    User(String),
    /// A `team:<name>` reference, resolved to the owning registry's
    /// declared member set at config load (an undeclared team is a config
    /// error there). Matches an authenticated caller whose username is a
    /// member. `name` is kept for diagnostics only.
    Team { name: String, members: BTreeSet<String> },
}

/// Only the `$`-sigiled spellings are built-ins; any other token is a
/// username, which can only *narrow* access, so this stays infallible.
/// Near-miss spellings (verdaccio's `@all`/bare aliases, an unknown
/// `$...`) and `team:` references are handled at YAML load (`AccessSpec`
/// in the config module): the former are rejected, the latter resolved
/// against the registry's declared teams. A programmatic caller passing
/// one of those spellings just gets a username that matches nobody.
impl From<&str> for AccessToken {
    fn from(token: &str) -> Self {
        match token {
            "$all" => AccessToken::All,
            "$authenticated" => AccessToken::Authenticated,
            "$anonymous" => AccessToken::Anonymous,
            name => AccessToken::User(name.to_string()),
        }
    }
}

/// One `access` / `publish` permission: the set of tokens that satisfy
/// it. An empty list admits no one (an explicit `unpublish: []`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AccessList(Vec<AccessToken>);

impl AccessList {
    /// Build from already-resolved tokens (the config loader's path,
    /// where `team:` references have been resolved to member sets).
    pub(crate) fn new(tokens: Vec<AccessToken>) -> Self {
        Self(tokens)
    }

    /// Build from individual built-in or username tokens (e.g. the
    /// elements of a YAML sequence). Each string is one token, taken
    /// verbatim; `team:` references cannot be built this way — they need
    /// the owning registry's team declarations (see [`Self::new`]).
    pub fn from_tokens<Tokens, Token>(tokens: Tokens) -> Self
    where
        Tokens: IntoIterator<Item = Token>,
        Token: AsRef<str>,
    {
        Self(tokens.into_iter().map(|token| AccessToken::from(token.as_ref())).collect())
    }

    /// Whether `identity` satisfies any token in the list.
    #[must_use]
    pub fn allows(&self, identity: &Identity) -> bool {
        self.0.iter().any(|token| identity.satisfies(token))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The resolved caller identity an [`AccessList`] is evaluated against.
/// Just the authenticated username (or its absence): team membership
/// lives in the [`AccessToken::Team`] tokens, resolved at config load,
/// so identity carries no memberships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// No valid credentials were presented.
    Anonymous,
    /// Authenticated as `username`.
    User { username: String },
}

impl Identity {
    #[must_use]
    pub fn user(username: impl Into<String>) -> Self {
        Self::User { username: username.into() }
    }

    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Identity::User { .. })
    }

    fn satisfies(&self, token: &AccessToken) -> bool {
        match (token, self) {
            (AccessToken::All, _) => true,
            (AccessToken::Authenticated, Identity::User { .. }) => true,
            (AccessToken::Anonymous, Identity::Anonymous) => true,
            (AccessToken::User(name), Identity::User { username }) => name == username,
            (AccessToken::Team { members, .. }, Identity::User { username }) => {
                members.contains(username)
            }
            _ => false,
        }
    }
}

/// One entry of a registry's `packages:` map: a namespace claim
/// ([`PackagePattern`] key) plus optional per-package rules. `access`
/// controls who can read the packument and tarballs; `publish` controls
/// who can publish or change dist-tags; `unpublish` controls destructive
/// writes. An omitted field falls back to the registry-level default
/// carried by the owning [`PackageRules`].
#[derive(Debug, Clone)]
pub struct PackageRule {
    pub pattern: PackagePattern,
    pub access: Option<AccessList>,
    pub publish: Option<AccessList>,
    pub unpublish: Option<AccessList>,
}

/// One concrete registry's `packages:` map: its namespace (the key set)
/// and its per-package rules (the values), with the registry-level
/// defaults an entry's omitted fields fall back to.
///
/// Selection is by **specificity** — the most specific matching key wins,
/// and key order carries no meaning (see the module docs). No entry can be
/// dead: an exact key carves its name out of a scope key, which still
/// serves the rest, so there is no shadowed-entry validation inside a
/// registry; a duplicate key is the only error (rejected at config load).
#[derive(Debug, Clone)]
pub struct PackageRules {
    rules: Vec<PackageRule>,
    /// Winner lookup by specificity tier, rebuilt whenever the rule set
    /// changes: at most one key per tier can match a given name, so the
    /// most specific match resolves with map lookups instead of a scan of
    /// every rule — `for_package` runs on every read, write, search hit,
    /// and route classification.
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

impl Default for PackageRules {
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

impl PackageRules {
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

    /// Override the registry-level unpublish default (nobody). Used by the
    /// programmatic registry-mock constructors, whose fixtures exercise
    /// unpublish flows with any authenticated user.
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
        self.rules.iter().map(|rule| rule.pattern.clone()).collect()
    }

    /// Whether any rule carries the given field, i.e. the map refines that
    /// permission somewhere. Lets config validation reject `publish:` /
    /// `unpublish:` values on an upstream registry, where no write can land.
    #[must_use]
    pub fn refines_writes(&self) -> bool {
        self.rules.iter().any(|rule| rule.publish.is_some() || rule.unpublish.is_some())
    }

    /// The effective permissions for `package`: the **most specific**
    /// matching entry's fields, each falling back to the registry-level
    /// default. Selection is order-free — the restricted pattern language
    /// guarantees at most one matching key per specificity tier, so the
    /// winner is unique regardless of where it appears in the map — and
    /// indexed, so it costs tier lookups rather than a scan of every rule.
    #[must_use]
    pub fn for_package(&self, package: &str) -> Effective<'_> {
        let winner = self.index.winner(package).map(|position| &self.rules[position]);
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
            || self
                .rules
                .iter()
                .any(|rule| rule.access.as_ref().is_some_and(|access| access.allows(identity)))
    }
}

#[cfg(test)]
mod tests;
