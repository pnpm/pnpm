use super::{CanonicalPackageName, Ecosystem, RegistryConfigError, fmt};

/// A package-name pattern: one member of a concrete registry's declared
/// namespace.
///
/// Deliberately a small, **decidable** language so [`Self::covers`] can decide
/// statically whether one pattern matches a superset of another — the property
/// [`crate::Registries::validate`] relies on to detect shadowed sources. A general glob
/// (`wax`) would make coverage undecidable, so registry patterns are restricted
/// to these four shapes; an unrecognized wildcard is a parse error rather than
/// a silently-narrowing literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackagePattern {
    /// `**` — every package name.
    All,
    /// `@*/*` — every scoped package, any scope.
    AnyScoped,
    /// `@<scope>/*` — every package in one scope. Stores the scope without its
    /// leading `@`.
    Scope(String),
    /// `<namespace>/*` — every image repository under one leading path
    /// component. One component only, so two namespace patterns are either
    /// equal or disjoint and the specificity chain stays strict.
    Namespace(String),
    /// A literal package name (`foo`, `@scope/foo`, or `namespace/image`).
    Exact(String),
}

impl PackagePattern {
    /// Parse a registry pattern. `**` claims everything in any ecosystem; the
    /// wildcard shape below it is the one that ecosystem's names can carry,
    /// and everything else must be a well-formed name.
    ///
    /// An unsupported glob, or a typo like `@acme` meaning `@acme/*`, fails
    /// loudly rather than being read as a literal name that silently never
    /// matches and lets the scope land on a later router source.
    pub fn parse(pattern: &str, ecosystem: Ecosystem) -> Result<Self, RegistryConfigError> {
        if pattern.is_empty() {
            return Err(invalid_pattern(pattern, ecosystem));
        }
        if pattern == "**" {
            return Ok(PackagePattern::All);
        }
        match ecosystem {
            Ecosystem::Npm => Self::parse_scoped_pattern(pattern),
            Ecosystem::Oci => Self::parse_image_pattern(pattern),
            // A crate or project name is one flat token with no namespace in
            // it, so an exact name is the only claim below `**`. A scoped
            // shape here would be a claim no name of this ecosystem could
            // ever match.
            Ecosystem::Cargo | Ecosystem::Pypi => Self::parse_exact(pattern, ecosystem),
        }
    }

    /// The npm shapes: `@*/*` for every scoped name, `@<scope>/*` for one
    /// scope, or a literal name.
    pub(super) fn parse_scoped_pattern(pattern: &str) -> Result<Self, RegistryConfigError> {
        if pattern == "@*/*" {
            return Ok(PackagePattern::AnyScoped);
        }
        if let Some(scope) = pattern.strip_prefix('@').and_then(|rest| rest.strip_suffix("/*")) {
            // A wildcard inside the scope is an unsupported glob; a scope
            // that request parsing would reject — `@.acme`, `@..`, a
            // separator — is a claim no valid package name can ever match.
            if scope.contains('*') {
                return Err(invalid_pattern(pattern, Ecosystem::Npm));
            }
            if !pnpr_package_name::is_safe_path_segment(scope) {
                return Err(RegistryConfigError::ScopePatternNotAScope {
                    pattern: pattern.to_string(),
                });
            }
            return Ok(PackagePattern::Scope(scope.to_string()));
        }
        Self::parse_exact(pattern, Ecosystem::Npm)
    }

    /// The image-repository shapes: `<namespace>/*` for one leading path
    /// component, or a literal repository name. An image name carries no `@`,
    /// so npm's scoped shapes are not reachable here.
    pub(super) fn parse_image_pattern(pattern: &str) -> Result<Self, RegistryConfigError> {
        let Some(namespace) = pattern.strip_suffix("/*") else {
            return Self::parse_exact(pattern, Ecosystem::Oci);
        };
        // One component only, so two namespace patterns are either equal or
        // disjoint and the specificity chain below stays strict.
        if namespace.contains('*') || namespace.contains('/') {
            return Err(invalid_pattern(pattern, Ecosystem::Oci));
        }
        pnpr_package_name::canonicalize_oci_name(namespace).map(PackagePattern::Namespace).map_err(
            |_| RegistryConfigError::NamespacePatternNotANamespace { pattern: pattern.to_string() },
        )
    }

    /// A literal name, canonicalized the way a request for it will be.
    pub(super) fn parse_exact(
        pattern: &str,
        ecosystem: Ecosystem,
    ) -> Result<Self, RegistryConfigError> {
        if pattern.contains('*') {
            return Err(invalid_pattern(pattern, ecosystem));
        }
        CanonicalPackageName::parse(pattern, ecosystem)
            .map(|name| PackagePattern::Exact(name.as_str().to_string()))
            .map_err(|_| RegistryConfigError::ExactPatternNotAName { pattern: pattern.to_string() })
    }

    /// How specific this pattern is: an exact name beats `@scope/*` beats
    /// `@*/*` beats `**`. For any one package name, the patterns that can
    /// match it form a strict chain — at most one per tier can exist in a
    /// duplicate-free set — so most-specific-match selection is total and
    /// order-free. That is what lets a registry's `packages:` map be a YAML
    /// mapping whose key order carries no meaning.
    #[must_use]
    pub fn specificity(&self) -> u8 {
        match self {
            PackagePattern::All => 0,
            PackagePattern::AnyScoped => 1,
            PackagePattern::Scope(_) | PackagePattern::Namespace(_) => 2,
            PackagePattern::Exact(_) => 3,
        }
    }

    /// The scope of a well-formed scoped package name (`@acme/foo` →
    /// `acme`), without its leading `@`; `None` for an unscoped or
    /// malformed name. The scope-tier key a specificity lookup consults.
    #[must_use]
    pub fn scope_of(package: &str) -> Option<&str> {
        scoped_name(package).map(|(scope, _)| scope)
    }

    /// The leading path component of a multi-component image repository name
    /// (`acme/app` → `acme`); `None` for a single-component name. The
    /// namespace-tier key a specificity lookup consults.
    #[must_use]
    pub fn namespace_of(package: &str) -> Option<&str> {
        package.split_once('/').map(|(namespace, _)| namespace)
    }

    /// Whether this pattern matches `package`.
    #[must_use]
    pub fn matches(&self, package: &str) -> bool {
        match self {
            PackagePattern::All => true,
            // A scoped pattern matches only a well-formed `@scope/name`, never a
            // bare `@scope` with no name segment.
            PackagePattern::AnyScoped => scoped_name(package).is_some(),
            PackagePattern::Scope(scope) => {
                scoped_name(package).is_some_and(|(package_scope, _)| package_scope == scope)
            }
            PackagePattern::Namespace(namespace) => {
                Self::namespace_of(package).is_some_and(|leading| leading == namespace)
            }
            PackagePattern::Exact(name) => name == package,
        }
    }

    /// Whether this pattern matches every package the `other` pattern matches
    /// (i.e. `self` ⊇ `other`). Decides source shadowing in [`crate::Registries::validate`].
    ///
    /// A union of earlier patterns can only shadow `other` through a single
    /// member: the one unbounded case — many `@<scope>/*` covering `@*/*` —
    /// would need every scope enumerated, which is impossible, so per-pattern
    /// coverage is sufficient for union coverage in this language.
    #[must_use]
    pub fn covers(&self, other: &PackagePattern) -> bool {
        use PackagePattern::{All, AnyScoped, Exact, Namespace, Scope};
        match self {
            All => true,
            AnyScoped => match other {
                All | Namespace(_) => false,
                AnyScoped | Scope(_) => true,
                // Consistent with `matches`: an exact name is scoped only when
                // it is a well-formed `@scope/name`, never a bare `@scope`.
                Exact(name) => scoped_name(name).is_some(),
            },
            Scope(scope) => match other {
                Scope(other_scope) => other_scope == scope,
                Exact(name) => scoped_name(name).is_some_and(|(name_scope, _)| name_scope == scope),
                All | AnyScoped | Namespace(_) => false,
            },
            Namespace(namespace) => match other {
                Namespace(other_namespace) => other_namespace == namespace,
                Exact(name) => Self::namespace_of(name).is_some_and(|leading| leading == namespace),
                All | AnyScoped | Scope(_) => false,
            },
            Exact(name) => matches!(other, Exact(other_name) if other_name == name),
        }
    }
}

impl fmt::Display for PackagePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackagePattern::All => f.write_str("**"),
            PackagePattern::AnyScoped => f.write_str("@*/*"),
            PackagePattern::Scope(scope) => write!(f, "@{scope}/*"),
            PackagePattern::Namespace(namespace) => write!(f, "{namespace}/*"),
            PackagePattern::Exact(name) => f.write_str(name),
        }
    }
}

/// The `(scope, name)` of a well-formed scoped package (`@acme/foo` →
/// `("acme", "foo")`), or `None` when it is unscoped or missing either segment
/// (`@acme`, `@/foo`, `@acme/`).
pub(super) fn scoped_name(package: &str) -> Option<(&str, &str)> {
    let (scope, name) = package.strip_prefix('@')?.split_once('/')?;
    (!scope.is_empty() && !name.is_empty()).then_some((scope, name))
}

/// The wildcard shapes an ecosystem's names can carry, for the message an
/// operator reads when theirs is not one of them.
pub(super) fn wildcard_shapes(ecosystem: Ecosystem) -> &'static str {
    match ecosystem {
        Ecosystem::Npm => "`@scope/*` or `@*/*`",
        Ecosystem::Oci => "`<namespace>/*`",
        // A crate or project name is one flat token, so there is no namespace
        // to claim below `**`.
        Ecosystem::Cargo | Ecosystem::Pypi => "nothing narrower",
    }
}

pub(super) fn invalid_pattern(pattern: &str, ecosystem: Ecosystem) -> RegistryConfigError {
    RegistryConfigError::InvalidPattern { pattern: pattern.to_string(), ecosystem }
}
