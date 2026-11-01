//! Contract test: every [`Config`] field is classified by how it reaches the
//! repeat-install fast path.
//!
//! The field list is read from the source of `Config`, so a new field fails
//! [`every_config_field_is_classified`] until it is classified. A field in
//! [`Reach::Recorded`] or [`Reach::BlocksFastPath`] is proven by changing it.
//! The other buckets carry the reason, which the test cannot check.

use super::{super::config_blocks_fast_path, current_settings, isolated_included};
use crate::optimistic_repeat_install::first_setting_drift;
use indexmap::IndexMap;
use pnpm_config::{Config, LinkWorkspacePackages, NodeLinker, TrustPolicy};
use pnpm_workspace_state::WorkspaceState;
use std::{collections::BTreeSet, path::Path};

/// How a [`Config`] field reaches the fast path.
enum Reach {
    /// The workspace state records it. `change` must make the install gate
    /// report the setting `key`.
    Recorded { key: &'static str, change: fn(&mut Config) },
    /// `change` rules the fast path out.
    BlocksFastPath(fn(&mut Config)),
    /// It reaches the fast path through the input named here.
    Through(&'static str),
    /// A full install of the same selection does not act on a change to it.
    NotAnInput(&'static str),
    /// A full install acts on a change to it, and the fast path does not see
    /// the change. Each entry is a known gap, tracked by
    /// <https://github.com/pnpm/tasks/issues/219>.
    Unseen(&'static str),
}

impl Reach {
    /// Why an unproven classification holds. `None` for the ones the tests
    /// prove by changing the field.
    fn reason(&self) -> Option<&'static str> {
        match self {
            Recorded { .. } | BlocksFastPath(_) => None,
            Through(reason) | NotAnInput(reason) | Unseen(reason) => Some(reason),
        }
    }
}

use Reach::{BlocksFastPath, NotAnInput, Recorded, Through, Unseen};

const CLI_ONLY: &str = "a command-line or output setting";
const NETWORK: &str = "how packages are fetched, not which";
const OTHER_COMMAND: &str = "read by another command";
const RESOLUTION_ONLY: &str =
    "read by resolution only, and a full install keeps a lockfile that satisfies the manifests";
const SCRIPTS: &str = "how scripts run, and an install runs no build of a package it keeps";
const DERIVED: &str = "a value the config loader derives, folded into the fields it derives";
const LOCKFILE_LOCATION: &str = "picks the lockfile the checks read";
const STORE_INTERNALS: &str = "how the store is read and written, not what is linked";
const AUTH: &str = "credentials, not what is installed";

#[rustfmt::skip]
const CLASSIFICATION: &[(&str, Reach)] = &[
    ("add_missing_peer_types", Recorded { key: "addMissingPeerTypes", change: |config| config.add_missing_peer_types = !config.add_missing_peer_types }),
    ("aggregate_output", NotAnInput(CLI_ONLY)),
    ("agent_skills_disabled", Through("the agent skill targets, compared after the fast path")),
    ("allow_builds", Recorded { key: "allowBuilds", change: |config| { config.allow_builds.insert("foo".into(), true); } }),
    ("allow_skills", Recorded { key: "allowSkills", change: |config| { config.allow_skills.insert("foo".into(), true); } }),
    ("allow_unused_patches", NotAnInput("whether an unused patch fails the install")),
    ("allowed_deprecated_versions", NotAnInput(RESOLUTION_ONLY)),
    ("audit_config", NotAnInput(OTHER_COMMAND)),
    ("audit_ignore_prune", NotAnInput(OTHER_COMMAND)),
    ("audit_level", NotAnInput(OTHER_COMMAND)),
    ("auth_headers", NotAnInput(AUTH)),
    ("auth_tokens_by_uri", NotAnInput(AUTH)),
    ("auto_dedupe", Recorded { key: "autoDedupe", change: |config| config.auto_dedupe = !config.auto_dedupe }),
    ("auto_install_peers", Recorded { key: "autoInstallPeers", change: |config| config.auto_install_peers = !config.auto_install_peers }),
    ("auto_install_peers_from_highest_match", NotAnInput(RESOLUTION_ONLY)),
    ("bail", NotAnInput(CLI_ONLY)),
    ("block_exotic_subdeps", NotAnInput(RESOLUTION_ONLY)),
    ("boot_config", NotAnInput("selects the pnpm that runs, before any install")),
    ("boot_inherited", NotAnInput("selects the pnpm that runs, before any install")),
    ("boot_overrides", NotAnInput("selects the pnpm that runs, before any install")),
    ("cache_dir", NotAnInput(NETWORK)),
    ("cargo", NotAnInput("Cargo dependencies are installed by their own step")),
    ("catalog_mode", NotAnInput(OTHER_COMMAND)),
    ("catalog_prune", NotAnInput(OTHER_COMMAND)),
    ("catalogs", Through("the catalogs the install resolves with, compared as the catalogs cache")),
    ("changed_files_ignore_pattern", NotAnInput(OTHER_COMMAND)),
    ("child_concurrency", NotAnInput(SCRIPTS)),
    ("ci", Through("the defaults it selects, such as `frozenLockfile`")),
    ("cli", Through("the settings it sets, each classified under its own field")),
    ("cli_settings", NotAnInput("records which settings came from the command line")),
    ("color", NotAnInput(CLI_ONLY)),
    ("concurrency_groups", NotAnInput(OTHER_COMMAND)),
    ("config_dependencies", Through("compared by the config-dependencies check")),
    ("config_dir", NotAnInput("where the global config is read from")),
    ("configured_virtual_store_dir", Through("the configured `virtualStoreDir`, recorded as `virtualStoreDir`")),
    ("dangerously_allow_all_builds", Recorded { key: "dangerouslyAllowAllBuilds", change: |config| config.dangerously_allow_all_builds = !config.dangerously_allow_all_builds }),
    ("dedupe_direct_deps", Recorded { key: "dedupeDirectDeps", change: |config| config.dedupe_direct_deps = !config.dedupe_direct_deps }),
    ("dedupe_injected_deps", Recorded { key: "dedupeInjectedDeps", change: |config| config.dedupe_injected_deps = !config.dedupe_injected_deps }),
    ("dedupe_peer_dependents", Recorded { key: "dedupePeerDependents", change: |config| config.dedupe_peer_dependents = !config.dedupe_peer_dependents }),
    ("dedupe_peers", Recorded { key: "dedupePeers", change: |config| config.dedupe_peers = !config.dedupe_peers }),
    ("deploy_all_files", NotAnInput(OTHER_COMMAND)),
    ("disallow_workspace_cycles", NotAnInput("whether a cycle fails the install")),
    ("dlx_cache_max_age", NotAnInput(OTHER_COMMAND)),
    ("embed_readme", NotAnInput(OTHER_COMMAND)),
    ("enable_global_virtual_store", Recorded { key: "enableGlobalVirtualStore", change: |config| config.enable_global_virtual_store = !config.enable_global_virtual_store }),
    ("enable_modules_dir", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory applies it")),
    ("enable_pre_post_scripts", NotAnInput(SCRIPTS)),
    ("engine_strict", NotAnInput("whether an incompatible package fails the install")),
    ("exclude_links_from_lockfile", Recorded { key: "excludeLinksFromLockfile", change: |config| config.exclude_links_from_lockfile = !config.exclude_links_from_lockfile }),
    ("explicit_settings", Through("the settings it names, each classified under its own field")),
    ("extend_node_path", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory rewrites the bins")),
    ("external_dependencies", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory applies it")),
    ("extra_bin_paths", NotAnInput(SCRIPTS)),
    ("extra_env", NotAnInput(SCRIPTS)),
    ("fail_if_no_match", NotAnInput(CLI_ONLY)),
    ("fetch_min_speed_ki_bps", NotAnInput(NETWORK)),
    ("fetch_retries", NotAnInput(NETWORK)),
    ("fetch_retry_factor", NotAnInput(NETWORK)),
    ("fetch_retry_maxtimeout", NotAnInput(NETWORK)),
    ("fetch_retry_mintimeout", NotAnInput(NETWORK)),
    ("fetch_timeout", NotAnInput(NETWORK)),
    ("fetch_warn_timeout_ms", NotAnInput(NETWORK)),
    ("filter", Through("the selection the fast path is scoped to")),
    ("filter_prod", Through("the selection the fast path is scoped to")),
    ("force", Through("skips the fast path before it runs")),
    ("force_ignores_platform", Through("acts only with `force`, which skips the fast path")),
    ("force_legacy_deploy", NotAnInput(OTHER_COMMAND)),
    ("frozen_lockfile", NotAnInput("whether the lockfile may be rewritten, not what is installed")),
    ("frozen_store", NotAnInput(STORE_INTERNALS)),
    ("git_branch_lockfile_candidates", Through(LOCKFILE_LOCATION)),
    ("git_branch_lockfile_name", Through(LOCKFILE_LOCATION)),
    ("git_checks", NotAnInput(OTHER_COMMAND)),
    ("git_tag_version", NotAnInput("read only by `pnpm version`")),
    ("git_shallow_hosts", NotAnInput(NETWORK)),
    ("global_bin", NotAnInput("the global packages, which a project install does not touch")),
    ("global_bin_dir", NotAnInput("the global packages, which a project install does not touch")),
    ("global_dir", NotAnInput("the global packages, which a project install does not touch")),
    ("global_pkg_dir", NotAnInput("the global packages, which a project install does not touch")),
    ("global_pnpmfile", Through("the pnpmfile list, compared as `pnpmfiles`")),
    ("global_shims", NotAnInput("the global packages, which a project install does not touch")),
    ("global_virtual_store_before_loaded", Through("restores `enableGlobalVirtualStore`, which is recorded")),
    ("global_virtual_store_dir", Recorded { key: "globalVirtualStoreDir", change: |config| { config.explicit_settings.insert("globalVirtualStoreDir".into(), "/links".into()); config.global_virtual_store_dir = "/links".into(); } }),
    ("hoist", Through(DERIVED)),
    ("hoist_pattern", Recorded { key: "hoistPattern", change: |config| config.hoist_pattern = Some(vec!["foo".into()]) }),
    ("hoist_patterns_before_virtual_store_only", Through(DERIVED)),
    ("hoist_workspace_packages", Recorded { key: "hoistWorkspacePackages", change: |config| config.hoist_workspace_packages = !config.hoist_workspace_packages }),
    ("hoisting_limits", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory applies it")),
    ("ignore_compatibility_db", NotAnInput(RESOLUTION_ONLY)),
    ("ignore_pnpmfile", Through("the pnpmfile list, compared as `pnpmfiles`")),
    ("ignore_scripts", NotAnInput(SCRIPTS)),
    ("ignore_workspace", Through("the project list, compared by the project-structure check")),
    ("ignore_workspace_cycles", NotAnInput(CLI_ONLY)),
    ("ignore_workspace_root_check", NotAnInput(OTHER_COMMAND)),
    ("ignored_optional_dependencies", Recorded { key: "ignoredOptionalDependencies", change: |config| config.ignored_optional_dependencies = Some(vec!["foo".into()]) }),
    ("include_workspace_root", Through("the selection the fast path is scoped to")),
    ("indexes_by_ecosystem", NotAnInput("non-npm packages are installed by their own step")),
    ("init_author_email", NotAnInput(OTHER_COMMAND)),
    ("init_author_name", NotAnInput(OTHER_COMMAND)),
    ("init_author_url", NotAnInput(OTHER_COMMAND)),
    ("init_license", NotAnInput(OTHER_COMMAND)),
    ("init_package_manager", NotAnInput(OTHER_COMMAND)),
    ("init_type", NotAnInput(OTHER_COMMAND)),
    ("init_version", NotAnInput(OTHER_COMMAND)),
    ("inject_workspace_packages", Recorded { key: "injectWorkspacePackages", change: |config| config.inject_workspace_packages = !config.inject_workspace_packages }),
    ("install_state_dir", Through("the modules directory the workspace state is read from")),
    ("isolate_local_directory_imports", NotAnInput("a local directory dependency rules the fast path out on its own")),
    ("legacy_dir_filtering", Through("the selection the fast path is scoped to")),
    ("link_workspace_packages", Recorded { key: "linkWorkspacePackages", change: |config| config.link_workspace_packages = LinkWorkspacePackages::Deep }),
    ("lockfile", NotAnInput("whether `pnpm-lock.yaml` is written, not what is installed")),
    ("lockfile_dir", Through(LOCKFILE_LOCATION)),
    ("lockfile_include_resolution_settings", Recorded { key: "lockfile.includeResolutionSettings", change: |config| config.lockfile_include_resolution_settings = !config.lockfile_include_resolution_settings }),
    ("lockfile_include_tarball_url", NotAnInput(RESOLUTION_ONLY)),
    ("lockfile_integrity_target", Recorded { key: "lockfile.integrityTarget", change: |config| config.lockfile_integrity_target = pnpm_lockfile::IntegrityTarget::Tar }),
    ("loglevel", NotAnInput(CLI_ONLY)),
    ("macos_backup", NotAnInput("a Time Machine exclusion, not what is installed")),
    ("max_sockets", NotAnInput(NETWORK)),
    ("max_tarball_unpacked_size", NotAnInput(NETWORK)),
    ("merge_git_branch_lockfiles", BlocksFastPath(|config| config.merge_git_branch_lockfiles = true)),
    ("merge_git_branch_lockfiles_branch_pattern", Through(DERIVED)),
    ("minimum_release_age", Recorded { key: "minimumReleaseAge", change: |config| config.minimum_release_age = Some(config.minimum_release_age.unwrap_or_default() + 1) }),
    ("minimum_release_age_exclude", Recorded { key: "minimumReleaseAgeExclude", change: |config| config.minimum_release_age_exclude = Some(vec!["foo".into()]) }),
    ("minimum_release_age_exclude_prune", NotAnInput("prunes exclude entries from the workspace manifest")),
    ("minimum_release_age_ignore_missing_time", Recorded { key: "minimumReleaseAgeIgnoreMissingTime", change: |config| config.minimum_release_age_ignore_missing_time = !config.minimum_release_age_ignore_missing_time }),
    ("minimum_release_age_strict", Recorded { key: "minimumReleaseAgeStrict", change: |config| config.minimum_release_age_strict = Some(false) }),
    ("modules_cache_max_age", NotAnInput("when orphans are pruned, not what is installed")),
    ("modules_dir", Through("the modules directory the workspace state is read from")),
    ("network_concurrency", NotAnInput(NETWORK)),
    ("network_concurrency_by_registry", NotAnInput(NETWORK)),
    ("node_download_mirrors", NotAnInput(NETWORK)),
    ("node_linker", Through("the layout's node linker, compared as `nodeLinker`")),
    ("node_linker_excluded", Through("recorded only for the `loaded` linker, which rules the fast path out")),
    ("node_options", NotAnInput(SCRIPTS)),
    ("node_package_map_mode", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory applies it")),
    ("node_package_map_type", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory applies it")),
    ("node_version", NotAnInput("whether an incompatible package fails the install")),
    ("npmrc_auth_file", NotAnInput(AUTH)),
    ("npmrc_warnings", NotAnInput(CLI_ONLY)),
    ("offline", NotAnInput(NETWORK)),
    ("optimistic_repeat_install", BlocksFastPath(|config| config.optimistic_repeat_install = false)),
    ("optional", Through("the layout's dependency groups, compared as `optional`")),
    ("overrides", Recorded { key: "overrides", change: |config| config.overrides = Some(IndexMap::from([("foo".to_string(), "1.0.0".to_string())])) }),
    ("package_configs", Through("the settings it sets per project, each classified under its own field")),
    ("package_extensions", Recorded { key: "packageExtensions", change: |config| config.package_extensions = serde_json::from_value(serde_json::json!({ "foo": { "dependencies": { "bar": "1.0.0" } } })).unwrap() }),
    ("package_import_method", NotAnInput("an install does not re-import a package that is in place")),
    ("package_lock", Through(DERIVED)),
    ("package_manager_bootstrap", NotAnInput("selects the pnpm that runs, before any install")),
    ("patched_dependencies", Recorded { key: "patchedDependencies", change: |config| config.patched_dependencies = Some(IndexMap::from([("foo".to_string(), "patches/foo.patch".to_string())])) }),
    ("patched_dependency_hashes_override", Through("the patch hashes the lockfile records")),
    ("patches_dir", NotAnInput(OTHER_COMMAND)),
    ("peer_dependency_rules", NotAnInput("which peer dependency issues are reported")),
    ("peers_suffix_max_length", Recorded { key: "peersSuffixMaxLength", change: |config| config.peers_suffix_max_length += 1 }),
    ("pending", NotAnInput(OTHER_COMMAND)),
    ("pipeline_base", NotAnInput(OTHER_COMMAND)),
    ("pipelines", NotAnInput(OTHER_COMMAND)),
    ("pm_on_fail", NotAnInput("selects the pnpm that runs, before any install")),
    ("pnpmfile", Through("the pnpmfile list, compared as `pnpmfiles`")),
    ("pnpr_server", NotAnInput(NETWORK)),
    ("prefer_frozen_lockfile", NotAnInput("whether the lockfile may be rewritten, not what is installed")),
    ("prefer_offline", NotAnInput(NETWORK)),
    ("prefer_symlinked_executables", NotAnInput("a full install leaves the bins it linked")),
    ("prefer_workspace_packages", Recorded { key: "preferWorkspacePackages", change: |config| config.prefer_workspace_packages = !config.prefer_workspace_packages }),
    ("progress", NotAnInput(CLI_ONLY)),
    ("provenance", NotAnInput(OTHER_COMMAND)),
    ("proxy", NotAnInput(NETWORK)),
    ("proxy_keys", NotAnInput(NETWORK)),
    ("prune_direct_dependencies", BlocksFastPath(|config| config.prune_direct_dependencies = true)),
    ("public_hoist_pattern", Recorded { key: "publicHoistPattern", change: |config| config.public_hoist_pattern = Some(vec!["foo".into()]) }),
    ("publish_branch", NotAnInput(OTHER_COMMAND)),
    ("publish_wait_timeout", NotAnInput(OTHER_COMMAND)),
    ("python", NotAnInput("Python dependencies are installed by their own step")),
    ("raw_auth_config", NotAnInput(AUTH)),
    ("recursive", Through("the selection the fast path is scoped to")),
    ("recursive_install", Through("the selection the fast path is scoped to")),
    ("registries_by_prefix", Recorded { key: "registryPrefixes", change: |config| { config.registries_by_prefix.insert("foo".into(), "https://example.test/".into()); } }),
    ("registries_by_scope", Recorded { key: "registryRoutes", change: |config| { config.registries_by_scope.insert("@foo".into(), "https://example.test/".into()); } }),
    ("registry", NotAnInput("switching the default registry is a switch between mirrors of one registry")),
    ("registry_creds_by_uri", NotAnInput(AUTH)),
    ("registry_options_by_url", NotAnInput(NETWORK)),
    ("registry_supports_time_field", NotAnInput(RESOLUTION_ONLY)),
    ("remote_cache", NotAnInput(STORE_INTERNALS)),
    ("remote_side_effects_cache", NotAnInput(STORE_INTERNALS)),
    ("reporter", NotAnInput(CLI_ONLY)),
    ("reporter_hide_prefix", NotAnInput(CLI_ONLY)),
    ("required_scripts", NotAnInput(OTHER_COMMAND)),
    ("resolution_mode", NotAnInput(RESOLUTION_ONLY)),
    ("resolve_peers_from_workspace_root", NotAnInput(RESOLUTION_ONLY)),
    ("reverse", NotAnInput(CLI_ONLY)),
    ("runtime_on_fail", NotAnInput("whether a missing runtime fails the command")),
    ("save_catalog_name", NotAnInput(OTHER_COMMAND)),
    ("save_exact", NotAnInput(OTHER_COMMAND)),
    ("save_peer", NotAnInput(OTHER_COMMAND)),
    ("save_prefix", NotAnInput(OTHER_COMMAND)),
    ("save_types", NotAnInput(OTHER_COMMAND)),
    ("save_workspace_protocol", NotAnInput(OTHER_COMMAND)),
    ("scope", NotAnInput(OTHER_COMMAND)),
    ("script_shell", NotAnInput(SCRIPTS)),
    ("settings_read", NotAnInput("which settings the config loader reads")),
    ("shamefully_hoist", Through(DERIVED)),
    ("shared_workspace_lockfile", Through(LOCKFILE_LOCATION)),
    ("shell_emulator", NotAnInput(SCRIPTS)),
    ("side_effects_cache", NotAnInput(STORE_INTERNALS)),
    ("side_effects_cache_exclude", Recorded { key: "sideEffectsCache.exclude", change: |config| config.side_effects_cache_exclude = Some(vec!["foo".into()]) }),
    ("side_effects_cache_read_setting", NotAnInput(STORE_INTERNALS)),
    ("side_effects_cache_readonly", NotAnInput(STORE_INTERNALS)),
    ("side_effects_cache_write_setting", NotAnInput(STORE_INTERNALS)),
    ("skills_dirs", Recorded { key: "skills.dirs", change: |config| config.skills_dirs = Some(vec!["foo".into()]) }),
    ("skip_manifest_obfuscation", NotAnInput(OTHER_COMMAND)),
    ("skip_runtimes", Unseen("turned off, a full install links the runtimes it skipped, while `fresh_install_with_no_runtime_resolves_but_does_not_fetch_the_runtime` pins the fast path keeping them absent")),
    ("skip_store_dir_resolution", NotAnInput("how the config loader places the store")),
    ("sort", NotAnInput(CLI_ONLY)),
    ("source_date_epoch", NotAnInput(OTHER_COMMAND)),
    ("state_dir", NotAnInput("machine-local state, not the project's modules directory")),
    ("store_dir", Recorded { key: "storeDir", change: |config| { config.explicit_settings.insert("storeDir".into(), "/store".into()); config.store_dir = std::path::PathBuf::from("/store").into(); } }),
    ("store_dir_placement_skipped", NotAnInput("how the config loader places the store")),
    ("store_index_format", NotAnInput(STORE_INTERNALS)),
    ("store_relocation", NotAnInput("how the config loader places the store")),
    ("stream", NotAnInput(CLI_ONLY)),
    ("strict_dep_builds", Through("the ignored builds `.modules.yaml` records, checked after the fast path")),
    ("strict_peer_dependencies", NotAnInput("whether a peer dependency issue fails the install")),
    ("strict_store_pkg_content_check", NotAnInput(STORE_INTERNALS)),
    ("supported_architectures", Through("the layout's architectures, compared as `supportedArchitectures`")),
    ("sync_injected_deps_after_scripts", NotAnInput(OTHER_COMMAND)),
    ("symlink", Unseen("neither the fast path nor the frozen-tree check sees it, only an install into an empty modules directory applies it")),
    ("symlinked_lockfile_target_dirs", Through(LOCKFILE_LOCATION)),
    ("tag", NotAnInput(OTHER_COMMAND)),
    ("tag_version_prefix", NotAnInput(OTHER_COMMAND)),
    ("target_workspace_dir", NotAnInput("where resolution writes exclude entries")),
    ("tasks", NotAnInput(OTHER_COMMAND)),
    ("test_pattern", NotAnInput(OTHER_COMMAND)),
    ("tls", NotAnInput(NETWORK)),
    ("tls_by_uri", NotAnInput(NETWORK)),
    ("tools", NotAnInput("tools are installed by their own step")),
    ("trust_lockfile", Recorded { key: "trustLockfile", change: |config| config.trust_lockfile = !config.trust_lockfile }),
    ("trust_policy", Recorded { key: "trustPolicy", change: |config| { config.explicit_settings.insert("trustPolicy".into(), "off".into()); config.trust_policy = TrustPolicy::Off; } }),
    ("trust_policy_exclude", Recorded { key: "trustPolicyExclude", change: |config| config.trust_policy_exclude = Some(vec!["foo".into()]) }),
    ("trust_policy_exclude_prune", NotAnInput("prunes exclude entries from the workspace manifest")),
    ("trust_policy_ignore_after", Recorded { key: "trustPolicyIgnoreAfter", change: |config| config.trust_policy_ignore_after = Some(1) }),
    ("unsafe_perm", NotAnInput(SCRIPTS)),
    ("update_config", NotAnInput(OTHER_COMMAND)),
    ("update_notifier", NotAnInput(CLI_ONLY)),
    ("use_beta_cli", NotAnInput(CLI_ONLY)),
    ("use_git_branch_lockfile", Through(LOCKFILE_LOCATION)),
    ("use_stderr", NotAnInput(CLI_ONLY)),
    ("user_agent", NotAnInput(NETWORK)),
    ("verify_deps_before_run", NotAnInput("what `pnpm run` does with a stale tree")),
    ("verify_store_integrity", NotAnInput(STORE_INTERNALS)),
    ("versioning", NotAnInput(OTHER_COMMAND)),
    ("virtual_store_dir_max_length", Recorded { key: "virtualStoreDirMaxLength", change: |config| config.virtual_store_dir_max_length += 1 }),
    ("virtual_store_only", NotAnInput("only `pnpm fetch` sets it, which does not take the fast path")),
    ("workspace_concurrency", NotAnInput(SCRIPTS)),
    ("workspace_dir", Through("the workspace root the checks run in")),
    ("workspace_includes", Through("the project list, compared by the project-structure check")),
    ("workspace_key_issues", NotAnInput(CLI_ONLY)),
    ("workspace_package_patterns", Through("the project list, compared by the project-structure check")),
    ("workspace_registry_prefixes", NotAnInput(OTHER_COMMAND)),
    ("workspace_root", Through("the selection the fast path is scoped to")),
    ("workspace_search_skipped", NotAnInput("how the config loader finds the workspace")),
];

/// The names of `Config`'s fields, read from its source.
fn config_field_names() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/src/settings.rs");
    let source = std::fs::read_to_string(&path).expect("read the source of Config");
    let (_, body) =
        source.split_once("\npub struct Config {\n").expect("the source declares Config");
    let (body, _) = body.split_once("\n}\n").expect("Config's body ends");
    body.lines()
        .filter_map(|line| line.strip_prefix("    pub "))
        .filter_map(|field| field.split_once(':'))
        .map(|(name, _)| name.to_string())
        .collect()
}

#[test]
fn every_config_field_is_classified() {
    let classified: Vec<&str> = CLASSIFICATION
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let classified_set: BTreeSet<String> = classified
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(classified.len(), classified_set.len(), "a field is classified twice");
    let unexplained: Vec<_> = CLASSIFICATION
        .iter()
        .filter(|(_, reach)| reach.reason().is_some_and(str::is_empty))
        .map(|(name, _)| name)
        .collect();
    assert!(unexplained.is_empty(), "classified without a reason: {unexplained:?}");
    let fields = config_field_names();
    let unclassified: Vec<_> = fields.difference(&classified_set).collect();
    let removed: Vec<_> = classified_set.difference(&fields).collect();
    assert!(
        unclassified.is_empty() && removed.is_empty(),
        "classify every new Config field in CLASSIFICATION; unclassified: {unclassified:?}, no longer in Config: {removed:?}",
    );
}

#[test]
fn every_recorded_field_is_compared() {
    let base = Config::default();
    let state = recorded_state(&base);
    assert_eq!(install_gate_drift(&state, &base), None);
    for (name, reach) in CLASSIFICATION {
        let Recorded { key, change } = reach else { continue };
        let mut config = base.clone();
        change(&mut config);
        assert_eq!(install_gate_drift(&state, &config), Some(*key), "changing `{name}`");
    }
}

#[test]
fn every_blocking_field_blocks_the_fast_path() {
    let base = Config::default();
    assert_eq!(config_blocks_fast_path(&base), None);
    for (name, reach) in CLASSIFICATION {
        let BlocksFastPath(change) = reach else { continue };
        let mut config = base.clone();
        change(&mut config);
        assert!(config_blocks_fast_path(&config).is_some(), "changing `{name}`");
    }
}

fn recorded_state(config: &Config) -> WorkspaceState {
    WorkspaceState {
        settings: current_settings(config, NodeLinker::Isolated, isolated_included(), None),
        ..Default::default()
    }
}

fn install_gate_drift(state: &WorkspaceState, config: &Config) -> Option<&'static str> {
    first_setting_drift(state, config, NodeLinker::Isolated, isolated_included(), None, &[])
}
