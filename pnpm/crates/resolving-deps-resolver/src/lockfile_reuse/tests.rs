use std::collections::HashMap;

/// The registry facts with only the scope map populated — what these tests
/// vary; the aliases and per-registry settings stay empty.
fn registry_context(registries: HashMap<String, String>) -> pnpm_lockfile::RegistryContext {
    pnpm_lockfile::RegistryContext { registries, ..Default::default() }
}

use pnpm_lockfile::{
    BundledDependencies, ComVer, GitResolution, ImporterDepVersion, Lockfile, LockfileResolution,
    LockfileVersion, PackageMetadata, PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot,
    RegistryResolution, ResolvedDependencySpec, StringOrList, TarballResolution, TarballRevision,
};

use super::{reusable_importer_dep, synthesize_reused_result};

fn single_dep_lockfile(alias: &str, specifier: &str, resolved: &str) -> Lockfile {
    let mut deps = HashMap::new();
    deps.insert(
        alias.parse::<PkgName>().expect("parse alias"),
        ResolvedDependencySpec {
            specifier: specifier.to_string(),
            version: ImporterDepVersion::Regular(
                resolved.parse::<PkgVerPeer>().expect("parse version"),
            ),
        },
    );
    let mut lockfile = empty_lockfile();
    lockfile.importers = HashMap::from([(
        ".".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    )]);
    let key: PkgNameVerPeer = format!("{alias}@{resolved}").parse().expect("parse package key");
    lockfile.packages = Some(HashMap::from([(key, registry_metadata())]));
    lockfile
}

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

fn registry_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .parse()
                .expect("parse integrity"),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

#[test]
fn reuses_when_locked_version_satisfies_the_manifest_range() {
    let lockfile = single_dep_lockfile("react", "^18.0.0", "18.2.0");
    let key = reusable_importer_dep(
        &lockfile,
        ".",
        "react",
        "^18.0.0",
        &registry_context(default_registry()),
    )
    .expect("locked 18.2.0 satisfies ^18.0.0");
    assert_eq!(key.to_string(), "react@18.2.0");
}

#[test]
fn reuses_across_a_widened_but_still_satisfied_range() {
    let lockfile = single_dep_lockfile("react", "^18.0.0", "18.2.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "react",
            ">=17",
            &registry_context(default_registry())
        )
        .is_some(),
    );
}

#[test]
fn fresh_resolves_when_range_no_longer_satisfies_locked_version() {
    let lockfile = single_dep_lockfile("react", "^18.0.0", "18.2.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "react",
            "^19.0.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

#[test]
fn fresh_resolves_when_locked_prerelease_does_not_satisfy_stable_range() {
    let lockfile = single_dep_lockfile("react", "21.0.0-rc.0", "21.0.0-rc.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "react",
            "21.0.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "react",
            "^21.0.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

#[test]
fn fresh_resolves_a_new_dependency_absent_from_the_lockfile() {
    let lockfile = single_dep_lockfile("react", "^18.0.0", "18.2.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "left-pad",
            "^1.0.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

#[test]
fn reuses_a_locked_runtime_the_widened_range_still_admits() {
    let lockfile = single_dep_lockfile("node", "runtime:26.10.0", "runtime:26.10.0");
    let key = reusable_importer_dep(
        &lockfile,
        ".",
        "node",
        "runtime:^26",
        &registry_context(default_registry()),
    )
    .expect("locked 26.10.0 satisfies ^26");
    assert_eq!(key.to_string(), "node@runtime:26.10.0");
}

#[test]
fn fresh_resolves_a_runtime_range_that_excludes_the_locked_version() {
    let lockfile = single_dep_lockfile("node", "runtime:^26", "runtime:26.10.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "node",
            "runtime:^27",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

#[test]
fn fresh_resolves_an_unchanged_runtime_range_the_locked_version_left() {
    let lockfile = single_dep_lockfile("node", "runtime:^26", "runtime:27.0.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "node",
            "runtime:^26",
            &registry_context(default_registry())
        )
        .is_none(),
    );
    let lockfile = single_dep_lockfile("node", "runtime:26.10.0", "runtime:26.11.1");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "node",
            "runtime:26.10.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

#[test]
fn reuses_a_locked_runtime_under_an_unchanged_non_range_selector() {
    let lockfile = single_dep_lockfile("node", "runtime:lts", "runtime:24.11.0");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "node",
            "runtime:lts",
            &registry_context(default_registry())
        )
        .is_some(),
    );
}

#[test]
fn reuses_a_locked_runtime_only_from_the_same_release_channel() {
    let lockfile = single_dep_lockfile("node", "runtime:rc/24.0.0-rc.4", "runtime:24.0.0-rc.4");
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "node",
            "runtime:rc/^24.0.0-rc.3",
            &registry_context(default_registry())
        )
        .is_some(),
    );
    assert!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "node",
            "runtime:^24.0.0-rc.3",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

#[test]
fn reuses_an_unchanged_git_specifier_at_its_locked_commit() {
    let specifier = "git+file:///repo#main";
    let resolved = "git+file:///repo#0123456789012345678901234567890123456789";
    let mut lockfile = single_dep_lockfile("git-pkg", specifier, resolved);
    let key: PkgNameVerPeer = format!("git-pkg@{resolved}").parse().expect("parse git key");
    lockfile.packages = Some(HashMap::from([(
        key.clone(),
        PackageMetadata {
            resolution: LockfileResolution::Git(GitResolution {
                repo: "file:///repo".to_string(),
                commit: "0123456789012345678901234567890123456789".to_string(),
                integrity: None,
                path: None,
            }),
            version: Some("1.0.0".to_string()),
            ..registry_metadata()
        },
    )]));

    assert_eq!(
        reusable_importer_dep(
            &lockfile,
            ".",
            "git-pkg",
            specifier,
            &registry_context(default_registry())
        ),
        Some(key),
    );
}

#[test]
fn synthesizes_a_registry_resolution_with_the_recorded_integrity() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let metadata = registry_metadata();
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata.clone())]));

    let result =
        synthesize_reused_result(&lockfile, &key, "react").expect("registry dep is reusable");
    assert_eq!(result.id.as_str(), "react@18.2.0");
    let name_ver = result.package.name_ver.expect("name_ver");
    assert_eq!(name_ver.name.to_string(), "react");
    assert_eq!(name_ver.suffix.to_string(), "18.2.0");
    assert_eq!(result.resolution, metadata.resolution);
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.alias.as_deref(), Some("react"));
    let manifest = result.package.manifest.expect("synthesized manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("react"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("18.2.0"));
}

#[test]
fn synthesized_manifest_carries_peer_metadata() {
    let key: PkgNameVerPeer = "react-dom@18.2.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.peer_dependencies =
        Some(HashMap::from([("react".to_string(), "^18.0.0".to_string())]));
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result =
        synthesize_reused_result(&lockfile, &key, "react-dom").expect("registry dep is reusable");
    let manifest = result.package.manifest.expect("synthesized manifest");
    let peers = manifest
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
        .expect("peers");
    assert_eq!(peers.get("react").and_then(serde_json::Value::as_str), Some("^18.0.0"));
}

#[test]
fn synthesized_manifest_carries_deprecated_metadata() {
    let key: PkgNameVerPeer = "left-pad@1.3.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.deprecated = Some("use String.prototype.padStart()".to_string());
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result =
        synthesize_reused_result(&lockfile, &key, "left-pad").expect("registry dep is reusable");
    let manifest = result.package.manifest.expect("synthesized manifest");
    assert_eq!(
        manifest.get("deprecated").and_then(serde_json::Value::as_str),
        Some("use String.prototype.padStart()"),
    );
}

#[test]
fn synthesized_manifest_carries_bundled_dependencies() {
    let key: PkgNameVerPeer = "pkg-with-bundled-deps@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.bundled_dependencies = Some(BundledDependencies::Names(vec!["napi-wasm".to_string()]));
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result = synthesize_reused_result(&lockfile, &key, "pkg-with-bundled-deps")
        .expect("registry dep is reusable");
    let manifest = result.package.manifest.expect("synthesized manifest");
    assert_eq!(manifest.get("bundledDependencies"), Some(&serde_json::json!(["napi-wasm"])));
}

#[test]
fn synthesized_manifest_carries_the_boolean_bundled_dependencies_form() {
    let key: PkgNameVerPeer = "pkg-bundling-everything@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.bundled_dependencies = Some(BundledDependencies::Boolean(true));
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result = synthesize_reused_result(&lockfile, &key, "pkg-bundling-everything")
        .expect("registry dep is reusable");
    let manifest = result.package.manifest.expect("synthesized manifest");
    assert_eq!(manifest.get("bundledDependencies"), Some(&serde_json::Value::Bool(true)));
}

#[test]
fn synthesized_manifest_keeps_the_scalar_libc_form() {
    let key: PkgNameVerPeer = "pkg-with-scalar-libc@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.libc = Some(StringOrList::String("musl".to_string()));
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result = synthesize_reused_result(&lockfile, &key, "pkg-with-scalar-libc")
        .expect("registry dep is reusable");
    let manifest = result.package.manifest.expect("synthesized manifest");
    assert_eq!(manifest.get("libc"), Some(&serde_json::Value::String("musl".to_string())));
}

#[test]
fn does_not_reuse_directory_resolutions() {
    let key: PkgNameVerPeer = "pkg-from-tarball@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://example.test/pkg.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    assert!(synthesize_reused_result(&lockfile, &key, "pkg-from-tarball").is_none());
}

#[test]
fn synthesizes_a_git_resolution_with_the_locked_commit_and_manifest_version() {
    let key: PkgNameVerPeer = "git-pkg@git+file:///repo#0123456789012345678901234567890123456789"
        .parse()
        .expect("parse git key");
    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Git(GitResolution {
        repo: "file:///repo".to_string(),
        commit: "0123456789012345678901234567890123456789".to_string(),
        integrity: None,
        path: None,
    });
    metadata.version = Some("1.2.3".to_string());
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata.clone())]));

    let result = synthesize_reused_result(&lockfile, &key, "git-pkg")
        .expect("locked git dependency is reusable");
    assert_eq!(result.id.as_str(), key.to_string());
    assert_eq!(result.package.name_ver, None);
    assert_eq!(result.resolution, metadata.resolution);
    assert_eq!(result.resolved_via, "git-repository");
    let manifest = result.package.manifest.expect("synthesized git manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("git-pkg"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("1.2.3"));
}

#[test]
fn does_not_reuse_a_package_absent_from_the_packages_map() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let lockfile = empty_lockfile();
    assert!(synthesize_reused_result(&lockfile, &key, "react").is_none());
}

#[test]
fn does_not_reuse_a_non_semver_version_slot() {
    let key: PkgNameVerPeer =
        "pkg@https://example.test/pkg.tgz".parse().expect("parse url-keyed entry");
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), registry_metadata())]));
    assert!(synthesize_reused_result(&lockfile, &key, "pkg").is_none());
}

fn default_registry() -> HashMap<String, String> {
    HashMap::from([("default".to_string(), "https://registry.example.test/".to_string())])
}

#[test]
fn current_pkg_materializes_a_registry_resolution_into_its_tarball_url() {
    let key: PkgNameVerPeer = "react@18.2.0(foo@1.0.0)".parse().expect("parse key");
    let mut lockfile = empty_lockfile();
    lockfile.packages =
        Some(HashMap::from([("react@18.2.0".parse().expect("parse key"), registry_metadata())]));

    let current_pkg =
        super::current_pkg_from_lockfile(&lockfile, &key, &registry_context(default_registry()))
            .expect("packages entry exists");

    assert_eq!(current_pkg.id.to_string(), "react@18.2.0");
    assert_eq!(current_pkg.name.as_deref(), Some("react"));
    assert_eq!(current_pkg.version.as_deref(), Some("18.2.0"));
    let LockfileResolution::Tarball(tarball) = &current_pkg.resolution else {
        panic!("registry resolution must materialize as a tarball: {:?}", current_pkg.resolution);
    };
    assert_eq!(tarball.tarball, "https://registry.example.test/react/-/react-18.2.0.tgz");
    assert!(tarball.integrity.is_some(), "the recorded integrity carries over");
}

#[test]
fn current_pkg_routes_a_scoped_package_to_its_scope_registry() {
    let key: PkgNameVerPeer = "@scope/pkg@1.0.0".parse().expect("parse key");
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), registry_metadata())]));
    let mut registries = default_registry();
    registries.insert("@scope".to_string(), "https://scoped.example.test/".to_string());

    let current_pkg =
        super::current_pkg_from_lockfile(&lockfile, &key, &registry_context(registries))
            .expect("packages entry exists");

    let LockfileResolution::Tarball(tarball) = &current_pkg.resolution else {
        panic!("registry resolution must materialize as a tarball");
    };
    assert_eq!(tarball.tarball, "https://scoped.example.test/@scope/pkg/-/pkg-1.0.0.tgz");
}

#[test]
fn current_pkg_materializes_a_revision_from_the_registry_prefix_declaration() {
    let key: PkgNameVerPeer = "pkg@work:1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    let LockfileResolution::Registry(registry_resolution) = &mut metadata.resolution else {
        unreachable!("registry_metadata returns a registry resolution");
    };
    registry_resolution.integrity =
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
            .parse()
            .expect("parse integrity");
    registry_resolution.revision = Some(TarballRevision::try_from(7).unwrap());
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));
    let mut context = registry_context(default_registry());
    context.registries_by_prefix.insert(
        "work".to_string(),
        "https://registry.example.test/work/npm/".to_string(),
    );

    let current_pkg = super::current_pkg_from_lockfile(&lockfile, &key, &context)
        .expect("declared prefix makes the revision reusable");

    let LockfileResolution::Tarball(tarball) = current_pkg.resolution else {
        panic!("registry resolution must materialize as a tarball");
    };
    assert_eq!(
        tarball.tarball,
        format!("https://registry.example.test/work/npm/-/tarballs/sha512/{}", "A".repeat(86)),
    );
    assert_eq!(tarball.revision, Some(TarballRevision::try_from(7).unwrap()));
}

#[test]
fn current_pkg_passes_a_recorded_tarball_resolution_through() {
    let key: PkgNameVerPeer = "pkg@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://example.test/pkg-1.0.0.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let current_pkg =
        super::current_pkg_from_lockfile(&lockfile, &key, &registry_context(default_registry()))
            .expect("packages entry exists");

    let LockfileResolution::Tarball(tarball) = &current_pkg.resolution else {
        panic!("tarball resolution must pass through");
    };
    assert_eq!(tarball.tarball, "https://example.test/pkg-1.0.0.tgz");
}

#[test]
fn current_pkg_is_none_without_a_packages_entry() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let lockfile = empty_lockfile();
    assert!(
        super::current_pkg_from_lockfile(&lockfile, &key, &registry_context(default_registry()))
            .is_none(),
    );
}

#[test]
fn current_pkg_is_withheld_for_a_registry_entry_without_a_registry_map() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), registry_metadata())]));
    assert!(
        super::current_pkg_from_lockfile(&lockfile, &key, &registry_context(HashMap::new()))
            .is_none(),
    );
}

#[test]
fn prior_child_key_applies_the_satisfies_gate() {
    let snapshot: pnpm_lockfile::SnapshotEntry =
        serde_json::from_value(serde_json::json!({ "dependencies": { "bar": "1.2.0" } }))
            .expect("parse snapshot entry");

    let key =
        super::prior_child_key(&snapshot, "bar", "^1.0.0", &registry_context(default_registry()))
            .expect("recorded ref satisfies");
    assert_eq!(key.to_string(), "bar@1.2.0");

    assert!(
        super::prior_child_key(&snapshot, "bar", "^2.0.0", &registry_context(default_registry()))
            .is_none(),
        "an edited range the recorded version no longer satisfies yields no prior key",
    );
    assert!(
        super::prior_child_key(&snapshot, "baz", "^1.0.0", &registry_context(default_registry()))
            .is_none(),
        "unrecorded alias",
    );

    let prerelease_snapshot: pnpm_lockfile::SnapshotEntry =
        serde_json::from_value(serde_json::json!({ "dependencies": { "bar": "21.0.0-rc.0" } }))
            .expect("parse snapshot entry");
    assert!(
        super::prior_child_key(
            &prerelease_snapshot,
            "bar",
            "21.0.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
    assert!(
        super::prior_child_key(
            &prerelease_snapshot,
            "bar",
            "^21.0.0",
            &registry_context(default_registry())
        )
        .is_none(),
    );
}

/// Covers <https://github.com/pnpm/pnpm/issues/16654>.
#[test]
fn prior_child_key_rejects_an_alias_ref_for_a_bare_range() {
    let snapshot: pnpm_lockfile::SnapshotEntry = serde_json::from_value(serde_json::json!({
        "optionalDependencies": { "bar": "@scope/bar@1.2.0" },
    }))
    .expect("parse snapshot entry");

    assert!(
        super::prior_child_key(&snapshot, "bar", "^1.0.0", &registry_context(default_registry()))
            .is_none(),
    );
}

/// The registry facts with the `work` and `zeta` prefixes both naming one
/// registry, which the `@work` scope also routes to; `gh` is the built-in
/// alias, which nothing declares.
fn prefixed_registry_context() -> pnpm_lockfile::RegistryContext {
    let registry = "https://npm.example.test/";
    pnpm_lockfile::RegistryContext {
        registries: HashMap::from([
            ("default".to_string(), "https://registry.example.test/".to_string()),
            ("@work".to_string(), registry.to_string()),
        ]),
        registries_by_prefix: HashMap::from([
            ("gh".to_string(), "https://npm.pkg.github.com/".to_string()),
            ("work".to_string(), registry.to_string()),
            ("zeta".to_string(), registry.to_string()),
        ]),
        declared_prefixes: pnpm_lockfile::DeclaredRegistryPrefixes::new([
            ("work", registry),
            ("zeta", registry),
        ]),
        registry_options_by_url: std::collections::BTreeMap::new(),
    }
}

#[test]
fn locked_child_admits_a_named_registry_spec_for_its_registry_and_package() {
    let context = prefixed_registry_context();
    let alias: PkgName = "@scope/old".parse().unwrap();
    let key: PkgNameVerPeer = "@scope/old@work:1.2.0".parse().unwrap();

    // Bare range: the alias carries the package name.
    assert_eq!(super::locked_child_admits(&alias, &key, "work:^1.0.0", &context), Some(true));

    // Aliased shape naming the same package.
    assert_eq!(
        super::locked_child_admits(&alias, &key, "work:@scope/old@^1.0.0", &context),
        Some(true),
    );

    // Aliased shape naming a *different* package: reusing the recorded entry
    // would install `@scope/old` for a dependency that asked for
    // `@scope/new`.
    assert_eq!(
        super::locked_child_admits(&alias, &key, "work:@scope/new@^1.0.0", &context),
        Some(false),
    );

    // A spec aimed at another registry never satisfies this key; another
    // prefix of the same registry does.
    assert_eq!(super::locked_child_admits(&alias, &key, "gh:^1.0.0", &context), Some(false));
    assert_eq!(super::locked_child_admits(&alias, &key, "zeta:^1.0.0", &context), Some(true));

    // A prefix nothing declares is not a specifier the gate can judge.
    assert_eq!(super::locked_child_admits(&alias, &key, "gone:^1.0.0", &context), None);

    assert_eq!(super::locked_child_admits(&alias, &key, "work:^2.0.0", &context), Some(false));
}

#[test]
fn locked_child_admits_a_plain_range_through_the_registry_its_scope_routes_to() {
    let context = prefixed_registry_context();
    let alias: PkgName = "@work/foo".parse().unwrap();
    let qualified: PkgNameVerPeer = "@work/foo@work:1.2.0".parse().unwrap();
    let plain: PkgNameVerPeer = "@work/foo@1.2.0".parse().unwrap();

    // `@work` routes to the registry `work:` names, so either key satisfies
    // a range, and a plain key satisfies the prefixed spec.
    assert_eq!(super::locked_child_admits(&alias, &qualified, "^1.0.0", &context), Some(true));
    assert_eq!(super::locked_child_admits(&alias, &plain, "^1.0.0", &context), Some(true));
    assert_eq!(super::locked_child_admits(&alias, &plain, "work:^1.0.0", &context), Some(true));

    // A key of another registry never satisfies the range.
    let github: PkgNameVerPeer = "@work/foo@gh:1.2.0".parse().unwrap();
    assert_eq!(super::locked_child_admits(&alias, &github, "^1.0.0", &context), Some(false));
    let unscoped: PkgName = "foo".parse().unwrap();
    let unscoped_qualified: PkgNameVerPeer = "foo@work:1.2.0".parse().unwrap();
    assert_eq!(
        super::locked_child_admits(&unscoped, &unscoped_qualified, "^1.0.0", &context),
        Some(false),
    );
}

/// A key the registries setting now writes differently still pins its
/// version; whether its subtree is reused is [`locked_key_is_current`]'s call.
#[test]
fn prior_child_key_keeps_the_pin_of_a_key_the_registries_setting_rekeys() {
    let context = prefixed_registry_context();
    let snapshot: pnpm_lockfile::SnapshotEntry = serde_json::from_value(serde_json::json!({
        "dependencies": {
            "@work/foo": "1.2.0",
            "@work/bar": "work:1.2.0",
            "@work/baz": "zeta:1.2.0",
        },
    }))
    .expect("parse snapshot entry");

    for (alias, specifier) in [
        ("@work/foo", "^1.0.0"),
        ("@work/bar", "^1.0.0"),
        ("@work/bar", "zeta:^1.0.0"),
        ("@work/baz", "zeta:^1.0.0"),
    ] {
        assert!(
            super::prior_child_key(&snapshot, alias, specifier, &context).is_some(),
            "{alias} {specifier}",
        );
    }
}

#[test]
fn reusable_importer_dep_keeps_the_pin_of_a_key_the_registries_setting_rekeys() {
    let context = prefixed_registry_context();
    for recorded in ["1.2.0", "work:1.2.0", "zeta:1.2.0"] {
        let lockfile = single_dep_lockfile("@work/foo", "^1.0.0", recorded);
        assert!(
            reusable_importer_dep(&lockfile, ".", "@work/foo", "^1.0.0", &context).is_some(),
            "{recorded}",
        );
    }
}

/// A reused subtree has to carry the key a fresh resolve of its specifier
/// mints.
#[test]
fn locked_key_is_current_when_it_is_the_key_the_specifier_mints() {
    let context = prefixed_registry_context();
    let alias: PkgName = "@work/foo".parse().unwrap();
    let current = |key: &str, specifier: &str| {
        super::locked_key_is_current(
            &alias,
            &key.parse::<PkgNameVerPeer>().unwrap(),
            specifier,
            &context,
        )
    };

    // The `@work` scope's registry declares `work`, so a plain range mints a
    // `work:` key.
    assert!(current("@work/foo@work:1.2.0", "^1.0.0"));
    assert!(!current("@work/foo@1.2.0", "^1.0.0"));
    // Another declared alias of the registry mints the same key.
    assert!(current("@work/foo@work:1.2.0", "zeta:^1.0.0"));
    assert!(!current("@work/foo@zeta:1.2.0", "zeta:^1.0.0"));
    assert!(!current("@work/foo@zeta:1.2.0", "work:^1.0.0"));
    // Keys that are not registry versions are left alone.
    assert!(current("@work/foo@file:packages/foo", "file:packages/foo"));
}

/// A built-in alias qualifies only the specifiers that spell it, so neither
/// direction between a plain key and an `npmjs:` key is reused.
#[test]
fn locked_key_is_current_follows_the_spelling_of_a_built_in_alias() {
    let context = pnpm_lockfile::RegistryContext {
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        registries_by_prefix: HashMap::from([
            ("npmjs".to_string(), "https://registry.npmjs.org/".to_string()),
            ("gh".to_string(), "https://npm.pkg.github.com/".to_string()),
        ]),
        ..Default::default()
    };
    let alias: PkgName = "foo".parse().unwrap();
    let current = |key: &str, specifier: &str| {
        super::locked_key_is_current(
            &alias,
            &key.parse::<PkgNameVerPeer>().unwrap(),
            specifier,
            &context,
        )
    };

    assert!(current("foo@1.0.0", "^1.0.0"));
    assert!(current("foo@npmjs:1.0.0", "npmjs:^1.0.0"));
    assert!(!current("foo@1.0.0", "npmjs:^1.0.0"));
    assert!(!current("foo@npmjs:1.0.0", "^1.0.0"));
    assert!(current("foo@gh:1.0.0", "gh:^1.0.0"));
}

#[test]
fn attach_snapshot_dependencies_converts_dep_refs_to_manifest_specifiers() {
    let mut manifest = serde_json::json!({ "name": "pkg", "version": "1.0.0" });
    let snapshot: pnpm_lockfile::SnapshotEntry = serde_json::from_value(serde_json::json!({
        "dependencies": {
            "is-positive": "1.0.0(peer@2.0.0)",
            "aliased": "target@2.0.0",
            "linked": "link:packages/sub",
            "node": "runtime:22.0.0(peer@1.0.0)",
            "custom-node": "node@runtime:22.0.0(peer@1.0.0)",
        },
        "optionalDependencies": {
            "opt": "3.0.0",
        },
    }))
    .expect("parse snapshot");

    super::attach_snapshot_dependencies(&mut manifest, Some(&snapshot));

    assert_eq!(
        manifest,
        serde_json::json!({
            "name": "pkg",
            "version": "1.0.0",
            "dependencies": {
                "aliased": "npm:target@2.0.0",
                "custom-node": "npm:node@runtime:22.0.0",
                "is-positive": "1.0.0",
                "linked": "link:packages/sub",
                "node": "runtime:22.0.0",
            },
            "optionalDependencies": {
                "opt": "3.0.0",
            },
        }),
    );
}

/// A named-registry spec that names no package names the alias, so the gate
/// does not reuse another package recorded under the alias.
#[test]
fn locked_child_admits_a_bare_named_registry_spec_only_for_the_alias_package() {
    let context = prefixed_registry_context();
    let alias: PkgName = "foo".parse().expect("parse alias");
    let other: PkgNameVerPeer = "bar@work:1.2.0".parse().expect("parse key");
    assert_eq!(super::locked_child_admits(&alias, &other, "work:^1.0.0", &context), Some(false));
    assert_eq!(super::locked_child_admits(&alias, &other, "work:bar@^1.0.0", &context), Some(true));
    let own: PkgNameVerPeer = "foo@work:1.2.0".parse().expect("parse key");
    assert_eq!(super::locked_child_admits(&alias, &own, "work:^1.0.0", &context), Some(true));
}
