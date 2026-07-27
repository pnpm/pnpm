use pacquet_lockfile::{DirectoryResolution, LockfileResolution, PkgNameVerPeer};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};

use super::{
    ResolveDependencyTreeOptions, extract_children, landed_on_prior_entry, resolve_dependency_tree,
};

#[test]
fn dependency_engines_runtime_is_walked_as_a_runtime_dependency() {
    let result = ResolveResult {
        id: PkgResolutionId::from("parent@1.0.0"),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(serde_json::json!({
            "name": "parent",
            "version": "1.0.0",
            "engines": {
                "runtime": {
                    "name": "node",
                    "version": "22.19.0",
                    "onFail": "download",
                },
            },
        }))),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: "parent".to_string(),
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some("parent".to_string()),
        policy_violation: None,
    };
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("node".to_string(), "runtime:22.19.0".to_string(), false)],
    );
}

fn manifest_result(manifest: serde_json::Value) -> ResolveResult {
    ResolveResult {
        id: PkgResolutionId::from("parent@1.0.0"),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: "parent".to_string(),
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some("parent".to_string()),
        policy_violation: None,
    }
}

// Regression test for pnpm/pnpm#13334: npm ships bundled dependencies
// inside the package's own tarball, so they must not be resolved as
// edges of their own.
#[test]
fn bundled_dependencies_are_not_walked() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "bundled-dep": "^1.0.0", "regular-dep": "^2.0.0" },
        "optionalDependencies": { "bundled-optional": "^3.0.0" },
        "bundledDependencies": ["bundled-dep", "bundled-optional"],
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("regular-dep".to_string(), "^2.0.0".to_string(), false)],
    );
}

#[test]
fn bundle_dependencies_spelling_is_honored() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "bundled-dep": "^1.0.0", "regular-dep": "^2.0.0" },
        "bundleDependencies": ["bundled-dep"],
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("regular-dep".to_string(), "^2.0.0".to_string(), false)],
    );
}

#[test]
fn bundled_dependencies_true_bundles_every_dependency() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "one": "^1.0.0", "two": "^2.0.0" },
        "optionalDependencies": { "three": "^3.0.0" },
        "bundledDependencies": true,
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("three".to_string(), "^3.0.0".to_string(), true)],
    );
}

// `bundledDependencies: true` names the `dependencies` keys, and upstream
// filters the merged `{...optionalDependencies, ...dependencies}` map, so an
// alias listed in both groups is dropped from both.
#[test]
fn bundled_dependencies_true_also_drops_the_optional_duplicate() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "both": "^1.0.0" },
        "optionalDependencies": { "both": "^1.0.0", "optional-only": "^3.0.0" },
        "bundledDependencies": true,
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("optional-only".to_string(), "^3.0.0".to_string(), true)],
    );
}

fn key(raw: &str) -> PkgNameVerPeer {
    raw.parse().expect("parse snapshot key")
}

struct NestedWorkspaceLinkResolver {
    target_dir: std::path::PathBuf,
}

impl Resolver for NestedWorkspaceLinkResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let target_dir = self.target_dir.clone();
        let project_dir = opts.project_dir.clone();
        let alias = wanted.alias.clone().unwrap_or_default();
        Box::pin(async move {
            if alias != "shared" {
                return Ok(None);
            }
            let relative = pathdiff::diff_paths(target_dir, project_dir)
                .expect("target can be relativized")
                .display()
                .to_string()
                .replace('\\', "/");
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(format!("link:{relative}")),
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: Some(std::sync::Arc::new(
                    serde_json::json!({ "name": "shared", "version": "1.0.0" }),
                )),
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: relative,
                }),
                resolved_via: "workspace".to_string(),
                normalized_bare_specifier: None,
                alias: Some(alias),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

#[tokio::test]
async fn canonical_snapshot_link_id_is_relative_to_lockfile_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let manifest_path = temp.path().join("package.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "shared": "workspace:*" },
        }))
        .expect("serialize manifest"),
    )
    .expect("write manifest");
    let manifest = PackageManifest::from_path(manifest_path).expect("parse manifest");
    let project_dir = std::path::PathBuf::from("/repo/apps/nested/app");
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let resolver = NestedWorkspaceLinkResolver { target_dir: lockfile_dir.join("packages/shared") };

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { project_dir, lockfile_dir, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("resolve nested workspace link");

    let direct = tree.direct.first().expect("shared direct dependency");
    assert_eq!(direct.id, "link:packages/shared");
    assert_eq!(direct.node_id, crate::NodeId::leaf("link:packages/shared"));
    assert!(tree.packages.contains_key("link:packages/shared"));
    assert!(!tree.packages.contains_key("link:../../../packages/shared"));
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
    use super::{ChildrenOwner, ChildrenOwnerEntry, WorkspaceTreeCtx, lock_recoverable};
    use std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    let ctx = WorkspaceTreeCtx::default();
    let owner = ChildrenOwner {
        update_active: false,
        depth: 1,
        importer_order: 0,
        parent_path: vec!["root-dep@1.0.0".to_string()],
        importer_id: ".".to_string(),
    };
    let entry = |owner: ChildrenOwner| ChildrenOwnerEntry {
        owner,
        peer_shadowed: Arc::new(HashSet::new()),
    };
    lock_recoverable(&ctx.children_owner_by_id)
        .insert("pkg@1.0.0".to_string(), entry(owner.clone()));

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
    lock_recoverable(&ctx.children_owner_by_id).insert("pkg@1.0.0".to_string(), entry(new_owner));
    ctx.record_first_walk_missing(".", &miss(&[]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0").map(HashSet::len),
        Some(0),
        "a new ownership generation records afresh",
    );
}

#[test]
fn importer_scoped_update_owner_wins_before_discovery_order() {
    use super::ChildrenOwner;

    let ordinary = ChildrenOwner {
        update_active: false,
        depth: 0,
        importer_order: 0,
        parent_path: Vec::new(),
        importer_id: "unselected".to_string(),
    };
    let update_active = ChildrenOwner {
        update_active: true,
        depth: 10,
        importer_order: 10,
        parent_path: vec!["later".to_string()],
        importer_id: "selected".to_string(),
    };

    assert!(update_active.wins_over(&ordinary));
    assert!(!ordinary.wins_over(&update_active));
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
    fn returns_alias_for_npm_range_form() {
        // `foo@npm:^1.0.0`: the body after `npm:` is a semver range,
        // not a name. The install alias `foo` is the real package
        // name — without this branch, the range string itself would
        // be returned as the name and update targeting would miss.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("npm:^1.0.0"))).as_deref(),
            Some("foo"),
        );
    }

    #[test]
    fn returns_alias_for_npm_range_form_with_complex_range() {
        // The `npm:<range>` form supports any valid semver range in
        // the body — `>=1.0.0 <2.0.0`, `~1.2.3`, `1.x`, etc.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("npm:>=1.0.0 <2.0.0"))).as_deref(),
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
        // jsr dep could match an update target by alias and wrongly be
        // treated as one.
        assert_eq!(
            real_package_name_of(&wanted(Some("foo"), Some("jsr:foo@^1.0.0"))).as_deref(),
            None,
        );
    }
}

mod is_update_target {
    use std::collections::HashSet;

    use pacquet_resolving_resolver_base::WantedDependency;

    use super::super::{UpdateDepth, UpdateReuseScope, UpdateScope, is_update_target};

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
            0,
        ));
    }

    #[test]
    fn returns_false_for_none_scope() {
        // `None` is the "no reuse" sentinel; same outcome as `All` here.
        assert!(!is_update_target(
            unlimited(&UpdateReuseScope::None),
            &wanted_with(Some("foo"), Some("^1.0.0")),
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
            0,
        ));
    }

    #[test]
    fn returns_false_for_except_scope_when_not_targeted() {
        // `foo` is not in the user's update target list.
        assert!(!is_update_target(
            unlimited(&except(&["bar"])),
            &wanted_with(Some("foo"), Some("^1.0.0")),
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
            0,
        ));
    }

    #[test]
    fn returns_false_when_real_name_is_unrecoverable() {
        // Alias missing AND no bare_specifier pattern that yields a name.
        // Defensive: "not a targeted update" since we can't match.
        assert!(!is_update_target(unlimited(&except(&["foo"])), &wanted_with(None, None), 0));
    }

    #[test]
    fn depth_zero_targets_direct_dependencies_only() {
        let reuse = except(&["foo"]);
        let scope = UpdateScope { reuse: &reuse, max_depth: UpdateDepth::new(0) };
        let wanted = wanted_with(Some("foo"), Some("^1.0.0"));

        assert!(is_update_target(scope, &wanted, 0));
        assert!(!is_update_target(scope, &wanted, 1));
    }

    #[test]
    fn a_finite_depth_reaches_every_level_up_to_it() {
        let reuse = except(&["foo"]);
        let scope = UpdateScope { reuse: &reuse, max_depth: UpdateDepth::new(2) };
        let wanted = wanted_with(Some("foo"), Some("^1.0.0"));

        assert!(is_update_target(scope, &wanted, 2));
        assert!(!is_update_target(scope, &wanted, 3));
    }

    #[test]
    fn a_depth_no_graph_can_reach_is_unlimited() {
        let reuse = except(&["foo"]);
        let scope = UpdateScope { reuse: &reuse, max_depth: UpdateDepth::new(usize::MAX) };

        assert!(is_update_target(scope, &wanted_with(Some("foo"), Some("^1.0.0")), i32::MAX));
    }
}

/// With `name_ver` unset (git / tarball / local resolutions), the
/// deprecation payload's name and version come from the manifest, and a
/// manifest missing either field suppresses the warning instead of
/// emitting a malformed `name@` payload.
#[test]
fn deprecated_pkg_name_ver_falls_back_to_the_manifest() {
    let result = |manifest: serde_json::Value| ResolveResult {
        id: PkgResolutionId::from("git-pkg@https://example.com/repo.tgz"),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: ".".to_string(),
        }),
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: None,
        alias: Some("git-pkg".to_string()),
        policy_violation: None,
    };

    assert_eq!(
        super::deprecated_pkg_name_ver(&result(
            serde_json::json!({ "name": "git-pkg", "version": "2.0.0" })
        )),
        Some(("git-pkg".to_string(), "2.0.0".to_string())),
    );
    assert_eq!(
        super::deprecated_pkg_name_ver(&result(serde_json::json!({ "name": "git-pkg" }))),
        None,
    );
}

/// A resolver that hands back no manifest still has to give the package
/// an identity — see <https://github.com/pnpm/pnpm/issues/13410>.
mod fallback_manifest {
    use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
    use pacquet_resolving_resolver_base::{CurrentPkg, PkgResolutionId, WantedDependency};

    fn wanted(alias: Option<&str>, bare_specifier: Option<&str>) -> WantedDependency {
        WantedDependency {
            alias: alias.map(str::to_string),
            bare_specifier: bare_specifier.map(str::to_string),
            ..WantedDependency::default()
        }
    }

    fn current_pkg(name: Option<&str>, version: Option<&str>) -> CurrentPkg {
        CurrentPkg {
            id: PkgResolutionId::from("file:sub"),
            name: name.map(str::to_string),
            version: version.map(str::to_string),
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "sub".to_string(),
            }),
            published_at: None,
        }
    }

    #[test]
    fn the_alias_names_the_package() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(Some("no-manifest"), Some("file:./no-manifest-1.0.0.tgz")),
                None,
            ),
            serde_json::json!({ "name": "no-manifest", "version": "0.0.0" }),
        );
    }

    #[test]
    fn an_unaliased_dep_is_named_by_its_specifier_s_last_segment() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(None, Some("https://example.com/no-manifest-1.0.0.tgz")),
                None,
            ),
            serde_json::json!({ "name": "no-manifest-1.0.0.tgz", "version": "0.0.0" }),
        );
    }

    #[test]
    fn the_lockfile_s_pin_wins_over_the_alias() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(Some("sub"), Some("file:./sub")),
                Some(&current_pkg(Some("sub"), Some("2.0.0"))),
            ),
            serde_json::json!({ "name": "sub", "version": "2.0.0" }),
        );
    }

    #[test]
    fn a_half_recorded_pin_falls_through_to_the_alias() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(Some("sub"), Some("file:./sub")),
                Some(&current_pkg(Some("sub"), None)),
            ),
            serde_json::json!({ "name": "sub", "version": "0.0.0" }),
        );
    }
}
