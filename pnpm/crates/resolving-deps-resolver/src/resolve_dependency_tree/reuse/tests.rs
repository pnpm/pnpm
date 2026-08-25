mod higher_direct_dep_version {
    use rustc_hash::FxHashMap as HashMap;

    use node_semver::{Range, Version};

    use super::super::{DirectDepVersions, higher_direct_dep_version};

    fn direct(name: &str, versions: &[&str]) -> DirectDepVersions {
        let parsed =
            versions.iter().map(|raw| raw.parse::<Version>().expect("parse version")).collect();
        HashMap::from_iter([(name.to_string(), parsed)])
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
    use super::super::real_package_name_of;

    #[test]
    fn returns_none_when_bare_specifier_is_missing() {
        assert_eq!(real_package_name_of(Some("foo"), None).as_deref(), None);
    }

    #[test]
    fn falls_back_to_alias_for_plain_dep() {
        assert_eq!(real_package_name_of(Some("foo"), Some("^1.0.0")).as_deref(), Some("foo"));
    }

    #[test]
    fn falls_back_to_none_when_alias_is_missing_for_plain_dep() {
        assert_eq!(real_package_name_of(None, Some("^1.0.0")).as_deref(), None);
    }

    #[test]
    fn parses_real_name_from_npm_alias_with_version_range() {
        // Update targeting is keyed by the real name (matches the depPath
        // recorded in the lockfile, not the install alias).
        assert_eq!(real_package_name_of(Some("foo"), Some("npm:bar@^4")).as_deref(), Some("bar"));
    }

    #[test]
    fn parses_real_name_from_npm_alias_without_version() {
        assert_eq!(real_package_name_of(Some("foo"), Some("npm:bar")).as_deref(), Some("bar"));
    }

    #[test]
    fn parses_scoped_real_name_from_npm_alias() {
        // The `@` of the scope prefix sits at index 0, so the `idx >= 1`
        // guard skips it and the search finds the `@` separating name
        // from version.
        assert_eq!(
            real_package_name_of(Some("foo"), Some("npm:@scope/pkg@^4")).as_deref(),
            Some("@scope/pkg"),
        );
    }

    #[test]
    fn parses_scoped_real_name_from_npm_alias_without_version() {
        // Only one `@` (the scope marker) at index 0, which the
        // `idx >= 1` guard skips — the whole `rest` is the name.
        assert_eq!(
            real_package_name_of(Some("foo"), Some("npm:@scope/pkg")).as_deref(),
            Some("@scope/pkg"),
        );
    }

    #[test]
    fn returns_none_for_empty_npm_alias_target() {
        // Defensive: filtered out so the caller treats this as "not a
        // targeted update."
        assert_eq!(real_package_name_of(Some("foo"), Some("npm:")).as_deref(), None);
    }

    #[test]
    fn returns_alias_for_npm_range_form() {
        // `foo@npm:^1.0.0`: the body after `npm:` is a semver range,
        // not a name. The install alias `foo` is the real package
        // name — without this branch, the range string itself would
        // be returned as the name and update targeting would miss.
        assert_eq!(real_package_name_of(Some("foo"), Some("npm:^1.0.0")).as_deref(), Some("foo"));
    }

    #[test]
    fn returns_alias_for_npm_range_form_with_complex_range() {
        // The `npm:<range>` form supports any valid semver range in
        // the body — `>=1.0.0 <2.0.0`, `~1.2.3`, `1.x`, etc.
        assert_eq!(
            real_package_name_of(Some("foo"), Some("npm:>=1.0.0 <2.0.0")).as_deref(),
            Some("foo"),
        );
    }

    #[test]
    fn folds_jsr_specifier_to_npm_registry_name_with_version_range() {
        // `foo@jsr:@foo/bar@^1`: install alias is `foo`, but the picker
        // and lockfile snapshots key on the folded npm registry name
        // (`@jsr/foo__bar`). Update targeting must match against this
        // folded name, not the original jsr name, or jsr deps would
        // never count as update targets.
        assert_eq!(
            real_package_name_of(Some("foo"), Some("jsr:@foo/bar@^1")).as_deref(),
            Some("@jsr/foo__bar"),
        );
    }

    #[test]
    fn folds_jsr_specifier_to_npm_registry_name_without_version() {
        // Default-tag form `jsr:@foo/bar`: still folds to `@jsr/foo__bar`.
        assert_eq!(
            real_package_name_of(Some("foo"), Some("jsr:@foo/bar")).as_deref(),
            Some("@jsr/foo__bar"),
        );
    }

    #[test]
    fn returns_none_for_unparsable_jsr_specifier() {
        // A `jsr:` specifier that the parser rejects (here: missing scope)
        // must not fall back to the install alias — otherwise a broken
        // jsr dep could match an update target by alias and wrongly be
        // treated as one.
        assert_eq!(real_package_name_of(Some("foo"), Some("jsr:foo@^1.0.0")).as_deref(), None);
    }
}

mod is_update_target {
    use pnpm_resolving_resolver_base::WantedDependency;

    use crate::{UpdateTargets, VersionLine};

    use super::super::{UpdateDepth, UpdateReuseScope, UpdateScope, is_update_target};

    fn wanted_with(alias: Option<&str>, bare_specifier: Option<&str>) -> WantedDependency {
        WantedDependency {
            alias: alias.map(str::to_string),
            bare_specifier: bare_specifier.map(str::to_string),
            ..WantedDependency::default()
        }
    }

    fn except(names: &[&str]) -> UpdateReuseScope {
        UpdateReuseScope::Except(
            names.iter().map(|name| ((*name).to_string(), None)).collect::<UpdateTargets>(),
        )
    }

    /// A selector that pinned an exact version, so the target is scoped to
    /// that version's line.
    fn except_line(name: &str, version: &str) -> UpdateReuseScope {
        UpdateReuseScope::Except(
            std::iter::once((name.to_string(), VersionLine::parse(version))).collect(),
        )
    }

    fn version(version: &str) -> node_semver::Version {
        version.parse().expect("parse version")
    }

    /// The scope of a `--depth Infinity` update — the default, under
    /// which every node is judged by name alone.
    fn unlimited(reuse: &UpdateReuseScope) -> UpdateScope<'_> {
        UpdateScope { reuse, max_depth: UpdateDepth::UNLIMITED }
    }

    #[test]
    fn returns_false_for_all_scope() {
        // `All` = install/add default: no package is targeted for update.
        assert!(!is_update_target(
            unlimited(&UpdateReuseScope::All),
            &wanted_with(Some("foo"), Some("^1.0.0")),
            None,
            0,
        ));
    }

    #[test]
    fn returns_false_for_none_scope() {
        // `None` is the "no reuse" sentinel; same outcome as `All` here.
        assert!(!is_update_target(
            unlimited(&UpdateReuseScope::None),
            &wanted_with(Some("foo"), Some("^1.0.0")),
            None,
            0,
        ));
    }

    #[test]
    fn returns_true_for_except_scope_when_targeted() {
        // `foo` is in the user's update target list → this resolution
        // carries `update_requested`.
        assert!(is_update_target(
            unlimited(&except(&["foo"])),
            &wanted_with(Some("foo"), Some("^1.0.0")),
            None,
            0,
        ));
    }

    #[test]
    fn returns_false_for_except_scope_when_not_targeted() {
        // `foo` is not in the user's update target list.
        assert!(!is_update_target(
            unlimited(&except(&["bar"])),
            &wanted_with(Some("foo"), Some("^1.0.0")),
            None,
            0,
        ));
    }

    #[test]
    fn matches_real_name_for_npm_alias_target() {
        // The user updates `bar`, but the importer installed it under
        // alias `foo` via `foo@npm:bar@^4`. The real name `bar` is in
        // the target list, so the aliased dep counts as a target.
        assert!(is_update_target(
            unlimited(&except(&["bar"])),
            &wanted_with(Some("foo"), Some("npm:bar@^4")),
            None,
            0,
        ));
    }

    #[test]
    fn a_pinned_selector_targets_only_its_version_line() {
        let reuse = except_line("foo", "1.2.3");

        assert!(is_update_target(
            unlimited(&reuse),
            &wanted_with(Some("foo"), Some("^1.0.0")),
            Some(&version("1.0.0")),
            0,
        ));
        assert!(!is_update_target(
            unlimited(&reuse),
            &wanted_with(Some("foo"), Some("^2.0.0")),
            Some(&version("2.5.0")),
            0,
        ));
    }

    #[test]
    fn a_pinned_zero_x_selector_targets_only_its_minor_line() {
        let reuse = except_line("foo", "0.2.5");

        assert!(is_update_target(
            unlimited(&reuse),
            &wanted_with(Some("foo"), Some("^0.2.0")),
            Some(&version("0.2.1")),
            0,
        ));
        assert!(!is_update_target(
            unlimited(&reuse),
            &wanted_with(Some("foo"), Some("^0.3.0")),
            Some(&version("0.3.0")),
            0,
        ));
    }

    #[test]
    fn a_pinned_selector_matches_an_edge_with_no_locked_version() {
        // Nothing to judge the line against yet, so the name decides —
        // as it does in pnpm's version-less `updateMatching` calls.
        assert!(is_update_target(
            unlimited(&except_line("foo", "1.2.3")),
            &wanted_with(Some("foo"), Some("^2.0.0")),
            None,
            0,
        ));
    }

    #[test]
    fn returns_false_when_real_name_is_unrecoverable() {
        // Alias missing AND no bare_specifier pattern that yields a name.
        // Defensive: "not a targeted update" since we can't match.
        assert!(!is_update_target(unlimited(&except(&["foo"])), &wanted_with(None, None), None, 0));
    }

    #[test]
    fn depth_zero_targets_direct_dependencies_only() {
        let reuse = except(&["foo"]);
        let scope = UpdateScope { reuse: &reuse, max_depth: UpdateDepth::new(0) };
        let wanted = wanted_with(Some("foo"), Some("^1.0.0"));

        assert!(is_update_target(scope, &wanted, None, 0));
        assert!(!is_update_target(scope, &wanted, None, 1));
    }

    #[test]
    fn a_finite_depth_reaches_every_level_up_to_it() {
        let reuse = except(&["foo"]);
        let scope = UpdateScope { reuse: &reuse, max_depth: UpdateDepth::new(2) };
        let wanted = wanted_with(Some("foo"), Some("^1.0.0"));

        assert!(is_update_target(scope, &wanted, None, 2));
        assert!(!is_update_target(scope, &wanted, None, 3));
    }

    #[test]
    fn a_depth_no_graph_can_reach_is_unlimited() {
        let reuse = except(&["foo"]);
        let scope = UpdateScope { reuse: &reuse, max_depth: UpdateDepth::new(usize::MAX) };

        assert!(is_update_target(scope, &wanted_with(Some("foo"), Some("^1.0.0")), None, i32::MAX));
    }
}
