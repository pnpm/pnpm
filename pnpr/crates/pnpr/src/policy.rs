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
    pub fn allows(&self, identity: &Identity) -> bool {
        self.0.iter().any(|token| identity.satisfies(token))
    }

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
mod tests {
    use super::{AccessList, AccessToken, Identity, PackagePolicies};

    fn user(name: &str) -> Identity {
        Identity::User { username: name.to_string() }
    }

    #[test]
    fn token_parsing_maps_builtins_and_names() {
        assert_eq!(AccessToken::from("$all"), AccessToken::All);
        assert_eq!(AccessToken::from("@all"), AccessToken::All);
        assert_eq!(AccessToken::from("all"), AccessToken::All);
        assert_eq!(AccessToken::from("$authenticated"), AccessToken::Authenticated);
        assert_eq!(AccessToken::from("$anonymous"), AccessToken::Anonymous);
        assert_eq!(AccessToken::from("@anonymous"), AccessToken::Anonymous);
        // Anything else is a username / group name (no longer an error).
        assert_eq!(AccessToken::from("admin"), AccessToken::Named("admin".to_string()));
    }

    #[test]
    fn all_admits_everyone() {
        let list = AccessList::parse("$all");
        assert!(list.allows(&Identity::Anonymous));
        assert!(list.allows(&user("alice")));
    }

    #[test]
    fn authenticated_admits_only_logged_in() {
        let list = AccessList::parse("$authenticated");
        assert!(!list.allows(&Identity::Anonymous));
        assert!(list.allows(&user("alice")));
    }

    #[test]
    fn anonymous_admits_only_logged_out() {
        let list = AccessList::parse("$anonymous");
        assert!(list.allows(&Identity::Anonymous));
        assert!(!list.allows(&user("alice")));
    }

    #[test]
    fn usernames_grant_per_user_access() {
        // verdaccio's per-user access: list the usernames directly.
        let list = AccessList::parse("alice bob");
        assert!(list.allows(&user("alice")));
        assert!(list.allows(&user("bob")));
        assert!(!list.allows(&user("carol")));
        assert!(!list.allows(&Identity::Anonymous));
    }

    #[test]
    fn mixed_token_list_is_a_union() {
        // `$authenticated admin` — any logged-in user OR (redundantly)
        // the `admin` name; satisfied by any authenticated caller.
        let list = AccessList::parse("$authenticated admin");
        assert!(list.allows(&user("carol")));
        assert!(!list.allows(&Identity::Anonymous));
    }

    #[test]
    fn empty_list_admits_no_one() {
        let list = AccessList::parse("");
        assert!(list.is_empty());
        assert!(!list.allows(&Identity::Anonymous));
        assert!(!list.allows(&user("alice")));
    }

    #[test]
    fn defaults_match_registry_mock_config() {
        let policies = PackagePolicies::registry_mock_defaults();

        let needs_auth = policies.for_package("@pnpm.e2e/needs-auth");
        assert!(!needs_auth.access.allows(&Identity::Anonymous));
        assert!(needs_auth.access.allows(&user("alice")));

        let private = policies.for_package("@private/foo");
        assert!(!private.access.allows(&Identity::Anonymous));

        let public = policies.for_package("@pnpm.e2e/no-deps");
        assert!(public.access.allows(&Identity::Anonymous));
        assert!(!public.publish.allows(&Identity::Anonymous));
        assert!(public.publish.allows(&user("alice")));
    }

    #[test]
    fn first_matching_rule_wins() {
        let policies = PackagePolicies::registry_mock_defaults();
        // `@private/foo` matches `@private/*` first, not the `**`
        // catch-all: anonymous reads are denied.
        assert!(!policies.for_package("@private/foo").access.allows(&Identity::Anonymous));
    }

    #[test]
    fn falls_back_to_safe_defaults_when_no_rules_match() {
        let policies = PackagePolicies::new(vec![]);
        let effective = policies.for_package("anything");
        assert!(effective.access.allows(&Identity::Anonymous));
        assert!(!effective.publish.allows(&Identity::Anonymous));
        assert!(effective.publish.allows(&user("alice")));
    }
}
