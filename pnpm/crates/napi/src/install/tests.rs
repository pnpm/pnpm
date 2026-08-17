use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::Arc,
};

use pnpm_network::NoProxySetting;
use pnpm_testing_utils::registry::TestRegistry;

use super::{
    DepsRequiringBuildSink, EngineMode, InstallOptions, NetworkConfigInput, NodeApiProject,
    ProxyConfigInput, build_overlay, reject_non_object_manifests,
    reject_unsupported_install_options, run_install_inner, take_deps_requiring_build,
};
use crate::{
    config::{ConfigOverlay, resolve_config},
    reporter_bridge::{begin_stats, take_stats},
};

const WELL_FORMED_PATCH: &str = concat!(
    "diff --git a/patched-marker.txt b/patched-marker.txt\n",
    "new file mode 100644\n",
    "index 0000000..3f2e1d4\n",
    "--- /dev/null\n",
    "+++ b/patched-marker.txt\n",
    "@@ -0,0 +1 @@\n",
    "+patched\n",
);

#[test]
fn resolve_config_reloads_changed_workspace_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "ignoreScripts: true\n")
        .expect("write workspace yaml");
    let first = resolve_config(dir.path(), &ConfigOverlay::default()).expect("first config");
    assert!(first.ignore_scripts);

    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "ignoreScripts: false\n")
        .expect("rewrite workspace yaml");
    let second = resolve_config(dir.path(), &ConfigOverlay::default()).expect("second config");
    assert!(!second.ignore_scripts);
}

#[test]
fn build_overlay_maps_supported_install_options() {
    let mut options = install_options();
    options.external_dependencies = Some(vec!["bit-bin".to_string()]);
    options.exclude_links_from_lockfile = Some(true);
    options.inject_workspace_packages = Some(true);
    options.hoist_workspace_packages = Some(false);
    options.ignore_scripts = Some(true);
    options.engine_strict = Some(true);
    options.node_version = Some("18.20.4".to_string());
    options.minimum_release_age = Some(60);
    options.minimum_release_age_exclude = Some(vec!["left-pad".to_string()]);
    options.trust_lockfile = Some(false);
    options.network_config = Some(NetworkConfigInput {
        ca: Some(serde_json::json!(["cert-a", "cert-b"])),
        cert: Some(serde_json::json!("client-cert")),
        key: Some("client-key".to_string()),
        local_address: Some("127.0.0.1".to_string()),
        strict_ssl: Some(false),
        max_sockets: Some(7),
        network_concurrency: Some(12),
        fetch_retries: Some(4),
        fetch_retry_factor: Some(2),
        fetch_retry_mintimeout: Some(10),
        fetch_retry_maxtimeout: Some(20),
        fetch_timeout: Some(30),
        user_agent: Some("pnpm-test".to_string()),
    });
    options.proxy_config = Some(ProxyConfigInput {
        http_proxy: Some("http://proxy.test".to_string()),
        https_proxy: Some("https://proxy.test".to_string()),
        no_proxy: Some(serde_json::json!("localhost,127.0.0.1")),
    });

    let overlay = build_overlay(&options).expect("overlay");
    assert_eq!(overlay.external_dependencies.unwrap().len(), 1);
    assert_eq!(overlay.exclude_links_from_lockfile, Some(true));
    assert_eq!(overlay.inject_workspace_packages, Some(true));
    assert_eq!(overlay.hoist_workspace_packages, Some(false));
    assert_eq!(overlay.ignore_scripts, Some(true));
    assert_eq!(overlay.engine_strict, Some(true));
    assert_eq!(overlay.node_version, Some("18.20.4".to_string()));
    assert_eq!(overlay.minimum_release_age, Some(60));
    assert_eq!(overlay.minimum_release_age_exclude, Some(vec!["left-pad".to_string()]));
    assert_eq!(overlay.trust_lockfile, Some(false));
    assert_eq!(overlay.network_concurrency, Some(12));
    assert_eq!(overlay.max_sockets, Some(7));
    assert_eq!(overlay.fetch_retries, Some(4));
    assert_eq!(overlay.fetch_retry_factor, Some(2));
    assert_eq!(overlay.fetch_retry_mintimeout, Some(10));
    assert_eq!(overlay.fetch_retry_maxtimeout, Some(20));
    assert_eq!(overlay.fetch_timeout, Some(30));
    assert_eq!(overlay.user_agent, Some("pnpm-test".to_string()));
    let proxy = overlay.proxy.expect("proxy");
    assert_eq!(proxy.http_proxy, Some("http://proxy.test".to_string()));
    assert_eq!(proxy.https_proxy, Some("https://proxy.test".to_string()));
    assert_eq!(
        proxy.no_proxy,
        Some(NoProxySetting::List(vec!["localhost".to_string(), "127.0.0.1".to_string()])),
    );
    let tls = overlay.tls.expect("tls");
    assert_eq!(tls.ca, vec!["cert-a".to_string(), "cert-b".to_string()]);
    assert_eq!(tls.cert, Some("client-cert".to_string()));
    assert_eq!(tls.key, Some("client-key".to_string()));
    assert_eq!(tls.strict_ssl, Some(false));
    assert_eq!(tls.local_address.map(|ip| ip.to_string()), Some("127.0.0.1".to_string()));
}

#[test]
fn resolved_config_applies_trust_lockfile() {
    let dir = tempfile::tempdir().expect("tempdir");

    for (trust_lockfile, expected) in [(Some(false), false), (Some(true), true), (None, false)] {
        let mut options = install_options();
        options.trust_lockfile = trust_lockfile;
        let overlay = build_overlay(&options).expect("overlay");
        assert_eq!(resolve_config(dir.path(), &overlay).expect("config").trust_lockfile, expected);
    }
}

#[test]
fn resolved_config_applies_allow_unused_patches() {
    let dir = tempfile::tempdir().expect("tempdir");

    for (allow_unused_patches, expected) in
        [(Some(false), false), (Some(true), true), (None, false)]
    {
        let mut options = install_options();
        options.allow_unused_patches = allow_unused_patches;
        let overlay = build_overlay(&options).expect("overlay");
        assert_eq!(
            resolve_config(dir.path(), &overlay).expect("config").allow_unused_patches,
            expected,
        );
    }
}

#[test]
fn build_overlay_parses_link_workspace_packages() {
    use pnpm_config::LinkWorkspacePackages;

    let mut options = install_options();
    options.link_workspace_packages = Some(serde_json::json!("deep"));
    assert_eq!(
        build_overlay(&options).expect("overlay").link_workspace_packages,
        Some(LinkWorkspacePackages::Deep),
    );

    options.link_workspace_packages = Some(serde_json::json!(true));
    assert_eq!(
        build_overlay(&options).expect("overlay").link_workspace_packages,
        Some(LinkWorkspacePackages::DirectOnly),
    );

    options.link_workspace_packages = Some(serde_json::json!(false));
    assert_eq!(
        build_overlay(&options).expect("overlay").link_workspace_packages,
        Some(LinkWorkspacePackages::Off),
    );

    // Anything other than a boolean or "deep" is rejected.
    options.link_workspace_packages = Some(serde_json::json!("shallow"));
    assert!(build_overlay(&options).is_err());
}

#[test]
fn unsupported_install_options_fail_closed() {
    let mut options = install_options();
    options.auth_config = Some([("token".to_string(), "secret".to_string())].into());
    assert!(reject_unsupported_install_options(&options).is_err());

    let mut options = install_options();
    options.never_built_dependencies = Some(vec!["esbuild".to_string()]);
    assert!(reject_unsupported_install_options(&options).is_err());
}

#[test]
fn non_object_project_manifests_are_rejected() {
    let ok = vec![NodeApiProject {
        root_dir: "/a".to_string(),
        manifest: serde_json::json!({ "name": "x" }),
        dependency_manifest: None,
    }];
    assert!(reject_non_object_manifests(&ok).is_ok());

    for bad in [
        serde_json::json!([1, 2, 3]),
        serde_json::json!("oops"),
        serde_json::json!(42),
        serde_json::json!(null),
    ] {
        let projects = vec![NodeApiProject {
            root_dir: "/a".to_string(),
            manifest: bad,
            dependency_manifest: None,
        }];
        assert!(reject_non_object_manifests(&projects).is_err());
    }
}

#[test]
fn newly_supported_install_options_are_accepted() {
    // These options are accepted and flow through to the engine.
    let mut options = install_options();
    options.update = Some(true);
    options.depth = Some(0);
    options.engine_strict = Some(true);
    options.node_version = Some("20.11.0".to_string());
    options.enable_modules_dir = Some(false);
    options.ignore_package_manifest = Some(true);
    options.pnpm_home_dir = Some("/home/user/.local/share/pnpm".to_string());
    options.network_config = Some(NetworkConfigInput { max_sockets: Some(20), ..network_config() });
    assert!(reject_unsupported_install_options(&options).is_ok());
    assert_eq!(build_overlay(&options).expect("overlay").max_sockets, Some(20));
}

#[test]
fn repeat_install_uses_changed_in_memory_manifest() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));

    run_install_inner(&options, None, EngineMode::Install(None)).expect("first install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/foo").exists());

    options.projects[0].manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/bar": "100.0.0",
            "@pnpm.e2e/foo": "100.0.0"
        }
    });

    run_install_inner(&options, None, EngineMode::Install(None)).expect("second install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/bar").exists());
    assert_eq!(
        std::fs::read_to_string(project_dir.join("package.json")).expect("read package.json"),
        "{}\n",
    );
}

/// An empty list and an uncomputed one are different answers. The first
/// says the tree has no build-needing packages; the second says this
/// install never looked, so an embedder mirroring the field into a file
/// it owns has to keep its own record.
#[test]
fn take_deps_requiring_build_distinguishes_an_empty_list_from_an_uncomputed_one() {
    let empty = DepsRequiringBuildSink::default();
    *empty.lock().expect("lock sink") = Some(BTreeSet::new());
    assert_eq!(take_deps_requiring_build(Some(&empty), Vec::new()), Some(Vec::new()));

    let uncomputed = DepsRequiringBuildSink::default();
    assert_eq!(take_deps_requiring_build(Some(&uncomputed), Vec::new()), None);
}

/// The result preserves the sink's order so a consumer diffing it against
/// a recorded list sees no spurious churn.
#[test]
fn take_deps_requiring_build_reports_the_list_in_sorted_order() {
    let sink = DepsRequiringBuildSink::default();
    *sink.lock().expect("lock sink") = Some(BTreeSet::from([
        "zzz@1.0.0".to_string(),
        "aaa@1.0.0".to_string(),
        "mmm@1.0.0".to_string(),
    ]));

    assert_eq!(
        take_deps_requiring_build(Some(&sink), Vec::new()),
        Some(vec!["aaa@1.0.0".to_string(), "mmm@1.0.0".to_string(), "zzz@1.0.0".to_string()]),
    );
}

/// Without the option the field carries the blocked builds, and stays
/// undefined when nothing was blocked.
#[test]
fn take_deps_requiring_build_falls_back_to_blocked_builds_without_the_option() {
    assert_eq!(
        take_deps_requiring_build(None, vec!["blocked@1.0.0".to_string()]),
        Some(vec!["blocked@1.0.0".to_string()]),
    );
    assert_eq!(take_deps_requiring_build(None, Vec::new()), None);
}

/// `returnListOfDepsRequiringBuild` reports every package whose files
/// carry install scripts, sorted. `hello-world-js-bin` arrives as a
/// dependency of the postinstall example and carries no install scripts
/// of its own, so it must not appear.
#[test]
fn return_list_of_deps_requiring_build_reports_every_script_bearing_package() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = script_deps_install_options(temp_dir.path());
    options.dangerously_allow_all_builds = Some(true);

    let sink = DepsRequiringBuildSink::default();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
        .expect("install");

    assert_eq!(
        take_deps_requiring_build(Some(&sink), Vec::new()),
        Some(vec![
            "@pnpm.e2e/install-script-example@1.0.0".to_string(),
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0".to_string(),
        ]),
    );
}

/// The list is independent of the allow-build policy. A package whose
/// scripts the default policy blocks still requires a build, and an
/// embedder that gates builds itself needs to know about it.
#[test]
fn return_list_of_deps_requiring_build_includes_packages_whose_builds_were_blocked() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let options = script_deps_install_options(temp_dir.path());

    let sink = DepsRequiringBuildSink::default();
    begin_stats();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
        .expect("install");
    let blocked = take_stats().deps_requiring_build;

    assert_eq!(
        blocked.len(),
        2,
        "without an allow-build policy both builds must be blocked, else this test proves nothing",
    );
    assert_eq!(
        take_deps_requiring_build(Some(&sink), Vec::new()),
        Some(vec![
            "@pnpm.e2e/install-script-example@1.0.0".to_string(),
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0".to_string(),
        ]),
    );
}

/// Only a fresh resolve that materializes `node_modules` computes the
/// list. A repeat install (served from the frozen path), an explicit
/// `frozenLockfile`, and a `lockfileOnly` run all leave it uncomputed.
#[test]
fn return_list_of_deps_requiring_build_is_uncomputed_without_a_fresh_materialization() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = script_deps_install_options(temp_dir.path());
    options.dangerously_allow_all_builds = Some(true);

    let seed = DepsRequiringBuildSink::default();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&seed))))
        .expect("seed install");
    let seeded = seed.lock().expect("lock sink").clone();
    dbg!(&seeded);
    assert!(seeded.is_some(), "the seed install computes the list");

    for (label, mutate) in [
        ("repeat install", (|_: &mut InstallOptions| {}) as fn(&mut InstallOptions)),
        ("frozen lockfile", |options: &mut InstallOptions| {
            options.frozen_lockfile = Some(true);
        }),
        ("lockfile only", |options: &mut InstallOptions| {
            options.lockfile_only = Some(true);
        }),
    ] {
        let mut options = script_deps_install_options(temp_dir.path());
        options.dangerously_allow_all_builds = Some(true);
        mutate(&mut options);

        let sink = DepsRequiringBuildSink::default();
        run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
            .unwrap_or_else(|error| panic!("{label} install: {error}"));

        assert_eq!(
            take_deps_requiring_build(Some(&sink), Vec::new()),
            None,
            "{label} must leave the list uncomputed",
        );
    }
}

/// A tree with no script-bearing package has nothing to build, and that
/// is an answer. The install reports an empty list rather than none, so
/// an embedder replaces its recorded list instead of keeping a stale one.
#[test]
fn return_list_of_deps_requiring_build_reports_an_empty_list_for_a_tree_without_build_scripts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let options = install_options_for(
        temp_dir.path(),
        "project",
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );

    let sink = DepsRequiringBuildSink::default();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
        .expect("install");

    assert_eq!(take_deps_requiring_build(Some(&sink), Vec::new()), Some(Vec::new()));
}

/// A script-bearing package this install skips is left out even when the
/// shared store already knows it requires a build. The reported list
/// covers what this project installed, not what the store has seen.
#[test]
fn return_list_of_deps_requiring_build_excludes_skipped_packages() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let warm_store = install_options_for(
        temp_dir.path(),
        "warm-store",
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }
        }),
    );
    let warmed = DepsRequiringBuildSink::default();
    run_install_inner(&warm_store, None, EngineMode::Install(Some(Arc::clone(&warmed))))
        .expect("warm the store");
    assert_eq!(
        take_deps_requiring_build(Some(&warmed), Vec::new()),
        Some(vec!["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0".to_string()]),
        "the store must know this package requires a build, else the skip proves nothing",
    );

    let mut options = install_options_for(
        temp_dir.path(),
        "skipping",
        serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }
        }),
    );
    options.include_optional_deps = Some(false);

    let sink = DepsRequiringBuildSink::default();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
        .expect("install skipping the optional dependency");

    assert_eq!(take_deps_requiring_build(Some(&sink), Vec::new()), Some(Vec::new()));
}

/// Install options for a project depending on two packages that carry
/// install scripts, sharing one store across the calls in a test so a
/// repeat install can hit the frozen path.
fn script_deps_install_options(temp_dir: &Path) -> InstallOptions {
    install_options_for(
        temp_dir,
        "project",
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
                "@pnpm.e2e/install-script-example": "1.0.0"
            }
        }),
    )
}

/// Install options for one project under `temp_dir`, with
/// `returnListOfDepsRequiringBuild` set and the store shared across every
/// project in the same `temp_dir`.
fn install_options_for(
    temp_dir: &Path,
    project_name: &str,
    manifest: serde_json::Value,
) -> InstallOptions {
    let registry = TestRegistry::start();
    let project_dir = temp_dir.join(project_name);
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects =
        vec![NodeApiProject { root_dir: project_dir_string, manifest, dependency_manifest: None }];
    options.store_dir = Some(temp_dir.join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));
    options.return_list_of_deps_requiring_build = Some(true);
    options
}

/// The lockfile must record `overrides` in declaration order — the
/// napi boundary carries them through an `IndexMap`, and a `HashMap`
/// regression would rewrite the block in a random order on every
/// install (a sorted map would flip the non-lexicographic order below).
#[test]
fn lockfile_records_overrides_in_declaration_order() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));
    options.overrides = Some(indexmap::IndexMap::from_iter([
        ("zzz-unmatched".to_string(), "1.0.0".to_string()),
        ("aaa-unmatched".to_string(), "2.0.0".to_string()),
    ]));

    run_install_inner(&options, None, EngineMode::Install(None)).expect("install");

    let lockfile =
        std::fs::read_to_string(project_dir.join("pnpm-lock.yaml")).expect("read lockfile");
    let zzz = lockfile.find("zzz-unmatched").expect("zzz override recorded");
    let aaa = lockfile.find("aaa-unmatched").expect("aaa override recorded");
    assert!(zzz < aaa, "overrides must keep declaration order (zzz before aaa), got:\n{lockfile}");
}

#[test]
fn allow_unused_patches_downgrades_an_unmatched_patch_to_a_warning() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");
    std::fs::create_dir(project_dir.join("patches")).expect("create patches dir");
    std::fs::write(project_dir.join("patches/unmatched.patch"), WELL_FORMED_PATCH)
        .expect("write patch file");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));
    options.patched_dependencies = Some(indexmap::IndexMap::from_iter([(
        "is-negative@1.0.0".to_string(),
        "patches/unmatched.patch".to_string(),
    )]));

    let error = run_install_inner(&options, None, EngineMode::Install(None))
        .expect_err("an unmatched patch must fail the install");
    assert!(
        error.reason.contains("ERR_PNPM_UNUSED_PATCH"),
        "expected ERR_PNPM_UNUSED_PATCH, got: {reason}",
        reason = error.reason,
    );

    options.allow_unused_patches = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("allowUnusedPatches must let the install through");
}

fn install_options() -> InstallOptions {
    InstallOptions {
        dir: String::new(),
        projects: vec![NodeApiProject {
            root_dir: String::new(),
            manifest: serde_json::json!({}),
            dependency_manifest: None,
        }],
        store_dir: None,
        cache_dir: None,
        registries: None,
        auth_config: None,
        proxy_config: None,
        network_config: None,
        node_linker: None,
        link_workspace_packages: None,
        hoist_pattern: None,
        public_hoist_pattern: None,
        external_dependencies: None,
        overrides: None,
        package_import_method: None,
        auto_install_peers: None,
        exclude_links_from_lockfile: None,
        lockfile_only: None,
        frozen_lockfile: None,
        prefer_frozen_lockfile: None,
        prefer_offline: None,
        offline: None,
        virtual_store_dir_max_length: None,
        enable_global_virtual_store: None,
        global_virtual_store_dir: None,
        package_extensions: None,
        patched_dependencies: None,
        allow_unused_patches: None,
        peers_suffix_max_length: None,
        dedupe_peer_dependents: None,
        dedupe_peers: None,
        dedupe_direct_deps: None,
        dedupe_injected_deps: None,
        resolve_peers_from_workspace_root: None,
        inject_workspace_packages: None,
        hoist_workspace_packages: None,
        enable_modules_dir: None,
        ignore_package_manifest: None,
        node_version: None,
        engine_strict: None,
        minimum_release_age: None,
        minimum_release_age_exclude: None,
        never_built_dependencies: None,
        update: None,
        depth: None,
        include_optional_deps: None,
        ignore_scripts: None,
        trust_lockfile: None,
        network_concurrency: None,
        fetch_retries: None,
        fetch_retry_factor: None,
        fetch_retry_mintimeout: None,
        fetch_retry_maxtimeout: None,
        fetch_timeout: None,
        user_agent: None,
        strict_dep_builds: None,
        return_list_of_deps_requiring_build: None,
        allow_builds: None,
        dangerously_allow_all_builds: None,
        peer_dependency_rules: None,
        auth_header_by_uri: None,
        pnpm_home_dir: None,
    }
}

fn network_config() -> NetworkConfigInput {
    NetworkConfigInput {
        ca: None,
        cert: None,
        key: None,
        local_address: None,
        strict_ssl: None,
        max_sockets: None,
        network_concurrency: None,
        fetch_retries: None,
        fetch_retry_factor: None,
        fetch_retry_mintimeout: None,
        fetch_retry_maxtimeout: None,
        fetch_timeout: None,
        user_agent: None,
    }
}

/// `safe_intersect` mirrors v11's `mergePeers` helper: pairwise semver
/// range intersection, `None` on an empty intersection or unparsable
/// range (→ recorded as a conflict by the caller).
#[test]
fn safe_intersect_matches_merge_peers_semantics() {
    use super::safe_intersect;

    // Overlapping ranges intersect to a non-empty range.
    let merged = safe_intersect(["^16.8.0", "16 || 17"].into_iter()).expect("ranges overlap");
    let range: node_semver::Range = merged.parse().expect("intersection parses");
    assert!(range.satisfies(&"16.9.1".parse().unwrap()));
    assert!(!range.satisfies(&"17.0.0".parse().unwrap()));

    // Disjoint ranges → None (conflict).
    assert_eq!(safe_intersect(["^16.0.0", "^17.0.0"].into_iter()), None);
    // Unparsable range → None, matching v11's swallow-errors behavior.
    assert_eq!(safe_intersect(["^16.0.0", "not-a-range"].into_iter()), None);
}

/// The wire shape mirrors v11's `PeerDependencyIssues`: `missing` /
/// `bad` entries verbatim, `intersections` from the non-optional
/// missing ranges, and disjoint ranges surfacing under `conflicts`.
#[test]
fn peer_issues_to_json_derives_conflicts_and_intersections() {
    use pnpm_resolving_deps_resolver::{
        MissingPeer, ParentChain, PeerDependencyIssue, PeerDependencyIssues,
    };

    let missing_entry = |range: &str, optional: bool| MissingPeer {
        wanted_range: range.to_string(),
        raw_range: range.to_string(),
        optional,
        parents: ParentChain::from_names(["comp1".to_string()]),
    };
    let mut issues = PeerDependencyIssues::default();
    issues.missing.insert("react".to_string(), vec![missing_entry("^16.8.0", false)]);
    issues.missing.insert(
        "conflicted".to_string(),
        vec![missing_entry("^1.0.0", false), missing_entry("^2.0.0", false)],
    );
    issues.missing.insert("optional-only".to_string(), vec![missing_entry("*", true)]);
    issues.bad.insert(
        "styled".to_string(),
        vec![PeerDependencyIssue {
            wanted_range: "^5.0.0".to_string(),
            found_version: "4.1.0".to_string(),
            optional: false,
            parents: ParentChain::from_names([
                "root".to_string(),
                "mid".to_string(),
                "leaf".to_string(),
            ]),
            resolved_from: ParentChain::from_names(["provider".to_string()]),
        }],
    );

    let json = super::peer_issues_to_json(&issues);
    assert_eq!(json["intersections"]["react"], "^16.8.0");
    assert_eq!(json["conflicts"], serde_json::json!(["conflicted"]));
    assert!(json["intersections"].get("optional-only").is_none());
    assert_eq!(json["missing"]["react"][0]["wantedRange"], "^16.8.0");
    assert_eq!(json["missing"]["react"][0]["parents"][0]["name"], "comp1");
    assert_eq!(
        json["bad"]["styled"][0]["parents"],
        serde_json::json!([
            { "name": "root", "version": "" },
            { "name": "mid", "version": "" },
            { "name": "leaf", "version": "" },
        ]),
    );
    assert_eq!(
        json["bad"]["styled"][0]["resolvedFrom"],
        serde_json::json!([{ "name": "provider", "version": "" }]),
    );
    assert_eq!(json["bad"]["styled"][0]["foundVersion"], "4.1.0");
}
