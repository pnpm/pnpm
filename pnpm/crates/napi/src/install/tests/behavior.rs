use super::{
    Arc, BTreeSet, DepsRequiringBuildSink, EngineMode, InstallOptions, NetworkConfigInput,
    NoProxySetting, ProxyConfigInput, begin_stats, build_overlay, install_options,
    install_options_for, network_config, reject_unsupported_install_options, run_install_inner,
    script_deps_install_options, take_deps_requiring_build, take_stats,
};

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
        fetch_warn_timeout_ms: Some(40),
        fetch_min_speed_ki_bps: Some(50),
        user_agent: Some("pnpm-test".to_string()),
    });
    options.proxy_config = Some(ProxyConfigInput {
        http_proxy: Some("http://proxy.test".to_string()),
        https_proxy: Some("https://proxy.test".to_string()),
        no_proxy: Some(serde_json::json!("localhost,127.0.0.1")),
    });

    let overlay = build_overlay(&options, false).expect("overlay");
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
    assert_eq!(overlay.fetch_warn_timeout_ms, Some(40));
    assert_eq!(overlay.fetch_min_speed_ki_bps, Some(50));
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
fn unsupported_install_options_fail_closed() {
    let mut options = install_options();
    options.auth_config = Some([("token".to_string(), "secret".to_string())].into());
    assert!(reject_unsupported_install_options(&options).is_err());

    let mut options = install_options();
    options.never_built_dependencies = Some(vec!["esbuild".to_string()]);
    assert!(reject_unsupported_install_options(&options).is_err());
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
    assert_eq!(build_overlay(&options, false).expect("overlay").max_sockets, Some(20));
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
