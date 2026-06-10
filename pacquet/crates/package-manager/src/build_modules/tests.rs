use super::{AllowBuildPolicy, BuildModules, parse_name_version_from_key};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pacquet_config::Config;
use pacquet_executor::ScriptsPrependNodePath;
use pacquet_lockfile::{
    PackageKey, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotEntry,
};
use pacquet_reporter::{IgnoredScriptsLog, LogEvent, Reporter, SilentReporter};
#[cfg(unix)]
use pacquet_reporter::{SkippedOptionalPackage, SkippedOptionalReason};
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tempfile::tempdir;

/// Build an [`AllowBuildPolicy`] from a list of `(spec, allowed)`
/// pairs, mirroring how `pnpm-workspace.yaml`'s `allowBuilds` map
/// would arrive at the policy. Each spec is parsed through
/// [`AllowBuildPolicy::from_config`] so version unions and depPath
/// keys work the same way they do at runtime.
/// Panics on any parse failure — test inputs must be valid.
fn policy_from_specs<const LEN: usize>(
    entries: [(&str, bool); LEN],
    dangerously_allow_all: bool,
) -> AllowBuildPolicy {
    let mut config = Config::new();
    config.dangerously_allow_all_builds = dangerously_allow_all;
    for (spec, value) in entries {
        config.allow_builds.insert(spec.to_string(), value);
    }
    AllowBuildPolicy::from_config(&config).expect("valid specs")
}

#[test]
fn parse_scoped_key() {
    let (name, version) = parse_name_version_from_key("/@pnpm.e2e/install-script-example@1.0.0");
    assert_eq!(name, "@pnpm.e2e/install-script-example");
    assert_eq!(version, "1.0.0");
}

#[test]
fn parse_unscoped_key() {
    let (name, version) = parse_name_version_from_key("/is-positive@1.0.0");
    assert_eq!(name, "is-positive");
    assert_eq!(version, "1.0.0");
}

#[test]
fn parse_key_without_leading_slash() {
    let (name, version) = parse_name_version_from_key("express@4.18.1");
    assert_eq!(name, "express");
    assert_eq!(version, "4.18.1");
}

// Policy-logic tests below mirror upstream `building/policy/test/index.ts`
// (`https://github.com/pnpm/pnpm/blob/80037699fb/building/policy/test/index.ts`).
// They drive `AllowBuildPolicy::new` directly with in-memory rule maps so the
// policy logic stays decoupled from the manifest reader, exactly the way
// upstream tests drive `createAllowBuildFunction(opts)` decoupled from
// `Config` parsing.

#[test]
fn default_policy_denies_all() {
    let policy = AllowBuildPolicy::default();
    assert_eq!(policy.check("any-package@1.0.0"), None);
}

#[test]
fn explicit_allow() {
    let policy = policy_from_specs([("@pnpm.e2e/install-script-example", true)], false);
    assert_eq!(policy.check("@pnpm.e2e/install-script-example@1.0.0"), Some(true));
}

#[test]
fn explicit_allow_requires_registry_style_dep_path() {
    let policy = policy_from_specs([("@pnpm.e2e/install-script-example", true)], false);
    assert_eq!(
        policy.check("@pnpm.e2e/install-script-example@git+https://example.com/x.git#abc123"),
        None,
    );
    assert_eq!(policy.check("@pnpm.e2e/install-script-example@1.0.0"), Some(true));
}

#[test]
fn explicit_allow_by_dep_path_allows_untrusted_package_identity() {
    let policy = policy_from_specs(
        [("foo@git+https://github.com/org/foo.git#abc123", true), ("foo", true)],
        false,
    );
    assert_eq!(
        policy.check("foo@git+https://github.com/org/foo.git#abc123(react@19.0.0)"),
        Some(true),
    );
    assert_eq!(policy.check("foo@git+https://github.com/attacker/foo.git#abc123"), None);
}

#[test]
fn explicit_allow_by_tarball_dep_path_allows_untrusted_package_identity() {
    let policy = policy_from_specs([("foo@https://example.com/foo.tgz", true)], false);

    assert_eq!(policy.check("foo@https://example.com/foo.tgz"), Some(true));
}

#[test]
fn explicit_deny() {
    let policy = policy_from_specs([("@pnpm.e2e/bad-package", false)], false);
    assert_eq!(policy.check("@pnpm.e2e/bad-package@1.0.0"), Some(false));
}

#[test]
fn unlisted_returns_none() {
    let policy = policy_from_specs([("@pnpm.e2e/allowed", true)], false);
    assert_eq!(policy.check("@pnpm.e2e/not-listed@1.0.0"), None);
}

/// Upstream checks `expandedDisallowed` before `expandedAllowed`
/// in [`createAllowBuildFunction`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/src/index.ts#L36-L43),
/// so a bare-name disallow wins over an exact-version allow.
/// Pacquet matches that order — pre-[#397]-item-5, the matcher
/// checked exact-version first, which diverged from upstream.
///
/// [#397]: https://github.com/pnpm/pacquet/issues/397
#[test]
fn disallow_bare_name_wins_over_allow_exact_version() {
    let policy =
        policy_from_specs([("@pnpm.e2e/pkg@1.0.0", true), ("@pnpm.e2e/pkg", false)], false);
    assert_eq!(policy.check("@pnpm.e2e/pkg@1.0.0"), Some(false));
    assert_eq!(policy.check("@pnpm.e2e/pkg@2.0.0"), Some(false));
}

/// The converse: a bare-name allow combined with an exact-version
/// disallow → the disallow on `pkg@1.0.0` fires only for that
/// version; other versions hit the bare-name allow.
#[test]
fn disallow_exact_version_with_allow_bare_name() {
    let policy =
        policy_from_specs([("@pnpm.e2e/pkg", true), ("@pnpm.e2e/pkg@1.0.0", false)], false);
    assert_eq!(policy.check("@pnpm.e2e/pkg@1.0.0"), Some(false));
    assert_eq!(policy.check("@pnpm.e2e/pkg@2.0.0"), Some(true));
}

#[test]
fn empty_rules_denies_all() {
    let policy = policy_from_specs([], false);
    assert_eq!(policy.check("any-package@1.0.0"), None);
}

#[test]
fn dangerously_allow_all_builds() {
    let policy = policy_from_specs([], true);
    assert_eq!(policy.check("any-package@1.0.0"), Some(true));
    assert_eq!(policy.check("other-package@2.0.0"), Some(true));
}

#[test]
fn dangerously_allow_all_allows_artifact_dep_paths() {
    let policy = policy_from_specs([], true);
    assert_eq!(policy.check("anything@git+https://example.com/x.git#abc123"), Some(true));
}

#[test]
fn dangerously_allow_all_overrides_deny() {
    let policy = policy_from_specs([("@pnpm.e2e/pkg", false)], true);
    assert_eq!(policy.check("@pnpm.e2e/pkg@1.0.0"), Some(true));
}

/// Mirrors upstream's
/// [`'should allowBuilds with true value'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/test/index.ts#L5-L15)
/// — version unions expand into separate exact-version allows.
/// `qar@1.0.0 || 2.0.0` allows exactly those two versions, leaves
/// other versions unlisted (`None`).
#[test]
fn allow_via_version_union() {
    let policy = policy_from_specs([("foo", true), ("qar@1.0.0 || 2.0.0", true)], false);
    assert_eq!(policy.check("foo@1.0.0"), Some(true));
    assert_eq!(policy.check("bar@1.0.0"), None);
    assert_eq!(policy.check("qar@1.0.0"), Some(true));
    assert_eq!(policy.check("qar@2.0.0"), Some(true));
    assert_eq!(policy.check("qar@1.1.0"), None);
}

/// Mirrors upstream's
/// [`'should not allow patterns in allowBuilds'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/test/index.ts#L28-L34)
/// — wildcards in `allowBuilds` keys are accepted by the parser
/// (they land as literal strings in the expanded set) but the
/// `HashSet::contains` lookup means they never match a real
/// package name. Use `dangerouslyAllowAllBuilds` for blanket
/// allow.
#[test]
fn wildcard_name_in_allow_builds_does_not_match_real_package() {
    let policy = policy_from_specs([("is-*", true)], false);
    assert_eq!(policy.check("is-odd@1.0.0"), None);
    assert_eq!(policy.check("is-positive@1.0.0"), None);
}

/// `from_config` propagates `expand_package_version_specs` errors —
/// an invalid version union in `Config.allow_builds` surfaces as
/// `ERR_PNPM_INVALID_VERSION_UNION` rather than silently dropping
/// the rule.
#[test]
fn from_config_propagates_invalid_version_union() {
    let mut config = Config::new();
    config.allow_builds.insert("foo@not-a-version".to_string(), true);
    let err = AllowBuildPolicy::from_config(&config).expect_err("must reject");
    assert!(matches!(err, crate::VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");
}

#[test]
fn from_config_propagates_name_pattern_in_version_union() {
    let mut config = Config::new();
    config.allow_builds.insert("foo*@1.0.0".to_string(), true);
    let err = AllowBuildPolicy::from_config(&config).expect_err("must reject");
    assert!(
        matches!(err, crate::VersionPolicyError::NamePatternInVersionUnion { .. }),
        "got: {err:?}",
    );
}

// The next two tests exercise `from_config` end-to-end: an empty Config
// folds to the default policy (deny everything), and a Config populated by
// `pnpm-workspace.yaml` round-trips through the same logic the in-memory
// tests above cover. The `package.json` reader was removed in pacquet
// pnpm/pacquet#397 item 5 — settings come from `pnpm-workspace.yaml` only.

#[test]
fn empty_config_denies_all() {
    let policy = AllowBuildPolicy::from_config(&Config::new()).expect("empty config never errors");
    assert_eq!(policy.check("anything@1.0.0"), None);
}

#[test]
fn from_config_consumes_allow_builds_and_dangerously_allow_all_builds() {
    let mut config = Config::new();
    config.dangerously_allow_all_builds = false;
    config.allow_builds.insert("@pnpm.e2e/install-script-example".to_string(), true);
    config.allow_builds.insert("@pnpm.e2e/bad-package".to_string(), false);

    let policy = AllowBuildPolicy::from_config(&config).expect("valid specs");
    assert_eq!(policy.check("@pnpm.e2e/install-script-example@1.0.0"), Some(true));
    assert_eq!(policy.check("@pnpm.e2e/bad-package@1.0.0"), Some(false));
    assert_eq!(policy.check("@pnpm.e2e/unrelated@1.0.0"), None);
}

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

/// Materialize a `<virtual_store_dir>/<store_name>/node_modules/<pkg_name>/package.json`
/// fixture so `pkg_requires_build` returns true for `key`. The script body
/// is harmless (`true`) — the existing tests rely on the policy gate to block
/// execution, so the script never actually runs.
fn create_buildable_pkg(virtual_store_dir: &Path, key: &PackageKey) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    let manifest = serde_json::json!({
        "scripts": { "postinstall": "true" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}

fn root_importers(deps: &[(&str, &str)]) -> HashMap<String, ProjectSnapshot> {
    let map: ResolvedDependencyMap = deps
        .iter()
        .map(|(n, v)| {
            (
                name(n),
                ResolvedDependencySpec { specifier: (*v).to_string(), version: ver(v).into() },
            )
        })
        .collect();
    HashMap::from([(
        ".".to_string(),
        ProjectSnapshot {
            specifiers: None,
            dependencies: (!map.is_empty()).then_some(map),
            optional_dependencies: None,
            dev_dependencies: None,
            dependencies_meta: None,
            publish_directory: None,
        },
    )])
}

/// Default-deny: a buildable package not listed in `allowBuilds` lands in
/// the returned ignored set, sorted lexically. "Buildable" is computed from
/// the extracted package directory (postinstall in package.json), matching
/// upstream's `pkgRequiresBuild`.
#[test]
fn build_modules_collects_ignored_builds() {
    let snapshots = HashMap::from([
        (key("zzz", "1.0.0"), SnapshotEntry::default()),
        (key("aaa", "2.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("zzz", "1.0.0"), ("aaa", "2.0.0")]);
    let policy = AllowBuildPolicy::default(); // empty → default-deny

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("aaa", "2.0.0"));

    let ignored = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        importers: &importers,
        packages: None,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("run BuildModules");
    dbg!(&ignored);

    assert_eq!(
        ignored,
        vec!["aaa@2.0.0".to_string(), "zzz@1.0.0".to_string()],
        "ignored set must be sorted lexicographically: {ignored:?}",
    );
}

/// Parallel-path variant of [`build_modules_collects_ignored_builds`]
/// running under `child_concurrency: 2` so the rayon
/// `chunk.par_iter().try_for_each(...)` dispatch is actually
/// exercised. The other `BuildModules` tests all run with
/// `child_concurrency: 1`, which is the sequential codepath; this
/// test pins the concurrent codepath against a fixture that places
/// two policy-denied build candidates in the same chunk (no
/// dependency edges between them, so the topo sort puts them both
/// in chunk 0).
///
/// The assertion is the same sorted ignored-set as the sequential
/// test — a regression that dropped the `pool.install` /
/// `try_for_each` wrapping would still collect both names but in
/// non-deterministic order on insertion. The
/// `BTreeSet`-backed `ignored_builds` ordering hides that, so
/// breakage would more likely show up as a lock contention bug
/// (e.g. dropping the `Mutex` wrapping) which would manifest as a
/// rustc / clippy error rather than a runtime failure; this test
/// at least documents that the codepath is exercised.
#[test]
fn build_modules_collects_ignored_builds_under_concurrency() {
    let snapshots = HashMap::from([
        (key("zzz", "1.0.0"), SnapshotEntry::default()),
        (key("aaa", "2.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("zzz", "1.0.0"), ("aaa", "2.0.0")]);
    let policy = AllowBuildPolicy::default();

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("aaa", "2.0.0"));

    let ignored = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        importers: &importers,
        packages: None,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 2,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("run BuildModules under concurrency");
    dbg!(&ignored);

    // Same expected output as the sequential test — both members of
    // the same chunk insert into the `Mutex<BTreeSet>` concurrently
    // and the BTreeSet's iteration order normalizes the result.
    assert_eq!(
        ignored,
        vec!["aaa@2.0.0".to_string(), "zzz@1.0.0".to_string()],
        "ignored set must be the same under concurrency 2 as under 1: {ignored:?}",
    );
}

/// Explicit `false` in `allowBuilds` is silently skipped — it does NOT
/// land in the ignored-scripts list. Mirrors upstream
/// `building/during-install/src/index.ts:91-93`.
#[test]
fn build_modules_excludes_explicit_deny_from_ignored() {
    let snapshots = HashMap::from([
        (key("denied", "1.0.0"), SnapshotEntry::default()),
        (key("ignored", "1.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("denied", "1.0.0"), ("ignored", "1.0.0")]);

    let policy = policy_from_specs([("denied", false)], false);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("denied", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("ignored", "1.0.0"));

    let ignored = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        importers: &importers,
        packages: None,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("run BuildModules");
    dbg!(&ignored);

    assert_eq!(
        ignored,
        vec!["ignored@1.0.0".to_string()],
        "explicit-false must NOT appear in ignored set: {ignored:?}",
    );
}

/// Optional dep whose postinstall fails must be reported through the
/// `pnpm:skipped-optional-dependency` channel (reason `build_failure`)
/// and NOT abort the install. Mirrors upstream
/// `building/during-install/src/index.ts:218-240` and the spirit of
/// `'do not fail on an optional dependency that has a non-optional
/// dependency with a failing postinstall script'` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/test/install/optionalDependencies.ts#L563-L572>.
///
/// The test uses the upstream fixture `@pnpm.e2e/failing-postinstall@1.0.0`
/// (script body verbatim from `/Volumes/src/pnpm/registry-mock/packages/failing-postinstall/package.json`)
/// so the failure mode is exactly the one upstream's optional-dep
/// tests exercise.
///
/// Unix-gated because the upstream script (`echo hello && echo world && exit 1`)
/// is POSIX shell syntax. The cmd-on-Windows path picks a different
/// shell — `pacquet_executor::select_shell` (tested in the executor
/// crate's `shell::tests`) covers the shell-selection branches in
/// isolation.
#[cfg(unix)]
#[test]
fn do_not_fail_on_optional_dep_with_failing_postinstall() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    let optional_snapshot = SnapshotEntry { optional: true, ..Default::default() };
    let snapshots = HashMap::from([(pkg_key.clone(), optional_snapshot)]);
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    // `dangerouslyAllowAllBuilds` so the policy lets the failing
    // script through to actually run — this test exercises the
    // build-failure path, not the policy gate.
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    let ignored = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        importers: &importers,
        packages: None,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<RecordingReporter>()
    .expect("optional build failure must NOT abort the install");
    dbg!(&ignored);

    let captured = EVENTS.lock().expect("lock").clone();
    dbg!(&captured);
    let skipped_event = captured
        .iter()
        .find_map(|event| match event {
            LogEvent::SkippedOptionalDependency(log) => Some(log),
            _ => None,
        })
        .expect("must emit pnpm:skipped-optional-dependency");
    assert_eq!(skipped_event.reason, SkippedOptionalReason::BuildFailure);
    let SkippedOptionalPackage::Installed { name, version, .. } = &skipped_event.package else {
        panic!("expected Installed payload for build_failure, got {:?}", skipped_event.package);
    };
    assert_eq!(name, "@pnpm.e2e/failing-postinstall");
    assert_eq!(version, "1.0.0");
    assert!(skipped_event.details.is_some(), "details must carry the error toString");
}

/// Ports the upstream `'using side effects cache'` test at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/test/install/sideEffects.ts#L79-L131>.
///
/// Upstream runs the install twice — first to populate the cache
/// via the WRITE path, then to consume it. Pacquet doesn't have a
/// WRITE path yet ([#421]'s slice (B)), so we hand-craft the same
/// state directly: a `side_effects_maps_by_snapshot` entry whose
/// cache key matches what `BuildModules` will compute via
/// `calc_dep_state`. With that in place, the gate skips the build
/// even though the package's `postinstall` would have failed —
/// observable via the absence of a `pnpm:lifecycle` event for the
/// stage.
///
/// The fixture (`@pnpm.e2e/failing-postinstall@1.0.0`, `postinstall:
/// echo hello && echo world && exit 1`) is the same upstream's
/// own tests use; if the gate were broken the build would run and
/// the install would propagate the exit-1 failure (cf.
/// `fail_when_failing_postinstall_is_required` below).
///
/// [#421]: https://github.com/pnpm/pacquet/issues/421
#[cfg(unix)]
#[test]
fn using_side_effects_cache_skips_rebuild() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pacquet_lockfile::PackageKey, pacquet_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pacquet_lockfile::PackageMetadata {
                resolution: pacquet_lockfile::LockfileResolution::Registry(
                    pacquet_lockfile::RegistryResolution {
                        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                            .parse()
                            .expect("parse integrity"),
                    },
                ),
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
            },
        )]);
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    // Compute the cache key the same way `BuildModules` will, then
    // pre-populate `side_effects_maps_by_snapshot` with a matching
    // entry. The inner FilesMap value is irrelevant for this
    // assertion — only presence of the key matters for the gate.
    let engine = "darwin;arm64;node20";
    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let mut state_cache = pacquet_graph_hasher::DepsStateCache::new();
    let expected_cache_key = pacquet_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut state_cache,
        &pkg_key,
        &pacquet_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            patch_file_hash: None,
            include_dep_graph_hash: true,
        },
    );
    let mut overlay = std::collections::HashMap::new();
    overlay.insert(expected_cache_key, std::collections::HashMap::new());
    let mut side_effects_maps = std::collections::HashMap::new();
    side_effects_maps.insert(pkg_key.clone(), std::sync::Arc::new(overlay));

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: Some(&side_effects_maps),
        engine_name: Some(engine),
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<RecordingReporter>()
    .expect("install must succeed when the cache hit skips the rebuild");

    // The build was skipped, so no `pnpm:lifecycle` event for the
    // postinstall stage should have been emitted. If the gate were
    // broken the failing-postinstall script would have run and
    // emitted a `Script` (and a non-zero `Exit`) event, plus
    // returned `Err(BuildModulesError::LifecycleScript(...))` from
    // `.run()`.
    let captured = EVENTS.lock().expect("lock").clone();
    let any_lifecycle = captured.iter().any(|e| matches!(e, LogEvent::Lifecycle(_)));
    assert!(!any_lifecycle, "side-effects cache hit must skip lifecycle scripts: {captured:#?}");
}

/// Negative pair: with `side_effects_cache = false`, even a
/// matching cache entry is ignored — the build runs. Mirrors
/// upstream's `sideEffectsCache: false` config branch.
#[cfg(unix)]
#[test]
fn side_effects_cache_disabled_bypasses_the_gate() {
    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pacquet_lockfile::PackageKey, pacquet_lockfile::PackageMetadata> =
        HashMap::new();
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    // Same overlay shape as the positive test, but the
    // `side_effects_cache: false` flag must short-circuit before
    // the lookup even runs.
    let mut overlay = std::collections::HashMap::new();
    overlay.insert("any-key".to_string(), std::collections::HashMap::new());
    let mut side_effects_maps = std::collections::HashMap::new();
    side_effects_maps.insert(pkg_key, std::sync::Arc::new(overlay));

    let err = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: Some(&side_effects_maps),
        engine_name: Some("darwin;arm64;node20"),
        side_effects_cache: false,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect_err("with cache disabled, the failing postinstall must run and the install must fail");
    assert!(matches!(err, crate::build_modules::BuildModulesError::LifecycleScript(_)));
}

/// Mirrors `'fail on a package with failing postinstall if the
/// package is both an optional and non-optional dependency'` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/test/install/optionalDependencies.ts#L574-L591>.
///
/// Upstream's resolver folds reachability ALL-paths-optional, so a
/// package reachable through any non-optional edge has
/// `snapshots[...].optional = false` in the lockfile (cf.
/// `installing/deps-resolver/src/resolveDependencies.ts:1605-1610`).
/// `BuildModules` then propagates the build failure rather than
/// swallowing it. Pacquet trusts the precomputed flag; this test
/// pins the propagation branch by supplying the same fixture with
/// `optional: false`, which is the lockfile shape upstream produces
/// for the dual-reachability case.
#[cfg(unix)]
#[test]
fn fail_when_failing_postinstall_is_required() {
    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    // `optional: false` — pacquet's analog of upstream's
    // ALL-paths-optional fold concluding the dep is required.
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    let err = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        importers: &importers,
        packages: None,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: None,
        store_index_writer: None,
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect_err("required build failure must propagate");
    eprintln!("ERR: {err}");
    assert!(matches!(err, crate::build_modules::BuildModulesError::LifecycleScript(_)));
}

/// Materialize a package fixture whose contents are byte-identical
/// to upstream's `@pnpm.e2e/failing-postinstall@1.0.0` at
/// `/Volumes/src/pnpm/registry-mock/packages/failing-postinstall/package.json`.
/// Reusing the upstream script body (`echo hello && echo world && exit 1`)
/// keeps the failure mode and exit code identical to what
/// `optionalDependencies.ts` exercises against the live mock
/// registry, without dragging the lockfile-with-real-integrity
/// machinery into a `BuildModules`-unit test.
#[cfg(unix)]
fn create_failing_postinstall_fixture(virtual_store_dir: &Path, key: &PackageKey) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    let manifest = serde_json::json!({
        "name": pkg_name,
        "version": name_version[at_idx + 1..].to_string(),
        "scripts": { "postinstall": "echo hello && echo world && exit 1" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}

/// Recording fake confirms `pnpm:ignored-scripts` is the right channel
/// for the package list. The frozen-install path emits this once after
/// `BuildModules::run` returns; this test exercises the equivalent
/// emit shape directly so `LogEvent::IgnoredScripts` stays connected
/// to the `BuildModules` return value.
#[test]
fn ignored_scripts_event_carries_returned_names() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let names = vec!["a@1.0.0".to_string(), "b@2.0.0".to_string()];
    RecordingReporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: pacquet_reporter::LogLevel::Debug,
        package_names: names.clone(),
    }));

    let captured = EVENTS.lock().expect("lock").clone();
    dbg!(&captured);
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::IgnoredScripts(IgnoredScriptsLog { package_names, .. })]
                if package_names == &names,
        ),
        "captured: {captured:?}",
    );
}

/// Materialize a package fixture whose postinstall touches a
/// marker file. After the script runs, the package directory has
/// a file (`generated.txt`) that the original tarball didn't, so
/// the WRITE-path diff produces a non-empty `added` entry under
/// the snapshot's cache key.
///
/// Returns the package directory path and the actual file mode of
/// `index.js`. The mode is read from disk because `fs::write()`
/// assigns permissions according to the process umask (typically
/// 0022 → 0o644, but 0002 → 0o664 when `pam_umask`'s `usergroups`
/// logic matches UID to group name, the default on Debian for
/// non-root users). Callers that pre-seed store rows should use
/// this returned mode so `calculate_diff()` doesn't flag a
/// mode-only mismatch on unchanged files.
#[cfg(unix)]
fn create_postinstall_modifies_source_fixture(
    virtual_store_dir: &Path,
    key: &PackageKey,
) -> (PathBuf, u32) {
    use std::os::unix::fs::PermissionsExt;

    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    // Bake the pristine `index.js` into the directory before the
    // postinstall runs. The WRITE-path diff compares the
    // post-build directory against the pre-seeded `files` map,
    // so this file must appear in both — the postinstall only
    // adds `generated.txt` on top.
    fs::write(pkg_dir.join("index.js"), "module.exports = 'hi'\n").expect("write index.js");
    let manifest = serde_json::json!({
        "name": pkg_name,
        "version": name_version[at_idx + 1..].to_string(),
        "scripts": { "postinstall": "echo touched > generated.txt" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    let actual_mode =
        std::fs::metadata(pkg_dir.join("index.js")).expect("stat index.js").permissions().mode()
            & 0o777;
    (pkg_dir, actual_mode)
}

/// Mirrors upstream's `'a postinstall script does not modify the
/// original sources added to the store'` at
/// <https://github.com/pnpm/pnpm/blob/7e3145f9fc/installing/deps-installer/test/install/sideEffects.ts#L189-L223>.
///
/// After a successful postinstall, `BuildModules` re-CAFS the
/// built directory, diffs against the pristine `PackageFilesIndex.files`
/// row pre-seeded in the store, and queues a mutation so the row's
/// `side_effects[cache_key]` carries the post-build files that
/// differ from the base. The base CAS blob is left untouched — the
/// digest the store-index row holds for `index.js` matches the
/// pristine content, not the post-build content.
///
/// Unix-gated because the fixture uses `sh -c` semantics for the
/// `postinstall` script. Windows shell selection is exercised
/// separately by `pacquet_executor::select_shell`.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn write_path_populates_side_effects_row() {
    use pacquet_store_dir::{
        CafsFileInfo, HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter,
        store_index_key,
    };

    let pkg_key = key("@pnpm/postinstall-modifies-source", "1.0.0");
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pacquet_lockfile::PackageKey, pacquet_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pacquet_lockfile::PackageMetadata {
                resolution: pacquet_lockfile::LockfileResolution::Registry(
                    pacquet_lockfile::RegistryResolution {
                        integrity: integrity_str.parse().expect("parse integrity"),
                    },
                ),
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
            },
        )]);
    let importers = root_importers(&[("@pnpm/postinstall-modifies-source", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");
    let (_pkg_dir, actual_mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);

    // Pre-seed the base PackageFilesIndex row that the WRITE
    // path will mutate. The base captures only `index.js`; the
    // postinstall creates `generated.txt` on top, so the diff's
    // `added` map should have exactly the `generated.txt` entry.
    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let mut base_files = HashMap::new();
    base_files.insert(
        "index.js".to_string(),
        CafsFileInfo {
            // The pristine content's actual digest is irrelevant
            // for this test — the WRITE path doesn't compare it
            // against on-disk CAS, just against the post-build
            // hashes from `add_files_from_dir`. So long as it
            // matches what `add_files_from_dir` will compute for
            // `module.exports = 'hi'\n`, the diff for `index.js`
            // stays empty (= no spurious entry in `added`).
            digest: sha512_hex(b"module.exports = 'hi'\n"),
            mode: actual_mode,
            size: b"module.exports = 'hi'\n".len() as u64,
            checked_at: None,
        },
    );
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        algo: HASH_ALGORITHM.to_string(),
        files: base_files,
        side_effects: None,
    };
    {
        let mut index = StoreIndex::open_in(&store_dir).expect("open index for seed");
        index
            .set_many(std::iter::once((files_index_file.clone(), base_row)))
            .expect("seed base row");
    }

    // Spawn the writer task once we've seeded the base row, so
    // the seed and the WRITE-path mutation don't race.
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    let engine = "darwin;arm64;node20";
    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let mut state_cache = pacquet_graph_hasher::DepsStateCache::new();
    let expected_cache_key = pacquet_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut state_cache,
        &pkg_key,
        &pacquet_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            patch_file_hash: None,
            include_dep_graph_hash: true,
        },
    );

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: Some(engine),
        side_effects_cache: true,
        side_effects_cache_write: true,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    // Drop our writer handle and wait for the task to flush the
    // queued WRITE-path mutation before reading the row back.
    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("row present");
    let side_effects = row.side_effects.expect("side_effects populated");
    let diff = side_effects.get(&expected_cache_key).expect("entry for cache key");
    let added = diff.added.as_ref().expect("added present");
    assert!(
        added.contains_key("generated.txt"),
        "added map should record the postinstall-created file: {added:?}",
    );
    assert!(
        !added.contains_key("index.js"),
        "pristine index.js must NOT appear in `added` (its digest matches base): {added:?}",
    );
}

/// Counterpart of the WRITE-path test: with `side_effects_cache_write
/// = false`, the same fixture's row must come out of `BuildModules`
/// with `side_effects = None`. Mirrors upstream's gate on
/// `opts.sideEffectsCacheWrite` at `building/during-install/src/index.ts:198`.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn write_path_disabled_skips_upload() {
    use pacquet_store_dir::{
        HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter, store_index_key,
    };

    let pkg_key = key("@pnpm/postinstall-modifies-source", "1.0.0");
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pacquet_lockfile::PackageKey, pacquet_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pacquet_lockfile::PackageMetadata {
                resolution: pacquet_lockfile::LockfileResolution::Registry(
                    pacquet_lockfile::RegistryResolution {
                        integrity: integrity_str.parse().expect("parse integrity"),
                    },
                ),
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
            },
        )]);
    let importers = root_importers(&[("@pnpm/postinstall-modifies-source", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    let (_pkg_dir, _actual_mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);

    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        algo: HASH_ALGORITHM.to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    {
        let mut index = StoreIndex::open_in(&store_dir).expect("open index for seed");
        index
            .set_many(std::iter::once((files_index_file.clone(), base_row)))
            .expect("seed base row");
    }
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: Some("darwin;arm64;node20"),
        side_effects_cache: true,
        side_effects_cache_write: false,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("row present");
    assert!(row.side_effects.is_none(), "write disabled must NOT populate side_effects");
}

/// Mirrors upstream's `'uploading errors do not interrupt
/// installation'` at <https://github.com/pnpm/pnpm/blob/7e3145f9fc/installing/deps-installer/test/install/sideEffects.ts#L166-L186>.
///
/// Upstream stubs `opts.storeController.upload` to throw and
/// asserts the install completes (the postinstall ran, the
/// generated file is on disk) but the `SQLite` row's `side_effects`
/// stays empty.
///
/// Pacquet has no DI seam for the upload, but the WRITE path's
/// only failure point is `add_files_from_dir`, which surfaces as
/// `UploadError::AddFilesFromDir`. We force that failure by having
/// the postinstall script create a 0-permission file in the
/// package directory: `add_files_from_dir` then fails to `fs::read`
/// it, returning an error that `BuildModules` swallows with
/// `tracing::warn!` (matching upstream's `try { … } catch { logger.warn }`).
/// The install completes, the postinstall-generated artifact is on
/// disk, and the build keeps going.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn upload_error_does_not_interrupt_install() {
    use pacquet_store_dir::{
        HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter, store_index_key,
    };

    let pkg_key = key("@pnpm/postinstall-modifies-source", "1.0.0");
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pacquet_lockfile::PackageKey, pacquet_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pacquet_lockfile::PackageMetadata {
                resolution: pacquet_lockfile::LockfileResolution::Registry(
                    pacquet_lockfile::RegistryResolution {
                        integrity: integrity_str.parse().expect("parse integrity"),
                    },
                ),
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
            },
        )]);
    let importers = root_importers(&[("@pnpm/postinstall-modifies-source", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    // Fixture variant: postinstall produces a regular file (to
    // prove the script ran end-to-end) AND a 0-permission file
    // (to force `add_files_from_dir` to fail on `fs::read`).
    let pkg_dir = create_postinstall_with_unreadable_fixture(virtual_store_dir.path(), &pkg_key);

    // Pre-seed a base row so we can assert that the swallowed
    // upload error leaves the row's `side_effects` field
    // untouched — matches upstream's `filesIndex2.sideEffects
    // toBeFalsy()` at sideEffects.ts:186.
    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        algo: HASH_ALGORITHM.to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    {
        let mut index = StoreIndex::open_in(&store_dir).expect("open index for seed");
        index
            .set_many(std::iter::once((files_index_file.clone(), base_row)))
            .expect("seed base row");
    }

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: Some("darwin;arm64;node20"),
        side_effects_cache: true,
        side_effects_cache_write: true,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: None,

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("upload failure must not propagate; install continues");

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    // The postinstall-generated artifact is on disk — proves the
    // build ran end-to-end and the swallowed upload error didn't
    // short-circuit the loop.
    assert!(
        pkg_dir.join("generated.txt").exists(),
        "postinstall-created file must be present after a swallowed upload failure",
    );

    // The base row stays untouched: the `add_files_from_dir`
    // error fired before `queue_side_effects_upload` ran, so the
    // writer task never saw a `SideEffectsUpload` for this row.
    // Mirrors upstream's `filesIndex2.sideEffects toBeFalsy()` at
    // sideEffects.ts:186.
    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("base row present");
    assert!(
        row.side_effects.is_none(),
        "swallowed upload error must leave `side_effects` unmodified",
    );

    // Restore perms so the tempdir cleanup can remove the file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(pkg_dir.join("unreadable"), fs::Permissions::from_mode(0o644));
    }
}

/// Variant of `create_postinstall_modifies_source_fixture` whose
/// postinstall additionally produces a 0-permission file. The
/// WRITE-path walker (`add_files_from_dir`) then fails on
/// `fs::read("unreadable")` with `EACCES`, surfacing as
/// `UploadError::AddFilesFromDir(ReadFile { … })` — a real upload
/// error that `BuildModules` must swallow.
#[cfg(unix)]
fn create_postinstall_with_unreadable_fixture(
    virtual_store_dir: &Path,
    key: &PackageKey,
) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("index.js"), "module.exports = 'hi'\n").expect("write index.js");
    let manifest = serde_json::json!({
        "name": pkg_name,
        "version": name_version[at_idx + 1..].to_string(),
        "scripts": {
            "postinstall": "echo touched > generated.txt && : > unreadable && chmod 000 unreadable"
        },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}

/// sha-512 hex helper for fixture-building. Pacquet's `CafsFileInfo`
/// stores digests as raw hex (no `sha512-` prefix); using the same
/// shape here keeps the test's pre-seeded base row in lockstep with
/// what `add_files_from_dir` will compute.
#[cfg(unix)]
fn sha512_hex(buf: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    let digest = Sha512::digest(buf);
    format!("{digest:x}")
}

/// When `BuildModules.patches` contains an entry for a snapshot,
/// the side-effects-cache key computed for that snapshot must
/// include the `;patch=<hash>` segment that
/// [`pacquet_graph_hasher::CalcDepStateOptions::patch_file_hash`]
/// appends.
///
/// Drive this through the WRITE path so the test can read the
/// cache key back out of the persisted row — the key shape is
/// the contract upstream relies on
/// (<https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L199-L204>).
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn write_path_cache_key_includes_patch_hash() {
    use pacquet_patching::ExtendedPatchInfo;
    use pacquet_store_dir::{
        CafsFileInfo, HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter,
        store_index_key,
    };

    let pkg_key = key("@pnpm/postinstall-modifies-source", "1.0.0");
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pacquet_lockfile::PackageKey, pacquet_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pacquet_lockfile::PackageMetadata {
                resolution: pacquet_lockfile::LockfileResolution::Registry(
                    pacquet_lockfile::RegistryResolution {
                        integrity: integrity_str.parse().expect("parse integrity"),
                    },
                ),
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
            },
        )]);
    let importers = root_importers(&[("@pnpm/postinstall-modifies-source", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");
    let (_pkg_dir, actual_mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);

    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let mut base_files = HashMap::new();
    base_files.insert(
        "index.js".to_string(),
        CafsFileInfo {
            digest: sha512_hex(b"module.exports = 'hi'\n"),
            mode: actual_mode,
            size: b"module.exports = 'hi'\n".len() as u64,
            checked_at: None,
        },
    );
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        algo: HASH_ALGORITHM.to_string(),
        files: base_files,
        side_effects: None,
    };
    {
        let mut index = StoreIndex::open_in(&store_dir).expect("open index for seed");
        index
            .set_many(std::iter::once((files_index_file.clone(), base_row)))
            .expect("seed base row");
    }

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    // The applier runs against `pkg_dir` before the postinstall, so
    // it needs a real patch file that succeeds. Touch a brand-new
    // file rather than modifying `index.js` so the assertions on
    // the diff map below stay simple — the patch adds
    // `patched.txt`, the postinstall adds `generated.txt`, the
    // pristine `index.js` stays at its base digest.
    let patch_dir = tempdir().expect("create patch dir");
    let patch_file = patch_dir.path().join("foo.patch");
    fs::write(
        &patch_file,
        "\
diff --git a/patched.txt b/patched.txt
new file mode 100644
--- /dev/null
+++ b/patched.txt
@@ -0,0 +1 @@
+hello from the patch
",
    )
    .expect("write patch");

    let patch_hash = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: patch_hash.to_string(),
            patch_file_path: Some(patch_file.clone()),
            key: "@pnpm/postinstall-modifies-source@1.0.0".to_string(),
        },
    )]);

    let engine = "darwin;arm64;node20";
    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let mut state_cache = pacquet_graph_hasher::DepsStateCache::new();
    let expected_cache_key_with_patch = pacquet_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut state_cache,
        &pkg_key,
        &pacquet_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            patch_file_hash: Some(patch_hash),
            include_dep_graph_hash: true,
        },
    );
    assert!(
        expected_cache_key_with_patch.contains(";patch="),
        "sanity: graph-hasher must emit ';patch=' for the patched options: {expected_cache_key_with_patch:?}",
    );

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: Some(engine),
        side_effects_cache: true,
        side_effects_cache_write: true,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: Some(&patches),

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("row present");
    let side_effects = row.side_effects.expect("side_effects populated");
    assert!(
        side_effects.contains_key(&expected_cache_key_with_patch),
        "patched cache key must appear in side_effects map: \
         expected key {expected_cache_key_with_patch:?}, got keys {:?}",
        side_effects.keys().collect::<Vec<_>>(),
    );
}

/// A patch in the `patches` map gets applied to the extracted
/// package dir before postinstall hooks run. Mirrors the upstream
/// `simple-with-patch` fixture at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-restorer/test/fixtures/simple-with-patch/>
/// at the unit level.
///
/// Drives `BuildModules` with a single `requires_build=false`
/// snapshot (no postinstall scripts), `dangerouslyAllowAllBuilds: true`
/// (irrelevant — no scripts to allow), and a patch that creates
/// `patched.txt`. After the run, `patched.txt` must exist on disk
/// with the patch body.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn patch_only_snapshot_gets_patched_via_build_modules() {
    use pacquet_patching::ExtendedPatchInfo;
    use pacquet_store_dir::{StoreDir, StoreIndexWriter};

    let pkg_key = key("is-positive", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("is-positive", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    // Lay down a pristine `is-positive` package dir under the
    // virtual store so the applier has a real target. No scripts,
    // so `requires_build_map` for this snapshot stays false — the
    // build trigger fires solely because of the patch entry.
    let pkg_dir = virtual_store_dir.path().join("is-positive@1.0.0/node_modules/is-positive");
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), r#"{"name":"is-positive","version":"1.0.0"}"#)
        .expect("write manifest");

    // Patch that creates a brand-new file. Pure Create operation;
    // diffy parses and applies it cleanly.
    let patch_dir = tempdir().expect("create patch dir");
    let patch_file = patch_dir.path().join("is-positive.patch");
    fs::write(
        &patch_file,
        "\
diff --git a/patched.txt b/patched.txt
new file mode 100644
--- /dev/null
+++ b/patched.txt
@@ -0,0 +1 @@
+applied
",
    )
    .expect("write patch");

    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: "0".repeat(64),
            patch_file_path: Some(patch_file.clone()),
            key: "is-positive@1.0.0".to_string(),
        },
    )]);

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: None,
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: false,
        side_effects_cache_write: false,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: Some(&patches),

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let patched = pkg_dir.join("patched.txt");
    assert!(patched.exists(), "patch must have created {}", patched.display());
    assert_eq!(fs::read_to_string(&patched).unwrap(), "applied\n");
}

/// When the resolved patch entry carries a hash but no
/// `patch_file_path`, surfacing `ERR_PNPM_PATCH_FILE_PATH_MISSING`
/// is the explicit signal the user should add the package to
/// `patchedDependencies` in `pnpm-workspace.yaml`. Mirrors upstream's
/// guard at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L172-L176>.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn missing_patch_file_path_errors_with_diagnostic() {
    use pacquet_patching::ExtendedPatchInfo;
    use pacquet_store_dir::{StoreDir, StoreIndexWriter};

    let pkg_key = key("is-positive", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("is-positive", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    let pkg_dir = virtual_store_dir.path().join("is-positive@1.0.0/node_modules/is-positive");
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), r#"{"name":"is-positive","version":"1.0.0"}"#)
        .expect("write manifest");

    // `patch_file_path: None` — the lockfile-only shape where a hash
    // is known but no live config provides a file. Must surface as
    // `ERR_PNPM_PATCH_FILE_PATH_MISSING`.
    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: "0".repeat(64),
            patch_file_path: None,
            key: "is-positive@1.0.0".to_string(),
        },
    )]);

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    let err = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pacquet_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: None,
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        engine_name: None,
        side_effects_cache: false,
        side_effects_cache_write: false,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: Some(&patches),

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_root_by_key: None,
        gather_ancestor_bin_paths: false,
    }
    .run::<SilentReporter>()
    .expect_err("missing patch_file_path must surface as PatchFilePathMissing");

    drop(writer);
    let _ = writer_task.await;

    assert!(matches!(err, super::BuildModulesError::PatchFilePathMissing { .. }), "got: {err:?}");
}

// --- bin_dirs_in_all_parent_dirs ----------------------------------------

/// Top-level hoisted package: the helper must produce one entry per
/// ancestor `node_modules/.bin` from `pkg_root` up to (and including)
/// `lockfile_dir`. Mirrors upstream's
/// [`binDirsInAllParentDirs`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L476-L487):
/// a `pkgRoot` of `<lockfile>/node_modules/foo` produces three paths
/// — the package's own `node_modules/.bin`, the synthetic
/// `<modules>/node_modules/.bin` from the dirname loop step, and the
/// final `<lockfile>/node_modules/.bin` push after the loop exits.
/// Synthetic entries don't exist on disk and are harmless to PATH
/// lookup; pinning the literal list keeps pacquet byte-for-byte
/// compatible with upstream's wire output.
#[test]
fn bin_dirs_top_level_hoisted_pkg() {
    let lockfile_dir = PathBuf::from("/repo");
    let pkg_root = PathBuf::from("/repo/node_modules/foo");
    let dirs = super::bin_dirs_in_all_parent_dirs(&pkg_root, &lockfile_dir);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/repo/node_modules/foo/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/.bin"),
        ],
    );
}

/// Conflict-nested package: a package living under
/// `<lockfile>/node_modules/parent/node_modules/child` produces five
/// entries — its own bin, parent's, the synthetic `<modules>/node_modules/.bin`,
/// the synthetic intermediate (`<parent>/node_modules/node_modules/.bin`),
/// and the final lockfile-root push. The two synthetic intermediates
/// fall out of the dirname-loop's per-step push but never resolve to
/// real directories.
#[test]
fn bin_dirs_nested_hoisted_pkg() {
    let lockfile_dir = PathBuf::from("/repo");
    let pkg_root = PathBuf::from("/repo/node_modules/parent/node_modules/child");
    let dirs = super::bin_dirs_in_all_parent_dirs(&pkg_root, &lockfile_dir);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/repo/node_modules/parent/node_modules/child/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/parent/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/parent/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/.bin"),
        ],
    );
}

/// Scoped package: `<lockfile>/node_modules/@scope/pkg`. Upstream's
/// `path.dirname(dir)[0] === '@'` guard never fires when paths are
/// absolute (the dirname's leading char is `/`); we mirror that by
/// pushing every step regardless of segment shape, including the
/// scope slot's synthetic `<scope>/node_modules/.bin`. See the
/// helper's doc-comment for the upstream rationale.
#[test]
fn bin_dirs_scoped_pkg_pushes_every_step() {
    let lockfile_dir = PathBuf::from("/repo");
    let pkg_root = PathBuf::from("/repo/node_modules/@scope/pkg");
    let dirs = super::bin_dirs_in_all_parent_dirs(&pkg_root, &lockfile_dir);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/repo/node_modules/@scope/pkg/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/@scope/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/.bin"),
        ],
    );
}

// --- pkg_root_for_key ---------------------------------------------------

/// Without an override map (the isolated linker), the helper falls
/// through to `virtual_store_dir_for_key` and returns
/// `Some(<slot>/node_modules/<name>)`. Pinning the `Some` shape
/// (rather than just the path) catches a future change that turns
/// the isolated path optional too.
#[test]
fn pkg_root_for_key_isolated_uses_layout() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();
    let layout = VirtualStoreLayout::new(config, None, None, None, None);

    let key: PackageKey = "is-positive@1.0.0".parse().expect("parse key");
    let result = super::pkg_root_for_key(&layout, None, &key).expect("isolated lookup hits");

    assert!(
        result.starts_with(&config.virtual_store_dir),
        "isolated pkg_dir lives under the virtual store: {result:?}",
    );
    assert!(
        result.ends_with("node_modules/is-positive"),
        "trailing path is `node_modules/<name>`: {result:?}",
    );
}

/// With an override map (the hoisted linker), the helper returns
/// the override entry verbatim — no virtual-store layout lookup.
#[test]
fn pkg_root_for_key_hoisted_uses_override() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();
    let layout = VirtualStoreLayout::new(config, None, None, None, None);

    let key: PackageKey = "is-positive@1.0.0".parse().expect("parse key");
    let hoisted_dir = PathBuf::from("/repo/node_modules/is-positive");
    let map: HashMap<PackageKey, PathBuf> = [(key.clone(), hoisted_dir.clone())].into();

    let result = super::pkg_root_for_key(&layout, Some(&map), &key).expect("override hits");
    assert_eq!(result, hoisted_dir);
}

/// A snapshot key absent from the override map (the walker decided
/// not to record it — pre-skipped, optional skip) returns `None` so
/// `BuildModules`'s per-snapshot loop can short-circuit instead of
/// attempting to `cd` into a path the caller never produced.
#[test]
fn pkg_root_for_key_hoisted_missing_returns_none() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();
    let layout = VirtualStoreLayout::new(config, None, None, None, None);

    let key: PackageKey = "absent@1.0.0".parse().expect("parse key");
    let map: HashMap<PackageKey, PathBuf> = HashMap::new();

    let result = super::pkg_root_for_key(&layout, Some(&map), &key);
    assert!(result.is_none(), "absent key surfaces None, got {result:?}");
}
