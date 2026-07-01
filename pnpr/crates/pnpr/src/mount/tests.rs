use super::{ConcreteKind, MountConfigError, MountKind, Mounts, PackagePattern, Resolved, Route};

fn pattern(raw: &str) -> PackagePattern {
    PackagePattern::parse(raw).expect("pattern parses")
}

fn route(patterns: &[&str], source: &str) -> Route {
    Route {
        patterns: patterns.iter().map(|raw| pattern(raw)).collect(),
        source: source.to_string(),
    }
}

fn mounts(entries: Vec<(&str, MountKind)>, default_target: Option<&str>) -> Mounts {
    let map = entries.into_iter().map(|(name, kind)| (name.to_string(), kind)).collect();
    Mounts::new(map, default_target.map(str::to_string))
}

// --- PackagePattern::parse -------------------------------------------------

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
            matches!(PackagePattern::parse(raw), Err(MountConfigError::InvalidPattern { .. })),
            "expected {raw:?} to be rejected",
        );
    }
}

// --- PackagePattern::matches -----------------------------------------------

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
/// scoped mount.
#[test]
fn scoped_patterns_require_a_name_segment() {
    for malformed in ["@acme", "@acme/", "@/foo", "@"] {
        assert!(!pattern("@*/*").matches(malformed), "@*/* wrongly matched {malformed:?}");
        assert!(!pattern("@acme/*").matches(malformed), "@acme/* wrongly matched {malformed:?}");
    }
}

// --- PackagePattern::covers ------------------------------------------------

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

// --- resolution ------------------------------------------------------------

#[test]
fn concrete_mount_resolves_to_itself() {
    let registry = mounts(vec![("acme", MountKind::Hosted), ("npmjs", MountKind::Upstream)], None);
    assert_eq!(
        registry.resolve("acme", "@acme/foo"),
        Resolved::Concrete { mount: "acme", kind: ConcreteKind::Hosted },
    );
    assert_eq!(
        registry.resolve("npmjs", "react"),
        Resolved::Concrete { mount: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn unknown_mount_resolves_to_unknown() {
    let registry = mounts(vec![("npmjs", MountKind::Upstream)], None);
    assert_eq!(registry.resolve("nope", "react"), Resolved::UnknownMount);
}

#[test]
fn router_resolves_first_matching_route_authoritatively() {
    let registry = mounts(
        vec![
            ("acme", MountKind::Hosted),
            ("corp", MountKind::Upstream),
            ("npmjs", MountKind::Upstream),
            (
                "main",
                MountKind::Router {
                    routes: vec![
                        route(&["@acme/*"], "acme"),
                        route(&["@corp/*"], "corp"),
                        route(&["**"], "npmjs"),
                    ],
                },
            ),
        ],
        Some("main"),
    );

    assert_eq!(
        registry.resolve("main", "@acme/foo"),
        Resolved::Concrete { mount: "acme", kind: ConcreteKind::Hosted },
    );
    assert_eq!(
        registry.resolve("main", "@corp/foo"),
        Resolved::Concrete { mount: "corp", kind: ConcreteKind::Upstream },
    );
    assert_eq!(
        registry.resolve("main", "lodash"),
        Resolved::Concrete { mount: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn router_earlier_route_wins_over_later_catch_all() {
    // `@acme/*` is private (hosted); the `**` catch-all must never be consulted
    // for an `@acme/*` name even though it would also match.
    let registry = mounts(
        vec![
            ("acme", MountKind::Hosted),
            ("npmjs", MountKind::Upstream),
            (
                "main",
                MountKind::Router {
                    routes: vec![route(&["@acme/*"], "acme"), route(&["**"], "npmjs")],
                },
            ),
        ],
        None,
    );
    assert_eq!(
        registry.resolve("main", "@acme/secret"),
        Resolved::Concrete { mount: "acme", kind: ConcreteKind::Hosted },
    );
}

#[test]
fn router_with_no_matching_route_is_no_route_not_fallthrough() {
    let registry = mounts(
        vec![
            ("acme", MountKind::Hosted),
            ("main", MountKind::Router { routes: vec![route(&["@acme/*"], "acme")] }),
        ],
        None,
    );
    assert_eq!(registry.resolve("main", "lodash"), Resolved::NoRoute);
}

#[test]
fn resolve_default_uses_default_target() {
    let registry = mounts(
        vec![
            ("npmjs", MountKind::Upstream),
            ("main", MountKind::Router { routes: vec![route(&["**"], "npmjs")] }),
        ],
        Some("main"),
    );
    assert_eq!(
        registry.resolve_default("react"),
        Resolved::Concrete { mount: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn resolve_default_without_target_is_unknown() {
    let registry = mounts(vec![("npmjs", MountKind::Upstream)], None);
    assert_eq!(registry.resolve_default("react"), Resolved::UnknownMount);
}

// --- validation ------------------------------------------------------------

fn router_mount(routes: Vec<Route>) -> MountKind {
    MountKind::Router { routes }
}

#[test]
fn valid_config_passes() {
    let registry = mounts(
        vec![
            ("acme", MountKind::Hosted),
            ("corp", MountKind::Upstream),
            ("npmjs", MountKind::Upstream),
            (
                "main",
                router_mount(vec![
                    route(&["@acme/*"], "acme"),
                    route(&["@corp/*"], "corp"),
                    route(&["**"], "npmjs"),
                ]),
            ),
        ],
        Some("main"),
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn rejects_undefined_default_target() {
    let registry = mounts(vec![("npmjs", MountKind::Upstream)], Some("ghost"));
    assert_eq!(
        registry.validate(),
        Err(MountConfigError::UndefinedDefaultTarget { target: "ghost".to_string() }),
    );
}

#[test]
fn rejects_catch_all_before_narrower_route() {
    // The dangerous common mistake: `**` first shadows a later private scope.
    let registry = mounts(
        vec![
            ("acme", MountKind::Hosted),
            ("npmjs", MountKind::Upstream),
            ("main", router_mount(vec![route(&["**"], "npmjs"), route(&["@acme/*"], "acme")])),
        ],
        None,
    );
    assert!(matches!(
        registry.validate(),
        Err(MountConfigError::UnreachableRoute { index: 1, .. }),
    ));
}

#[test]
fn rejects_route_shadowed_by_broader_scope() {
    let registry = mounts(
        vec![
            ("acme", MountKind::Hosted),
            ("other", MountKind::Upstream),
            ("main", router_mount(vec![route(&["@*/*"], "other"), route(&["@acme/*"], "acme")])),
        ],
        None,
    );
    assert!(matches!(
        registry.validate(),
        Err(MountConfigError::UnreachableRoute { index: 1, .. }),
    ));
}

#[test]
fn allows_narrower_route_before_broader() {
    // `@acme/foo` exact before `@acme/*` is fine — the exact matches strictly
    // less, so the scope route is still reachable for every other name.
    let registry = mounts(
        vec![
            ("a", MountKind::Hosted),
            ("b", MountKind::Upstream),
            ("main", router_mount(vec![route(&["@acme/foo"], "a"), route(&["@acme/*"], "b")])),
        ],
        None,
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn allows_sibling_scopes_before_any_scoped() {
    // `@a/*` and `@b/*` do not (and cannot) cover `@*/*`, so the broad route
    // stays reachable.
    let registry = mounts(
        vec![
            ("a", MountKind::Hosted),
            ("b", MountKind::Hosted),
            ("rest", MountKind::Upstream),
            (
                "main",
                router_mount(vec![
                    route(&["@a/*"], "a"),
                    route(&["@b/*"], "b"),
                    route(&["@*/*"], "rest"),
                ]),
            ),
        ],
        None,
    );
    assert_eq!(registry.validate(), Ok(()));
}

#[test]
fn rejects_duplicate_pattern() {
    let registry = mounts(
        vec![
            ("a", MountKind::Hosted),
            ("b", MountKind::Upstream),
            ("main", router_mount(vec![route(&["@acme/*"], "a"), route(&["@acme/*"], "b")])),
        ],
        None,
    );
    // A duplicate scope pattern is also unreachable; either diagnosis is a
    // rejection, but the duplicate check runs first within a route's loop only
    // after the reachability check, so the broader (unreachable) error wins.
    assert!(registry.validate().is_err());
}

#[test]
fn rejects_duplicate_pattern_when_reachable() {
    // Two routes whose union is reachable but that share an exact pattern: the
    // second route is reachable via its other pattern, so the duplicate-pattern
    // check is what fires.
    let registry = mounts(
        vec![
            ("a", MountKind::Hosted),
            ("b", MountKind::Hosted),
            ("rest", MountKind::Upstream),
            (
                "main",
                router_mount(vec![
                    route(&["@a/foo"], "a"),
                    route(&["@a/foo", "@b/*"], "b"),
                    route(&["**"], "rest"),
                ]),
            ),
        ],
        None,
    );
    assert_eq!(
        registry.validate(),
        Err(MountConfigError::DuplicatePattern {
            router: "main".to_string(),
            pattern: "@a/foo".to_string(),
        }),
    );
}

#[test]
fn rejects_unknown_source() {
    let registry = mounts(vec![("main", router_mount(vec![route(&["**"], "ghost")]))], None);
    assert_eq!(
        registry.validate(),
        Err(MountConfigError::UnknownSource {
            router: "main".to_string(),
            source: "ghost".to_string(),
        }),
    );
}

#[test]
fn rejects_self_referential_router() {
    let registry = mounts(vec![("main", router_mount(vec![route(&["**"], "main")]))], None);
    assert_eq!(
        registry.validate(),
        Err(MountConfigError::SelfReferentialRouter { router: "main".to_string() }),
    );
}

#[test]
fn rejects_router_targeting_another_router() {
    let registry = mounts(
        vec![
            ("npmjs", MountKind::Upstream),
            ("inner", router_mount(vec![route(&["**"], "npmjs")])),
            ("outer", router_mount(vec![route(&["**"], "inner")])),
        ],
        None,
    );
    assert_eq!(
        registry.validate(),
        Err(MountConfigError::NonConcreteSource {
            router: "outer".to_string(),
            source: "inner".to_string(),
        }),
    );
}

#[test]
fn rejects_empty_route() {
    let registry = mounts(
        vec![
            ("npmjs", MountKind::Upstream),
            ("main", router_mount(vec![Route { patterns: vec![], source: "npmjs".to_string() }])),
        ],
        None,
    );
    assert_eq!(
        registry.validate(),
        Err(MountConfigError::EmptyRoute { router: "main".to_string(), index: 0 }),
    );
}
