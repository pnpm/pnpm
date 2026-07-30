use std::collections::HashMap;

use pacquet_network::NoProxySetting;
use pacquet_testing_utils::registry::TestRegistry;

use super::{
    EngineMode, InstallOptions, NetworkConfigInput, NodeApiProject, ProxyConfigInput,
    build_overlay, reject_non_object_manifests, reject_unsupported_install_options,
    run_install_inner,
};
use crate::config::{ConfigOverlay, resolve_config};

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
fn build_overlay_parses_link_workspace_packages() {
    use pacquet_config::LinkWorkspacePackages;

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
fn never_built_dependencies_fold_into_allow_builds_as_denials() {
    let mut options = install_options();
    options.allow_builds =
        Some(HashMap::from([("esbuild".to_string(), true), ("core-js".to_string(), true)]));
    options.never_built_dependencies = Some(vec!["core-js".to_string(), "fsevents".to_string()]);
    options.dangerously_allow_all_builds = Some(true);

    let overlay = build_overlay(&options).expect("overlay");
    let allow_builds = overlay.allow_builds.expect("allow_builds");
    assert_eq!(allow_builds.get("esbuild"), Some(&true));
    assert_eq!(allow_builds.get("core-js"), Some(&false));
    assert_eq!(allow_builds.get("fsevents"), Some(&false));
    // The engine's allow-everything short-circuit runs before the explicit
    // denials, so a non-empty neverBuiltDependencies must turn it off.
    assert_eq!(overlay.dangerously_allow_all_builds, Some(false));
}

#[test]
fn empty_never_built_dependencies_leave_build_policy_untouched() {
    let mut options = install_options();
    options.never_built_dependencies = Some(vec![]);
    options.dangerously_allow_all_builds = Some(true);

    let overlay = build_overlay(&options).expect("overlay");
    assert_eq!(overlay.allow_builds, None);
    assert_eq!(overlay.dangerously_allow_all_builds, Some(true));
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

    run_install_inner(&options, None, EngineMode::Install).expect("first install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/foo").exists());

    options.projects[0].manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/bar": "100.0.0",
            "@pnpm.e2e/foo": "100.0.0"
        }
    });

    run_install_inner(&options, None, EngineMode::Install).expect("second install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/bar").exists());
    assert_eq!(
        std::fs::read_to_string(project_dir.join("package.json")).expect("read package.json"),
        "{}\n",
    );
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

    run_install_inner(&options, None, EngineMode::Install).expect("install");

    let lockfile =
        std::fs::read_to_string(project_dir.join("pnpm-lock.yaml")).expect("read lockfile");
    let zzz = lockfile.find("zzz-unmatched").expect("zzz override recorded");
    let aaa = lockfile.find("aaa-unmatched").expect("aaa override recorded");
    assert!(zzz < aaa, "overrides must keep declaration order (zzz before aaa), got:\n{lockfile}");
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
        ignored_dependencies: None,
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
    use pacquet_resolving_deps_resolver::{MissingPeer, ParentPackageRef, PeerDependencyIssues};

    let parent = ParentPackageRef { name: "comp1".to_string(), version: "1.0.0".to_string() };
    let missing_entry = |range: &str, optional: bool| MissingPeer {
        wanted_range: range.to_string(),
        raw_range: range.to_string(),
        optional,
        parents: vec![parent.clone()],
    };
    let mut issues = PeerDependencyIssues::default();
    issues.missing.insert("react".to_string(), vec![missing_entry("^16.8.0", false)]);
    issues.missing.insert(
        "conflicted".to_string(),
        vec![missing_entry("^1.0.0", false), missing_entry("^2.0.0", false)],
    );
    issues.missing.insert("optional-only".to_string(), vec![missing_entry("*", true)]);

    let json = super::peer_issues_to_json(&issues);
    assert_eq!(json["intersections"]["react"], "^16.8.0");
    assert_eq!(json["conflicts"], serde_json::json!(["conflicted"]));
    assert!(json["intersections"].get("optional-only").is_none());
    assert_eq!(json["missing"]["react"][0]["wantedRange"], "^16.8.0");
    assert_eq!(json["missing"]["react"][0]["parents"][0]["name"], "comp1");
    assert_eq!(json["bad"], serde_json::json!({}));
}
