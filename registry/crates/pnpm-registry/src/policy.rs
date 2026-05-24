//! Per-package access and publish rules. Mirrors the subset of
//! verdaccio's `packages:` config that `@pnpm/registry-mock` relies
//! on: a list of glob patterns, each with `access` and `publish`
//! permissions. The first matching pattern wins, with sensible
//! defaults applied when nothing matches.

use std::str::FromStr;

use wax::{Glob, Program};

use crate::error::RegistryError;

/// What identities are allowed to perform an action on a package.
/// Mirrors the verdaccio tokens with the same names. The subset is
/// minimal because `@pnpm/registry-mock` only ever uses these two —
/// `$anonymous` and per-group rules aren't needed for any pnpm test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRule {
    /// `$all` — anyone, authenticated or not.
    All,
    /// `$authenticated` — any caller carrying a valid Bearer token
    /// or Basic auth header.
    Authenticated,
}

impl FromStr for AccessRule {
    type Err = RegistryError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "$all" | "all" => Ok(AccessRule::All),
            "$authenticated" | "authenticated" => Ok(AccessRule::Authenticated),
            other => Err(RegistryError::InvalidAccessRule { value: other.to_string() }),
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
    pub access: AccessRule,
    pub publish: AccessRule,
}

impl PackagePolicy {
    pub fn new(
        pattern: &str,
        access: AccessRule,
        publish: AccessRule,
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
#[derive(Debug, Default, Clone)]
pub struct PackagePolicies {
    rules: Vec<PackagePolicy>,
}

/// Effective permissions for one package. Returned by
/// [`PackagePolicies::for_package`] with the catch-all defaults
/// applied when no rule matches: anonymous reads allowed,
/// authenticated writes required.
#[derive(Debug, Clone, Copy)]
pub struct Effective {
    pub access: AccessRule,
    pub publish: AccessRule,
}

impl PackagePolicies {
    pub fn new(rules: Vec<PackagePolicy>) -> Self {
        Self { rules }
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
            ("@private/*", AccessRule::Authenticated, AccessRule::Authenticated),
            ("@pnpm.e2e/needs-auth", AccessRule::Authenticated, AccessRule::Authenticated),
            ("**", AccessRule::All, AccessRule::Authenticated),
        ];
        let rules = rules
            .into_iter()
            .map(|(pattern, access, publish)| {
                PackagePolicy::new(pattern, access, publish)
                    .expect("registry-mock defaults compile")
            })
            .collect();
        Self::new(rules)
    }

    pub fn for_package(&self, package: &str) -> Effective {
        for rule in &self.rules {
            if rule.matches(package) {
                return Effective { access: rule.access, publish: rule.publish };
            }
        }
        // Fallback when no `**` catch-all is configured: reads open,
        // writes require auth. Same shape as the registry-mock default.
        Effective { access: AccessRule::All, publish: AccessRule::Authenticated }
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessRule, PackagePolicies};

    #[test]
    fn defaults_match_registry_mock_config() {
        let policies = PackagePolicies::registry_mock_defaults();

        let needs_auth = policies.for_package("@pnpm.e2e/needs-auth");
        assert_eq!(needs_auth.access, AccessRule::Authenticated);
        assert_eq!(needs_auth.publish, AccessRule::Authenticated);

        let private = policies.for_package("@private/foo");
        assert_eq!(private.access, AccessRule::Authenticated);

        let public = policies.for_package("@pnpm.e2e/no-deps");
        assert_eq!(public.access, AccessRule::All);
        assert_eq!(public.publish, AccessRule::Authenticated);

        let unscoped = policies.for_package("lodash");
        assert_eq!(unscoped.access, AccessRule::All);
        assert_eq!(unscoped.publish, AccessRule::Authenticated);
    }

    #[test]
    fn first_matching_rule_wins() {
        let policies = PackagePolicies::registry_mock_defaults();
        // `@private/foo` matches `@private/*` first, not the `**`
        // catch-all. The first rule's $authenticated access wins
        // over the catch-all's $all.
        assert_eq!(policies.for_package("@private/foo").access, AccessRule::Authenticated);
    }

    #[test]
    fn falls_back_to_safe_defaults_when_no_rules_match() {
        let policies = PackagePolicies::new(vec![]);
        let effective = policies.for_package("anything");
        assert_eq!(effective.access, AccessRule::All);
        assert_eq!(effective.publish, AccessRule::Authenticated);
    }
}
