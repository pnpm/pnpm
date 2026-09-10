use super::{
    DepsRequiringBuildSink, EngineMode, InstallOptions, NetworkConfigInput, NodeApiProject,
    PeerIssuesOptions, ProxyConfigInput, build_overlay, peer_issues::peer_issues_install_options,
    reject_non_object_manifests, reject_unsupported_install_options, run_install_inner,
    take_deps_requiring_build,
};
use crate::{
    config::{ConfigOverlay, resolve_config},
    reporter_bridge::{begin_stats, take_stats},
};
use pnpm_network::NoProxySetting;
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::registry::TestRegistry;
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::Arc,
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
        fetch_warn_timeout_ms: None,
        fetch_min_speed_ki_bps: None,
        user_agent: None,
        strict_dep_builds: None,
        return_list_of_deps_requiring_build: None,
        allow_builds: None,
        dangerously_allow_all_builds: None,
        peer_dependency_rules: None,
        auth_header_by_uri: None,
        pnpm_home_dir: None,
        reporter: None,
    }
}

fn peer_issues_options() -> PeerIssuesOptions {
    PeerIssuesOptions {
        dir: String::new(),
        projects: Vec::new(),
        store_dir: None,
        cache_dir: None,
        registries: None,
        auth_header_by_uri: None,
        proxy_config: None,
        network_config: None,
        overrides: None,
        peers_suffix_max_length: None,
        virtual_store_dir_max_length: None,
        auto_install_peers: None,
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
        fetch_warn_timeout_ms: None,
        fetch_min_speed_ki_bps: None,
        user_agent: None,
    }
}

mod workspace_settings;

mod behavior;

mod configuration;

mod manifests;

mod dependencies;

mod files;

mod reporting;

mod lockfile;
