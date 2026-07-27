use super::{ConcreteKind, PackagePattern, Registries, Registry, RegistryConfigError, Resolved};

fn pattern(raw: &str) -> PackagePattern {
    PackagePattern::parse(raw).expect("pattern parses")
}

fn patterns(raws: &[&str]) -> Vec<PackagePattern> {
    raws.iter().map(|raw| pattern(raw)).collect()
}

fn hosted(raws: &[&str]) -> Registry {
    Registry::Hosted { patterns: patterns(raws) }
}

fn upstream(raws: &[&str]) -> Registry {
    Registry::Upstream { patterns: patterns(raws) }
}

fn router(sources: &[&str]) -> Registry {
    Registry::Router { sources: sources.iter().map(ToString::to_string).collect() }
}

fn registries(entries: Vec<(&str, Registry)>, default_registry: Option<&str>) -> Registries {
    let map = entries.into_iter().map(|(name, kind)| (name.to_string(), kind)).collect();
    Registries::new(map, default_registry.map(str::to_string))
}

//  PackagePattern::parse ----------------------------------------------

#[test]
fn parses_recognized_shapes() {
    assert_eq!(pattern("**"), PackagePattern::All);
    assert_eq!(pattern("@*/*"), PackagePattern::AnyScoped);
    assert_eq!(pattern("@acme/*"), PackagePattern::Scope("acme".to_string()));
    assert_eq!(pattern("@acme/foo"), PackagePattern::Exact("@acme/foo".to_string()));
    assert_eq!(pattern("foo"), PackagePattern::Exact("foo".to_string()));
}

#[test]
fn rejects_unsupported_wildcards() {
    for raw in ["", "foo*", "@acme/*/extra", "@*/foo", "*", "@acme/ba*r", "a*b"] {
        assert!(
            matches!(PackagePattern::parse(raw), Err(RegistryConfigError::InvalidPattern { .. })),
            "expected {raw:?} to be rejected",
        );
    }
}

/// A wildcard-free pattern that is not a well-formed package name can never
/// match a request, so a typo like `@acme` (meaning `@acme/*`) must be a
/// config error rather than a literal that silently keeps the scope out of
/// the registry's namespace.
#[test]
fn rejects_exact_pattern_that_is_not_a_package_name() {
    for raw in ["@acme", "@acme/", "@/foo", ".hidden", "a/b/c", "@scope/../up"] {
        assert!(
            matches!(
                PackagePattern::parse(raw),
                Err(RegistryConfigError::ExactPatternNotAName { .. }),
            ),
            "expected {raw:?} to be rejected as not a package name",
        );
    }
}

/// A `@<scope>/*` pattern whose scope request parsing (`PackageName::parse`)
/// would reject is a claim no valid package name can ever match. It must be a
/// config error rather than a dead pattern — a mistyped private-scope claim
/// that never matches would silently let the names it was meant to cover land
/// on a later (often public) router source.
#[test]
fn rejects_scope_pattern_whose_scope_is_not_a_valid_scope() {
    for raw in ["@.acme/*", "@../*", "@/*", "@a/b/*", "@a:b/*"] {
        assert!(
            matches!(
                PackagePattern::parse(raw),
                Err(RegistryConfigError::ScopePatternNotAScope { .. }),
            ),
            "expected {raw:?} to be rejected as an invalid scope",
        );
    }
}

//  PackagePattern::matches --------------------------------------------

#[test]
fn matches_by_shape() {
    assert!(pattern("**").matches("foo"));
    assert!(pattern("**").matches("@acme/foo"));

    assert!(pattern("@*/*").matches("@acme/foo"));
    assert!(!pattern("@*/*").matches("foo"));

    assert!(pattern("@acme/*").matches("@acme/foo"));
    assert!(!pattern("@acme/*").matches("@other/foo"));
    assert!(!pattern("@acme/*").matches("acme"));

    assert!(pattern("@acme/foo").matches("@acme/foo"));
    assert!(!pattern("@acme/foo").matches("@acme/bar"));
}

/// A scoped pattern requires a real `@scope/name`; a bare `@scope`, an empty
/// scope, or an empty name must not match, so such an input isn't misrouted to a
/// scoped registry.
#[test]
fn scoped_patterns_require_a_name_segment() {
    for malformed in ["@acme", "@acme/", "@/foo", "@"] {
        assert!(!pattern("@*/*").matches(malformed), "@*/* wrongly matched {malformed:?}");
        assert!(!pattern("@acme/*").matches(malformed), "@acme/* wrongly matched {malformed:?}");
    }
}

//  PackagePattern::covers ---------------------------------------------

#[test]
fn covers_relation() {
    let all = pattern("**");
    let any_scoped = pattern("@*/*");
    let scope_a = pattern("@a/*");
    let scope_b = pattern("@b/*");
    let exact_scoped = pattern("@a/foo");
    let exact_plain = pattern("foo");

    // `**` covers everything.
    for other in [&all, &any_scoped, &scope_a, &exact_scoped, &exact_plain] {
        assert!(all.covers(other), "** should cover {other}");
    }
    // Nothing but `**` covers `**`.
    assert!(!any_scoped.covers(&all));
    assert!(!scope_a.covers(&all));

    // `@*/*` covers scoped things only.
    assert!(any_scoped.covers(&scope_a));
    assert!(any_scoped.covers(&exact_scoped));
    assert!(!any_scoped.covers(&exact_plain));
    assert!(!any_scoped.covers(&all));

    // `@a/*` covers only its own scope.
    assert!(scope_a.covers(&exact_scoped));
    assert!(!scope_a.covers(&scope_b));
    assert!(!scope_a.covers(&any_scoped));

    // Exact covers only the identical exact.
    assert!(exact_scoped.covers(&pattern("@a/foo")));
    assert!(!exact_scoped.covers(&scope_a));
    assert!(!exact_plain.covers(&pattern("bar")));
}

/// `covers` must agree with `matches`: a bare `@scope` exact isn't scoped, so no
/// scoped pattern covers it (otherwise validation would report a phantom shadow).
/// `parse` rejects that shape, so build the `Exact` directly to pin the
/// enum-level consistency.
#[test]
fn covers_agrees_with_matches_on_malformed_scoped_exacts() {
    let bare = PackagePattern::Exact("@acme".to_string()); // which `@*/*` doesn't match.
    assert!(!pattern("@*/*").covers(&bare));
    assert!(!pattern("@acme/*").covers(&bare));
    // A well-formed scoped exact is still covered.
    assert!(pattern("@*/*").covers(&pattern("@acme/foo")));
    assert!(pattern("@acme/*").covers(&pattern("@acme/foo")));
}

//  resolution ---------------------------------------------------------

#[test]
fn pattern_less_concrete_registry_resolves_to_itself_for_any_name() {
    let registry = registries(vec![("acme", hosted(&[])), ("npmjs", upstream(&[]))], None);
    assert_eq!(
        registry.resolve("acme", "@acme/foo"),
        Resolved::Concrete { registry: "acme", kind: ConcreteKind::Hosted },
    );
    assert_eq!(
        registry.resolve("npmjs", "react"),
        Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
    );
}

/// The namespace is enforced on the registry itself: addressing a concrete registry
/// directly (`/~<name>/`) with a name outside its declared patterns is a
/// definitive unclaimed, before storage or the upstream would be consulted.
#[test]
fn concrete_registry_does_not_resolve_an_unclaimed_name() {
    let registry =
        registries(vec![("acme", hosted(&["@acme/*"])), ("corp", upstream(&["@corp/*"]))], None);
    assert_eq!(
        registry.resolve("acme", "@acme/foo"),
        Resolved::Concrete { registry: "acme", kind: ConcreteKind::Hosted },
    );
    assert_eq!(registry.resolve("acme", "@typo/foo"), Resolved::Unclaimed);
    // A private upstream's bound: a public name can't be pulled through it.
    assert_eq!(
        registry.resolve("corp", "@corp/foo"),
        Resolved::Concrete { registry: "corp", kind: ConcreteKind::Upstream },
    );
    assert_eq!(registry.resolve("corp", "lodash"), Resolved::Unclaimed);
}

#[test]
fn unknown_registry_resolves_to_unknown() {
    let registry = registries(vec![("npmjs", upstream(&[]))], None);
    assert_eq!(registry.resolve("nope", "react"), Resolved::UnknownRegistry);
}

#[test]
fn router_resolves_first_claiming_source_authoritatively() {
    let registry = registries(
        vec![
            ("acme", hosted(&["@acme/*"])),
            ("corp", upstream(&["@corp/*"])),
            ("npmjs", upstream(&[])),
            ("main", router(&["acme", "corp", "npmjs"])),
        ],
        Some("main"),
    );

    assert_eq!(
        registry.resolve("main", "@acme/foo"),
        Resolved::Concrete { registry: "acme", kind: ConcreteKind::Hosted },
    );
    assert_eq!(
        registry.resolve("main", "@corp/foo"),
        Resolved::Concrete { registry: "corp", kind: ConcreteKind::Upstream },
    );
    assert_eq!(
        registry.resolve("main", "lodash"),
        Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn router_earlier_source_wins_over_later_catch_all() {
    // `@acme/*` is private (hosted); the pattern-less catch-all must never be
    // consulted for an `@acme/*` name even though it would also claim it.
    let registry = registries(
        vec![
            ("acme", hosted(&["@acme/*"])),
            ("npmjs", upstream(&[])),
            ("main", router(&["acme", "npmjs"])),
        ],
        None,
    );
    assert_eq!(
        registry.resolve("main", "@acme/secret"),
        Resolved::Concrete { registry: "acme", kind: ConcreteKind::Hosted },
    );
}

#[test]
fn router_with_no_claiming_source_is_unclaimed_not_fallthrough() {
    let registry =
        registries(vec![("acme", hosted(&["@acme/*"])), ("main", router(&["acme"]))], None);
    assert_eq!(registry.resolve("main", "lodash"), Resolved::Unclaimed);
}

#[test]
fn resolve_default_uses_default_registry() {
    let registry =
        registries(vec![("npmjs", upstream(&[])), ("main", router(&["npmjs"]))], Some("main"));
    assert_eq!(
        registry.resolve_default("react"),
        Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn resolve_default_without_target_is_unknown() {
    let registry = registries(vec![("npmjs", upstream(&[]))], None);
    assert_eq!(registry.resolve_default("react"), Resolved::UnknownRegistry);
}

//  validation ---------------------------------------------------------

#[test]
fn valid_config_passes() {
    let registry = registries(
        vec![
            ("acme", hosted(&["@acme/*"])),
            ("corp", upstream(&["@corp/*"])),
            ("npmjs", upstream(&[])),
            ("main", router(&["acme", "corp", "npmjs"])),
        ],
        Some("main"),
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn rejects_undefined_default_registry() {
    let registry = registries(vec![("npmjs", upstream(&[]))], Some("ghost"));
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::UndefinedDefaultRegistry { target: "ghost".to_string() }),
    );
}

#[test]
fn rejects_pattern_less_source_before_narrower_source() {
    // The dangerous common mistake: the catch-all listed first shadows a later
    // private scope.
    let registry = registries(
        vec![
            ("acme", hosted(&["@acme/*"])),
            ("npmjs", upstream(&[])),
            ("main", router(&["npmjs", "acme"])),
        ],
        None,
    );
    assert!(matches!(
        registry.validate(),
        Err(RegistryConfigError::UnreachableSource { index: 1, .. }),
    ));
}

#[test]
fn rejects_source_shadowed_by_broader_scope() {
    let registry = registries(
        vec![
            ("other", upstream(&["@*/*"])),
            ("acme", hosted(&["@acme/*"])),
            ("main", router(&["other", "acme"])),
        ],
        None,
    );
    assert!(matches!(
        registry.validate(),
        Err(RegistryConfigError::UnreachableSource { index: 1, .. }),
    ));
}

#[test]
fn allows_narrower_source_before_broader() {
    // `@acme/foo` exact before `@acme/*` is fine — the exact claims strictly
    // less, so the scope source is still reachable for every other name.
    let registry = registries(
        vec![
            ("a", hosted(&["@acme/foo"])),
            ("b", upstream(&["@acme/*"])),
            ("main", router(&["a", "b"])),
        ],
        None,
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn allows_sibling_scopes_before_any_scoped() {
    // `@a/*` and `@b/*` do not (and cannot) cover `@*/*`, so the broad source
    // stays reachable.
    let registry = registries(
        vec![
            ("a", hosted(&["@a/*"])),
            ("b", hosted(&["@b/*"])),
            ("rest", upstream(&["@*/*"])),
            ("main", router(&["a", "b", "rest"])),
        ],
        None,
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn rejects_partially_shadowed_source() {
    // The later source stays reachable through `plainpkg` (which `@*/*` cannot
    // cover), but its `@secret/foo` claim is swallowed by the earlier `@*/*`
    // source. That one pattern must be rejected by name — otherwise requests
    // for `@secret/foo` silently go to the public upstream instead of the
    // private registry, and the whole-source unreachability check never fires.
    let registry = registries(
        vec![
            ("public", upstream(&["@*/*"])),
            ("private", hosted(&["@secret/foo", "plainpkg"])),
            ("main", router(&["public", "private"])),
        ],
        None,
    );
    assert!(matches!(
        registry.validate(),
        Err(RegistryConfigError::ShadowedPattern { pattern, .. }) if pattern == "@secret/foo",
    ));
}

/// Two registries claiming the same pattern cannot be ordered: whichever is listed
/// later never receives the name. That's ambiguous provenance the operator has
/// to resolve in the declared namespaces, not by source order.
#[test]
fn rejects_identical_claim_across_two_sources() {
    let registry = registries(
        vec![
            ("a", hosted(&["@a/foo"])),
            ("b", hosted(&["@a/foo", "@b/*"])),
            ("main", router(&["a", "b"])),
        ],
        None,
    );
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::ShadowedPattern {
            router: "main".to_string(),
            source: "b".to_string(),
            pattern: "@a/foo".to_string(),
            by: "@a/foo".to_string(),
        }),
    );
}

/// Namespaces that overlap in both directions can't be saved by reordering —
/// either order leaves one source's claim dead, so both orders are rejected.
#[test]
fn rejects_bidirectionally_overlapping_sources_in_either_order() {
    let entries = |sources: &[&str]| {
        vec![
            ("a", hosted(&["@x/*", "@y/foo"])),
            ("b", hosted(&["@y/*", "@x/foo"])),
            ("main", router(sources)),
        ]
    };
    assert!(matches!(
        registries(entries(&["a", "b"]), None).validate(),
        Err(RegistryConfigError::ShadowedPattern { .. }),
    ));
    assert!(matches!(
        registries(entries(&["b", "a"]), None).validate(),
        Err(RegistryConfigError::ShadowedPattern { .. }),
    ));
}

/// A registry may declare internally redundant patterns (`@acme/*` plus an exact
/// `@acme/foo`); the union is the namespace, and using the registry in a router
/// must not report the registry as shadowing itself.
#[test]
fn allows_a_source_whose_own_patterns_overlap() {
    let registry = registries(
        vec![
            ("a", hosted(&["@acme/*", "@acme/foo"])),
            ("npmjs", upstream(&[])),
            ("main", router(&["a", "npmjs"])),
        ],
        None,
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn rejects_duplicate_pattern_within_one_registry() {
    let registry = registries(vec![("a", hosted(&["@acme/*", "@acme/*"]))], None);
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::DuplicatePattern {
            registry: "a".to_string(),
            pattern: "@acme/*".to_string(),
        }),
    );
}

#[test]
fn rejects_duplicate_source() {
    let registry = registries(
        vec![("npmjs", upstream(&["@a/*"])), ("main", router(&["npmjs", "npmjs"]))],
        None,
    );
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::DuplicateSource {
            router: "main".to_string(),
            source: "npmjs".to_string(),
        }),
    );
}

#[test]
fn rejects_unknown_source() {
    let registry = registries(vec![("main", router(&["ghost"]))], None);
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::UnknownSource {
            router: "main".to_string(),
            source: "ghost".to_string(),
        }),
    );
}

#[test]
fn rejects_self_referential_router() {
    let registry = registries(vec![("main", router(&["main"]))], None);
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::SelfReferentialRouter { router: "main".to_string() }),
    );
}

#[test]
fn rejects_router_targeting_another_router() {
    let registry = registries(
        vec![
            ("npmjs", upstream(&[])),
            ("inner", router(&["npmjs"])),
            ("outer", router(&["inner"])),
        ],
        None,
    );
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::NonConcreteSource {
            router: "outer".to_string(),
            source: "inner".to_string(),
        }),
    );
}

#[test]
fn rejects_router_with_no_sources() {
    // An empty router can never serve any package; it is only ever a config
    // mistake, so validation rejects it.
    let registry = registries(vec![("main", router(&[]))], None);
    assert_eq!(
        registry.validate(),
        Err(RegistryConfigError::EmptyRouter { router: "main".to_string() }),
    );
}
