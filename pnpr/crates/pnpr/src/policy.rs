//! Per-package access rules. Mirrors verdaccio's `packages:` config:
//! a list of glob patterns, each with `access` / `publish` permission
//! lists. The first matching pattern wins.
//!
//! Each permission is a list of tokens (verdaccio's space-separated
//! groups); a request is allowed when the caller's identity satisfies
//! any token in the list. Tokens are the built-in pseudo-groups
//! (`$all`, `$authenticated`, `$anonymous`, plus their `@`/bare
//! aliases) or a *name*. A name token matches either the authenticated
//! username or any group attached to that identity. pnpr's static
//! `groups:` config adds group membership on top of htpasswd users,
//! matching verdaccio's model where access lists do not distinguish
//! usernames from group names.

use std::collections::{BTreeMap, BTreeSet};
use wax::{Glob, Program};

use crate::error::RegistryError;

/// A single token in an access list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessToken {
    /// `$all` / `@all` / `all` — anyone, authenticated or not.
    All,
    /// `$authenticated` / `@authenticated` / `authenticated` — any
    /// caller carrying valid Bearer or Basic credentials.
    Authenticated,
    /// `$anonymous` / `@anonymous` / `anonymous` — only callers
    /// *without* valid credentials.
    Anonymous,
    /// A username or group name. Matches an authenticated caller whose
    /// username or group membership equals it.
    Named(String),
}

impl From<&str> for AccessToken {
    fn from(token: &str) -> Self {
        match token {
            "$all" | "@all" | "all" => AccessToken::All,
            "$authenticated" | "@authenticated" | "authenticated" => AccessToken::Authenticated,
            "$anonymous" | "@anonymous" | "anonymous" => AccessToken::Anonymous,
            name => AccessToken::Named(name.to_string()),
        }
    }
}

/// One `access` / `publish` permission: the set of tokens that satisfy
/// it. An empty list admits no one (verdaccio's empty `unpublish:`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AccessList(Vec<AccessToken>);

impl AccessList {
    /// Build from a verdaccio space-separated permission string
    /// (e.g. `"$authenticated admin"`).
    #[must_use]
    pub fn parse(spec: &str) -> Self {
        Self::from_tokens(spec.split_whitespace())
    }

    /// Build from already-separated tokens (e.g. a YAML sequence).
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

/// Static group memberships layered onto authenticated pnpr users.
/// Values are keyed by username so resolving a caller identity stays a
/// single lookup after the bearer token has been validated.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AccessGroups {
    by_user: BTreeMap<String, BTreeSet<String>>,
}

impl AccessGroups {
    pub(crate) fn add_user_to_group(&mut self, username: String, group: String) {
        self.by_user.entry(username).or_default().insert(group);
    }

    #[must_use]
    pub fn identity_for(&self, username: String) -> Identity {
        let groups = self.by_user.get(&username).cloned().unwrap_or_default();
        Identity::User { username, groups }
    }
}

/// The resolved caller identity an [`AccessList`] is evaluated against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// No valid credentials were presented.
    Anonymous,
    /// Authenticated as `username`, with any configured static groups.
    User { username: String, groups: BTreeSet<String> },
}

impl Identity {
    #[must_use]
    pub fn user(username: impl Into<String>) -> Self {
        Self::User { username: username.into(), groups: BTreeSet::new() }
    }

    #[must_use]
    pub fn user_with_groups<User, Groups, Group>(username: User, groups: Groups) -> Self
    where
        User: Into<String>,
        Groups: IntoIterator<Item = Group>,
        Group: Into<String>,
    {
        Self::User {
            username: username.into(),
            groups: groups.into_iter().map(Into::into).collect(),
        }
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
            (AccessToken::Named(name), Identity::User { username, groups }) => {
                name == username || groups.contains(name)
            }
            _ => false,
        }
    }
}

/// One entry in the access policy list. `pattern` is a wax glob
/// compiled from the verdaccio-style key (e.g. `@private/*`,
/// `@pnpm.e2e/needs-auth`, `**`). `access` controls who can read the
/// packument and tarballs; `publish` controls who can publish or
/// change dist-tags; `unpublish` controls destructive writes.
#[derive(Debug, Clone)]
pub struct PackagePolicy {
    pattern: Glob<'static>,
    pub access: AccessList,
    pub publish: AccessList,
    pub unpublish: AccessList,
}

impl PackagePolicy {
    pub fn new(
        pattern: &str,
        access: AccessList,
        publish: AccessList,
        unpublish: AccessList,
    ) -> Result<Self, RegistryError> {
        let glob = Glob::new(pattern)
            .map_err(|err| RegistryError::InvalidPolicyPattern {
                pattern: pattern.to_string(),
                reason: err.to_string(),
            })?
            .into_owned();
        Ok(Self { pattern: glob, access, publish, unpublish })
    }

    fn matches(&self, package: &str) -> bool {
        self.pattern.is_match(package)
    }
}

/// Ordered list of [`PackagePolicy`] rules. First match wins,
/// matching verdaccio's evaluation order — order in the config is
/// significant, since the catch-all `**` is almost always last.
#[derive(Debug, Clone)]
pub struct PackagePolicies {
    rules: Vec<PackagePolicy>,
    /// Applied to packages no rule matches: reads open, writes require
    /// auth. Owned here so [`Self::for_package`] can hand back a
    /// borrow.
    default_access: AccessList,
    default_publish: AccessList,
    default_unpublish: AccessList,
}

impl Default for PackagePolicies {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Effective permissions for one package, borrowed from the matched
/// rule (or the defaults when none matched).
#[derive(Debug, Clone, Copy)]
pub struct Effective<'a> {
    pub access: &'a AccessList,
    pub publish: &'a AccessList,
    pub unpublish: &'a AccessList,
}

impl PackagePolicies {
    #[must_use]
    pub fn new(rules: Vec<PackagePolicy>) -> Self {
        Self {
            rules,
            default_access: AccessList::parse("$all"),
            default_publish: AccessList::parse("$authenticated"),
            default_unpublish: AccessList::default(),
        }
    }

    /// `@pnpm/registry-mock`'s defaults, hard-coded so an out-of-the
    /// box `Config` already enforces the same access rules verdaccio
    /// did. The relevant patterns from `registry-mock`'s `config.yaml`:
    ///
    /// * `@private/*` — authenticated access + publish + unpublish
    /// * `@pnpm.e2e/needs-auth` — authenticated access + publish + unpublish
    /// * everything else — $all access, $authenticated publish + unpublish
    #[must_use]
    pub fn registry_mock_defaults() -> Self {
        let rules = [
            ("@private/*", "$authenticated", "$authenticated", "$authenticated"),
            ("@pnpm.e2e/needs-auth", "$authenticated", "$authenticated", "$authenticated"),
            ("**", "$all", "$authenticated", "$authenticated"),
        ];
        let rules = rules
            .into_iter()
            .map(|(pattern, access, publish, unpublish)| {
                PackagePolicy::new(
                    pattern,
                    AccessList::parse(access),
                    AccessList::parse(publish),
                    AccessList::parse(unpublish),
                )
                .expect("registry-mock defaults compile")
            })
            .collect();
        Self::new(rules)
    }

    #[must_use]
    pub fn for_package(&self, package: &str) -> Effective<'_> {
        for rule in &self.rules {
            if rule.matches(package) {
                return Effective {
                    access: &rule.access,
                    publish: &rule.publish,
                    unpublish: &rule.unpublish,
                };
            }
        }
        Effective {
            access: &self.default_access,
            publish: &self.default_publish,
            unpublish: &self.default_unpublish,
        }
    }
}

#[cfg(test)]
mod tests;
