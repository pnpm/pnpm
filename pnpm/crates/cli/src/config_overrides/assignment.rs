use super::{
    ColorMode, ConfigOverrides, normalize_registry_url, parse_bool, parse_bool_or_enum, parse_enum,
    scoped_registry_key,
};

impl ConfigOverrides {
    pub(super) fn set(&mut self, key: &str, value: &str) {
        self.set_boolean_install_option(key, value);
        self.set_boolean_execution_option(key, value);
        self.set_network_option(key, value);
        self.set_dependency_policy_option(key, value);
        self.set_layout_option(key, value);
        self.set_install_execution_option(key, value);
        if let Some(scope) = scoped_registry_key(key) {
            self.registries.insert(scope.to_owned(), normalize_registry_url(value));
        }
    }

    pub(super) fn set_boolean_install_option(&mut self, key: &str, value: &str) {
        match key {
            "allow-unused-patches" => self.allow_unused_patches = parse_bool(value),
            "dangerously-allow-all-builds" => {
                self.dangerously_allow_all_builds = parse_bool(value);
            }
            "engine-strict" => self.engine_strict = parse_bool(value),
            "frozen-store" => self.frozen_store = parse_bool(value),
            "hoist" => self.hoist = parse_bool(value),
            "ignore-pnpmfile" => self.ignore_pnpmfile = parse_bool(value),
            "link-workspace-packages" => {
                self.link_workspace_packages = parse_bool_or_enum(value);
            }
            "lockfile" => self.lockfile = parse_bool(value),
            "lockfile-include-tarball-url" => {
                self.lockfile_include_tarball_url = parse_bool(value);
            }
            "merge-git-branch-lockfiles" => {
                self.merge_git_branch_lockfiles = parse_bool(value);
            }
            "offline" => self.offline = parse_bool(value),
            "optimistic-repeat-install" => self.optimistic_repeat_install = parse_bool(value),
            "optional" => self.optional = parse_bool(value),
            "package-lock" => self.package_lock = parse_bool(value),
            "prefer-frozen-lockfile" => self.prefer_frozen_lockfile = parse_bool(value),
            "prefer-offline" => self.prefer_offline = parse_bool(value),
            "save-workspace-protocol" => {
                self.save_workspace_protocol = parse_bool_or_enum(value);
            }
            "shamefully-hoist" => self.shamefully_hoist = parse_bool(value),
            "side-effects-cache" => self.side_effects_cache = parse_bool(value),
            "side-effects-cache-readonly" => {
                self.side_effects_cache_readonly = parse_bool(value);
            }
            "strict-peer-dependencies" => self.strict_peer_dependencies = parse_bool(value),
            "trust-lockfile" => self.trust_lockfile = parse_bool(value),
            "verify-store-integrity" => self.verify_store_integrity = parse_bool(value),
            "virtual-store-only" => self.virtual_store_only = parse_bool(value),
            _ => {}
        }
    }

    pub(super) fn set_boolean_execution_option(&mut self, key: &str, value: &str) {
        match key {
            "bail" => self.bail = parse_bool(value),
            "ci" => self.ci = parse_bool(value),
            "color" => {
                self.color = parse_bool(value)
                    .map(|enabled| {
                        if enabled {
                            ColorMode::Always
                        } else {
                            ColorMode::Never
                        }
                    })
                    .or_else(|| parse_enum(value));
            }
            "embed-readme" => self.embed_readme = parse_bool(value),
            "ignore-workspace-root-check" => {
                self.ignore_workspace_root_check = parse_bool(value);
            }
            "node-experimental-package-map" => {
                self.node_experimental_package_map = parse_bool(value);
            }
            "pending" => self.pending = parse_bool(value),
            "recursive-install" => self.recursive_install = parse_bool(value),
            "reverse" => self.reverse = parse_bool(value),
            "shell-emulator" => self.shell_emulator = parse_bool(value),
            "skip-manifest-obfuscation" => {
                self.skip_manifest_obfuscation = parse_bool(value);
            }
            "sort" => self.sort = parse_bool(value),
            "unsafe-perm" => self.unsafe_perm = parse_bool(value),
            "use-beta-cli" => self.use_beta_cli = parse_bool(value),
            _ => {}
        }
    }

    pub(super) fn set_network_option(&mut self, key: &str, value: &str) {
        match key {
            "registry" => {
                self.registry = Some(normalize_registry_url(value));
            }
            "scope" => {
                self.scope = Some(value.to_string());
            }
            "https-proxy" => {
                self.https_proxy = Some(value.to_string());
            }
            "http-proxy" => {
                self.http_proxy = Some(value.to_string());
            }
            "no-proxy" => {
                self.no_proxy = Some(value.to_string());
            }
            "maxsockets" => {
                self.maxsockets = value.parse().ok();
            }
            "max-sockets" => {
                self.max_sockets = value.parse().ok();
            }
            _ => {}
        }
    }

    pub(super) fn set_dependency_policy_option(&mut self, key: &str, value: &str) {
        self.set_release_age_option(key, value);
        match key {
            "pm-on-fail" => {
                self.pm_on_fail = parse_enum(value);
            }
            "runtime-on-fail" => {
                self.runtime_on_fail = parse_enum(value);
            }
            "verify-deps-before-run" => {
                self.verify_deps_before_run = value.parse().ok();
            }
            "trust-policy" => {
                self.trust_policy = parse_enum(value);
            }
            "trust-policy-exclude" => {
                self.trust_policy_exclude
                    .get_or_insert_default()
                    .push(value.to_string());
            }
            "trust-policy-ignore-after" => {
                self.trust_policy_ignore_after = value.parse().ok();
            }
            _ => {}
        }
    }

    pub(super) fn set_layout_option(&mut self, key: &str, value: &str) {
        match key {
            "global-dir" => {
                self.global_dir = Some(value.to_string());
            }
            "hoist-pattern" => {
                self.hoist_pattern
                    .get_or_insert_default()
                    .push(value.to_string());
            }
            "modules-dir" => {
                self.modules_dir = Some(value.to_string());
            }
            "node-linker" => {
                self.node_linker = parse_enum(value);
            }
            "public-hoist-pattern" => {
                self.public_hoist_pattern
                    .get_or_insert_default()
                    .push(value.to_string());
            }
            "virtual-store-dir" => {
                self.virtual_store_dir = Some(value.to_string());
            }
            _ => {}
        }
    }

    pub(super) fn set_install_execution_option(&mut self, key: &str, value: &str) {
        match key {
            "child-concurrency" => {
                self.child_concurrency = value.parse().ok();
            }
            "deploy-all-files" => {
                self.deploy_all_files = parse_bool(value);
            }
            "force-legacy-deploy" => {
                self.force_legacy_deploy = parse_bool(value);
            }
            "ignore-scripts" => {
                self.ignore_scripts = parse_bool(value);
            }
            "inject-workspace-packages" => {
                self.inject_workspace_packages = parse_bool(value);
            }
            "package-import-method" => {
                self.package_import_method = parse_enum(value);
            }
            "shared-workspace-lockfile" => {
                self.shared_workspace_lockfile = parse_bool(value);
            }
            _ => {}
        }
    }

    pub(super) fn set_release_age_option(&mut self, key: &str, value: &str) {
        match key {
            "minimum-release-age" => {
                self.minimum_release_age = value.parse().ok();
            }
            "minimum-release-age-exclude" => {
                // nopt collects a repeated key it has no type for into a list,
                // and pnpm re-parses the `--config.` tokens without any types.
                self.minimum_release_age_exclude
                    .get_or_insert_default()
                    .push(value.to_string());
            }
            "minimum-release-age-ignore-missing-time" => {
                self.minimum_release_age_ignore_missing_time = parse_bool(value);
            }
            "minimum-release-age-strict" => {
                self.minimum_release_age_strict = parse_bool(value);
            }
            _ => {}
        }
    }
}
