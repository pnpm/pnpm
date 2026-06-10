//! Per-package access rules. Mirrors verdaccio's `packages:` config:
//! a list of glob patterns, each with `access` / `publish` permission
//! lists. The first matching pattern wins.
//!
//! Each permission is a list of tokens (verdaccio's space-separated
//! groups); a request is allowed when the caller's identity satisfies
//! any token in the list. Tokens are the built-in pseudo-groups
//! (`$all`, `$authenticated`, `$anonymous`, plus their `@`/bare
//! aliases) or a *name*. With htpasswd auth a caller's only group is
//! their own username, so a name token matches an authenticated caller
//! whose username equals it — group names supplied by other auth
//! backends would match here too once such a backend lands. This is
//! verdaccio's model, where the auth plugin owns group membership and
//! the htpasswd plugin contributes only the username.

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
    /// username (or, eventually, auth-provided group) equals it.
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

/// The resolved caller identity an [`AccessList`] is evaluated against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// No valid credentials were presented.
    Anonymous,
    /// Authenticated as `username`. (With htpasswd that's the caller's
    /// only group beyond the built-ins; a group-providing auth backend
    /// would widen this.)
    User { username: String },
}

impl Identity {
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Identity::User { .. })
    }

    fn satisfies(&self, token: &AccessToken) -> bool {
        match (token, self) {
            (AccessToken::All, _) => true,
            (AccessToken::Authenticated, Identity::User { .. }) => true,
            (AccessToken::Anonymous, Identity::Anonymous) => true,
            (AccessToken::Named(name), Identity::User { username }) => name == username,
            _ => false,
        }
    }
}

/// One entry in the access policy list. `pattern` is a wax glob
/// compiled from the verdaccio-style key (e.g. `@private/*`,
/// `@pnpm.e2e/needs-auth`, `**`). `access` controls who can read the
/// packument and tarballs; `publish` controls who can publish or
/// change dist-tags.
#[derive(Debug, Clone)]
pub struct PackagePolicy {
    pattern: Glob<'static>,
    pub access: AccessList,
    pub publish: AccessList,
}

impl PackagePolicy {
    pub fn new(
        pattern: &str,
        access: AccessList,
        publish: AccessList,
    ) -> Result<Self, RegistryError> {
        let glob = Glob::new(pattern)
            .map_err(|err| RegistryError::InvalidPolicyPattern {
                pattern: pattern.to_string(),
                reason: err.to_string(),
            })?
            .into_owned();
        Ok(Self { pattern: glob, access, publish })
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
}

impl PackagePolicies {
    #[must_use]
    pub fn new(rules: Vec<PackagePolicy>) -> Self {
        Self {
            rules,
            default_access: AccessList::parse("$all"),
            default_publish: AccessList::parse("$authenticated"),
        }
    }

    /// `@pnpm/registry-mock`'s defaults, hard-coded so an out-of-the
    /// box `Config` already enforces the same access rules verdaccio
    /// did. The relevant patterns from `registry-mock`'s `config.yaml`:
    ///
    /// * `@private/*` — authenticated access + publish
    /// * `@pnpm.e2e/needs-auth` — authenticated access + publish
    /// * everything else — $all access, $authenticated publish
    #[must_use]
    pub fn registry_mock_defaults() -> Self {
        let rules = [
            ("@private/*", "$authenticated", "$authenticated"),
            ("@pnpm.e2e/needs-auth", "$authenticated", "$authenticated"),
            ("**", "$all", "$authenticated"),
        ];
        let rules = rules
            .into_iter()
            .map(|(pattern, access, publish)| {
                PackagePolicy::new(pattern, AccessList::parse(access), AccessList::parse(publish))
                    .expect("registry-mock defaults compile")
            })
            .collect();
        Self::new(rules)
    }

    #[must_use]
    pub fn for_package(&self, package: &str) -> Effective<'_> {
        for rule in &self.rules {
            if rule.matches(package) {
                return Effective { access: &rule.access, publish: &rule.publish };
            }
        }
        Effective { access: &self.default_access, publish: &self.default_publish }
    }
}

#[cfg(test)]
mod tests;
