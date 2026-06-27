use pacquet_lockfile::PkgNameVerPeer;

use super::landed_on_prior_entry;

fn key(raw: &str) -> PkgNameVerPeer {
    raw.parse().expect("parse snapshot key")
}

#[test]
fn matches_a_plain_registry_id() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0"));
    assert!(!landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.1.0"));
}

#[test]
fn strips_the_recorded_key_peer_and_patch_suffixes() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0(bar@2.0.0)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)(bar@2.0.0)"), "foo@1.0.0"));
}

#[test]
fn strips_the_resolved_id_patch_suffix() {
    assert!(landed_on_prior_entry(
        &key("foo@1.0.0(patch_hash=0000)"),
        "foo@1.0.0(patch_hash=0000)"
    ));
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0(patch_hash=0000)"));
}

#[test]
fn matches_a_name_prefixed_file_id() {
    assert!(landed_on_prior_entry(&key("foo@file:packages/foo"), "foo@file:packages/foo"));
    assert!(!landed_on_prior_entry(&key("foo@file:packages/foo"), "file:packages/foo"));
}

#[test]
fn owner_missing_record_is_written_once_per_generation() {
    use super::{ChildrenOwner, WorkspaceTreeCtx, lock_recoverable};
    use std::collections::{HashMap, HashSet};

    let ctx = WorkspaceTreeCtx::default();
    let owner = ChildrenOwner {
        depth: 1,
        importer_order: 0,
        parent_path: vec!["root-dep@1.0.0".to_string()],
        importer_id: ".".to_string(),
    };
    lock_recoverable(&ctx.children_owner_by_id).insert("pkg@1.0.0".to_string(), owner.clone());

    let miss = |names: &[&str]| {
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        map.insert("pkg@1.0.0".to_string(), names.iter().map(|name| (*name).to_string()).collect());
        map
    };

    ctx.record_first_walk_missing("pkg-a", &miss(&["peer"]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0"),
        Some(&miss(&["peer"]).remove("pkg@1.0.0").unwrap()),
    );

    ctx.record_first_walk_missing(".", &miss(&["peer", "other-peer"]));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert_eq!(recorded.get("pkg@1.0.0").map(HashSet::len), Some(2));

    ctx.record_first_walk_missing(".", &miss(&[]));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert!(
        recorded.get("pkg@1.0.0").is_some_and(|names| names.contains("peer")),
        "the owner's post-hoist pass must not refresh the generation's record",
    );

    let new_owner = ChildrenOwner { depth: 0, ..owner };
    lock_recoverable(&ctx.children_owner_by_id).insert("pkg@1.0.0".to_string(), new_owner);
    ctx.record_first_walk_missing(".", &miss(&[]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0").map(HashSet::len),
        Some(0),
        "a new ownership generation records afresh",
    );
}

mod higher_direct_dep_version {
    use std::collections::HashMap;

    use node_semver::{Range, Version};

    use super::super::{DirectDepVersions, higher_direct_dep_version};

    fn direct(name: &str, versions: &[&str]) -> DirectDepVersions {
        let parsed =
            versions.iter().map(|raw| raw.parse::<Version>().expect("parse version")).collect();
        HashMap::from([(name.to_string(), parsed)])
    }

    fn ver(raw: &str) -> Version {
        raw.parse().expect("parse version")
    }

    fn range(raw: &str) -> Range {
        raw.parse().expect("parse range")
    }

    #[test]
    fn picks_the_highest_in_range_version_above_the_pin() {
        let direct = direct("foo", &["1.1.0", "1.5.0", "2.0.0"]);
        assert_eq!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range("^1.0.0")),
            Some(ver("1.5.0")),
        );
    }

    #[test]
    fn none_when_no_direct_version_is_higher() {
        let direct = direct("foo", &["1.0.0"]);
        assert!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range("^1.0.0"))
                .is_none(),
        );
    }

    #[test]
    fn does_not_refresh_a_prerelease_onto_a_stable_range() {
        // Matches pnpm's `semver.satisfies(.., true)`: a prerelease does not
        // satisfy a range that doesn't admit prereleases, so no refresh.
        let direct = direct("foo", &["1.2.0-beta.1"]);
        assert!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range("^1.0.0"))
                .is_none(),
        );
    }

    #[test]
    fn refreshes_a_prerelease_when_the_range_admits_it() {
        let direct = direct("foo", &["1.2.0-beta.1"]);
        assert_eq!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range(">=1.2.0-0")),
            Some(ver("1.2.0-beta.1")),
        );
    }
}

mod real_package_name_of {
    use pacquet_resolving_resolver_base::WantedDependency;

    use super::super::real_package_name_of;

    fn wanted(alias: Option<&str>, bare_specifier: Option<&str>) -> WantedDependency {
        WantedDependency {
            alias: alias.map(str::to_string),
            bare_specifier: bare_specifier.map(str::to_string),
            ..WantedDependency::default()
        }
    }

    #[test]
    fn returns_none_when_bare_specifier_is_missing() {
        assert_eq!(real_package_name_of(&wanted(Some("foo"), None)).as_deref(), None);
    }

    #[test]
    fn falls_back_to_alias_for_plain_dep() {
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("^1.0.0"))).as_deref(),
            Some("foo"),
        );
    }

    #[test]
    fn falls_back_to_none_when_alias_is_missing_for_plain_dep() {
        assert_eq!(real_package_name_of(&wanted(None, Some("^1.0.0"))).as_deref(), None);
    }

    #[test]
    fn parses_real_name_from_npm_alias_with_version_range() {
        // Update targeting is keyed by the real name (matches the depPath
        // recorded in the lockfile, not the install alias).
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("npm:bar@^4"))).as_deref(),
            Some("bar"),
        );
    }

    #[test]
    fn parses_real_name_from_npm_alias_without_version() {
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("npm:bar"))).as_deref(),
            Some("bar"),
        );
    }

    #[test]
    fn parses_scoped_real_name_from_npm_alias() {
        // The `@` of the scope prefix sits at index 0, so the `idx >= 1`
        // guard skips it and the search finds the `@` separating name
        // from version.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("npm:@scope/pkg@^4"))).as_deref(),
            Some("@scope/pkg"),
        );
    }

    #[test]
    fn parses_scoped_real_name_from_npm_alias_without_version() {
        // Only one `@` (the scope marker) at index 0, which the
        // `idx >= 1` guard skips — the whole `rest` is the name.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("npm:@scope/pkg"))).as_deref(),
            Some("@scope/pkg"),
        );
    }

    #[test]
    fn returns_none_for_empty_npm_alias_target() {
        // Defensive: filtered out so the caller treats this as "not a
        // targeted update."
        assert_eq!(real_package_name_of(&wanted(Some("foo"), Some("npm:"))).as_deref(), None);
    }

    #[test]
    fn folds_jsr_specifier_to_npm_registry_name_with_version_range() {
        // `foo@jsr:@foo/bar@^1`: install alias is `foo`, but the picker
        // and lockfile snapshots key on the folded npm registry name
        // (`@jsr/foo__bar`). Update targeting must match against this
        // folded name, not the original jsr name, or the bypass won't
        // fire for jsr deps.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("jsr:@foo/bar@^1"))).as_deref(),
            Some("@jsr/foo__bar"),
        );
    }

    #[test]
    fn folds_jsr_specifier_to_npm_registry_name_without_version() {
        // Default-tag form `jsr:@foo/bar`: still folds to `@jsr/foo__bar`.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("jsr:@foo/bar"))).as_deref(),
            Some("@jsr/foo__bar"),
        );
    }

    #[test]
    fn returns_none_for_unparsable_jsr_specifier() {
        // A `jsr:` specifier that the parser rejects (here: missing scope)
        // must not fall back to the install alias — otherwise a broken
        // jsr dep could match an `updateMatching` target by alias and
        // wrongly bypass preferred versions.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("jsr:foo@^1.0.0"))).as_deref(),
            None,
        );
    }
}

mod should_bypass_preferred {
    use std::collections::HashSet;

    use pacquet_resolving_resolver_base::WantedDependency;

    use super::super::{UpdateReuseScope, should_bypass_preferred};

    fn wanted_with(alias: Option<&str>, bare_specifier: Option<&str>) -> WantedDependency {
        WantedDependency {
            alias: alias.map(str::to_string),
            bare_specifier: bare_specifier.map(str::to_string),
            ..WantedDependency::default()
        }
    }

    fn except(names: &[&str]) -> UpdateReuseScope {
        UpdateReuseScope::Except(names.iter().map(|s| (*s).to_string()).collect::<HashSet<_>>())
    }

    #[test]
    fn returns_false_for_all_scope() {
        // `All` = install/add default: no package is targeted for update.
        assert!(!should_bypass_preferred(
            &UpdateReuseScope::All,
            &wanted_with(Some("foo"), Some("^1.0.0")),
        ));
    }

    #[test]
    fn returns_false_for_none_scope() {
        // `None` is the "no reuse" sentinel; same outcome as `All` here.
        assert!(!should_bypass_preferred(
            &UpdateReuseScope::None,
            &wanted_with(Some("foo"), Some("^1.0.0")),
        ));
    }

    #[test]
    fn returns_true_for_except_scope_when_targeted() {
        // `foo` is in the user's update target list → bypass preferred
        // versions for this resolution.
        assert!(should_bypass_preferred(
            &except(&["foo"]),
            &wanted_with(Some("foo"), Some("^1.0.0")),
        ));
    }

    #[test]
    fn returns_false_for_except_scope_when_not_targeted() {
        // `foo` is not in the user's update target list → dedup as usual.
        assert!(!should_bypass_preferred(
            &except(&["bar"]),
            &wanted_with(Some("foo"), Some("^1.0.0")),
        ));
    }

    #[test]
    fn matches_real_name_for_npm_alias_target() {
        // The user updates `bar`, but the importer installed it under
        // alias `foo` via `foo@npm:bar@^4`. The real name `bar` is in
        // the target list, so the bypass fires for the aliased dep.
        assert!(should_bypass_preferred(
            &except(&["bar"]),
            &wanted_with(Some("foo"), Some("npm:bar@^4")),
        ));
    }

    #[test]
    fn returns_false_when_real_name_is_unrecoverable() {
        // Alias missing AND no bare_specifier pattern that yields a name.
        // Defensive: "not a targeted update" since we can't match.
        assert!(!should_bypass_preferred(&except(&["foo"]), &wanted_with(None, None),));
    }
}
