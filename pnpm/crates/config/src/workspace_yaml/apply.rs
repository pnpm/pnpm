use super::{
    Config, Path, PnpmfileSetting, ProxyKeys, ProxyValue, SideEffectsCacheSetting, StoreDir,
    UpdateConfig, WorkspaceSettings, decided_allow_builds, no_proxy_scalar, normalize_registry_url,
    overlay, overlay_some, registries, resolve, resolve_child_concurrency, warn_deprecated_pairing,
};

impl WorkspaceSettings {
    /// Apply every set field onto `config`, leaving unset ones untouched.
    ///
    /// Path-valued settings are resolved against `base_dir` if relative —
    /// anchored at the workspace root where the yaml was found, matching pnpm.
    /// `scriptShell` is the exception; see [`Self::resolve_script_shell`].
    pub fn apply_to(mut self, config: &mut Config, base_dir: &Path) {
        self.apply_proxy_to(&mut config.proxy, &mut config.proxy_keys);

        // Captured before the `apply!` macro and audit if-lets below move
        // these out of `self`; consumed after, to warn on the redundant
        // combination of a new section key and its deprecated counterpart.
        let update_config_in_yaml = self.update_config.is_some();
        let audit_level_in_yaml = self.audit_level.is_some();
        let audit_config_in_yaml = self.audit_config.is_some();

        // `catalogPrune`'s former name, applied before the macro so the
        // canonical key wins when a file carries both.
        overlay(&mut config.catalog_prune, self.cleanup_unused_catalogs.take());

        // Tri-state on `Config`: `exec` treats "never asked" differently
        // from an explicit `false`, so the macro's "apply when set" shape
        // would collapse the distinction.
        overlay_some(&mut config.reporter_hide_prefix, self.reporter_hide_prefix.take());

        // pnpm spells the setting `gitBranchLockfile` and exposes the
        // resolved answer as `useGitBranchLockfile`; the macro below can
        // only apply fields the two structs name identically.
        overlay(&mut config.use_git_branch_lockfile, self.git_branch_lockfile.take());

        // `virtualStoreType` is the canonical spelling of the boolean
        // `enableGlobalVirtualStore`, which the macro below applies. Both
        // land in the same field, so applying this after the macro is what
        // makes the canonical key win when a file carries both.
        let virtual_store_type = self.virtual_store_type.take();

        macro_rules! apply {
            ($($field:ident),* $(,)?) => {$(
                overlay(&mut config.$field, self.$field.take());
            )*};
        }

        identically_named_settings!(apply);

        overlay_some(&mut config.pipeline_base, self.pipeline_base.take());

        if let Some(virtual_store_type) = virtual_store_type {
            config.enable_global_virtual_store = virtual_store_type.is_global();
        }

        // `globalShims` merges key-wise instead of replacing,
        // so a layer can flip one package without restating the defaults.
        if let Some(global_shims) = self.global_shims.take() {
            config.global_shims.apply(&global_shims);
        }

        self.apply_update_settings(config, update_config_in_yaml);

        self.apply_optional_settings(config);

        self.apply_path_settings(config, base_dir);
        self.apply_registry_settings(config);
        self.apply_project_settings(config, base_dir);
        self.apply_process_settings(config);
        self.apply_resolution_settings(config, base_dir);
        self.apply_policy_settings(config, audit_level_in_yaml, audit_config_in_yaml);
    }

    pub(super) fn apply_update_settings(
        &mut self,
        config: &mut Config,
        update_config_in_yaml: bool,
    ) {
        // The `update` section supersedes the deprecated `updateConfig`.
        // Applied after the macro so it overrides an `updateConfig` set in
        // the same file; both together is redundant and warned about.
        if let Some(update) = self.update.take() {
            if update_config_in_yaml {
                tracing::warn!(
                    target: "pacquet::config",
                    r#"Both the "update" and "updateConfig" settings are set. The deprecated "updateConfig" setting is ignored in favor of "update"."#,
                );
            }
            // The `update` section is authoritative when present, superseding
            // any deprecated `updateConfig`.
            config.update_config = UpdateConfig {
                ignore_dependencies: update.ignore_deps,
                changeset: update.changeset,
                github_actions: update.github_actions,
                github_actions_server: update.github_actions_server,
            };
        }
    }

    pub(super) fn apply_optional_settings(&mut self, config: &mut Config) {
        overlay_some(&mut config.frozen_lockfile, self.frozen_lockfile.take());
        overlay_some(
            &mut config.prefer_symlinked_executables,
            self.prefer_symlinked_executables.take(),
        );
        overlay_some(&mut config.save_catalog_name, self.save_catalog_name.take());
        overlay_some(&mut config.init_author_name, self.init_author_name.take());
        overlay_some(&mut config.init_author_email, self.init_author_email.take());
        overlay_some(&mut config.init_author_url, self.init_author_url.take());
        overlay_some(&mut config.init_license, self.init_license.take());
        overlay_some(&mut config.init_version, self.init_version.take());
        overlay_some(&mut config.save_prefix, self.save_prefix.take());

        overlay(&mut config.hoist_pattern, self.hoist_pattern.take());
        overlay(&mut config.public_hoist_pattern, self.public_hoist_pattern.take());

        // Applied AFTER `hoist_pattern` assignment so a yaml that sets
        // both `hoist: false` and `hoistPattern: ["..."]` still
        // disables — `hoist: false` wins.
        if !config.hoist {
            config.hoist_pattern = None;
        }
    }

    /// Path-valued settings, each resolved against `base_dir` when relative.
    pub(super) fn apply_path_settings(&mut self, config: &mut Config, base_dir: &Path) {
        if let Some(v) = self.modules_dir.take() {
            config.modules_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.virtual_store_dir.take() {
            config.virtual_store_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.global_virtual_store_dir.take() {
            config.global_virtual_store_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.global_dir.take() {
            config.global_dir = Some(resolve(base_dir, &v));
        }
        if let Some(v) = self.global_bin_dir.take() {
            config.global_bin_dir = Some(resolve(base_dir, &v));
        }
        // Last of the path-valued settings: pinning the lockfile dir
        // re-resolves `modulesDir` / `virtualStoreDir` against it, so it
        // must see whatever this layer just set.
        if let Some(v) = self.lockfile_dir.take() {
            config.pin_lockfile_dir(&resolve(base_dir, &v));
        }
        if let Some(v) = self.store_dir.take() {
            config.store_dir = StoreDir::from(resolve(base_dir, &v));
        }
    }

    /// Registry endpoints, their credentials, and the caches keyed by them.
    pub(super) fn apply_registry_settings(&mut self, config: &mut Config) {
        let declared_prefixes = self.apply_registry_declarations(config);
        if let Some(v) = self.registry.take() {
            config.registry = normalize_registry_url(&v);
        }
        overlay_some(&mut config.scope, self.scope.take());
        overlay_some(&mut config.pnpr_server, self.pnpr_server.take());
        overlay(&mut config.cargo, self.cargo.take());
        overlay(&mut config.python, self.python.take());
        if let Some(v) = self.remote_side_effects_cache.take() {
            config.remote_side_effects_cache.get_or_insert_default().overlay(v);
        }
        self.apply_side_effects_cache(config);
        self.apply_named_registries(config, declared_prefixes);
    }

    /// The `registries` declarations, reporting whether any of them declared
    /// a prefix.
    pub(super) fn apply_registry_declarations(&mut self, config: &mut Config) -> bool {
        let Some(entries) = self.registries.take() else {
            return false;
        };
        let lookups = registries::into_lookups(entries);
        if let Some(registry) = lookups.default_registry {
            config.registry = registry;
        }
        let declared_prefixes = !lookups.registries_by_prefix.is_empty();
        config.registries_by_scope.extend(lookups.registries_by_scope);
        config.registries_by_prefix.extend(lookups.registries_by_prefix);
        config.registry_options_by_url.extend(lookups.registry_options_by_url);
        declared_prefixes
    }

    /// The canonical declaration is applied after the alias so that it wins
    /// where both are set, and its `remote` half overlays rather than
    /// replaces: a repository names the organization while the machine
    /// supplies the signing key, and neither may drop the other's fields.
    pub(super) fn apply_side_effects_cache(&mut self, config: &mut Config) {
        match self.side_effects_cache.take() {
            Some(SideEffectsCacheSetting::Enabled(enabled)) => {
                config.apply_side_effects_cache_shorthand(enabled);
            }
            Some(SideEffectsCacheSetting::Settings(settings)) => {
                config.side_effects_cache_read_setting = Some(settings.read.unwrap_or(true));
                config.side_effects_cache_write_setting = Some(settings.write.unwrap_or(true));
                if let Some(remote) = settings.remote {
                    config.remote_side_effects_cache.get_or_insert_default().overlay(remote);
                }
            }
            None => {}
        }
    }

    /// A prefix a `registries` entry declares wins: `namedRegistries` is the
    /// deprecated spelling of the same thing.
    pub(super) fn apply_named_registries(&mut self, config: &mut Config, declared_prefixes: bool) {
        let Some(named) = self.named_registries.take() else {
            return;
        };
        if declared_prefixes {
            tracing::warn!(
                target: "pacquet::config",
                r#"Both the "registries" and "namedRegistries" settings declare registry prefixes. The deprecated "namedRegistries" setting is only read for prefixes "registries" does not declare."#,
            );
        }
        for (name, registry) in named {
            config.registries_by_prefix.entry(name).or_insert(registry);
        }
    }

    /// Settings describing the projects themselves: hooks, patches, build
    /// policy and the runtime they run under.
    pub(super) fn apply_project_settings(&mut self, config: &mut Config, base_dir: &Path) {
        config.workspace_dir = Some(base_dir.to_path_buf());
        overlay_some(&mut config.patched_dependencies, self.patched_dependencies.take());
        overlay_some(&mut config.patches_dir, self.patches_dir.take());
        if let Some(path) = self.global_pnpmfile.take() {
            config.global_pnpmfile = Some(pnpm_fs::lexical_normalize(&base_dir.join(path)));
        }
        if let Some(pnpmfile) = self.pnpmfile.take() {
            let paths = match pnpmfile {
                PnpmfileSetting::Single(path) => vec![path],
                PnpmfileSetting::Multiple(paths) => paths,
            };
            config.pnpmfile = Some(
                paths
                    .into_iter()
                    .map(|path| pnpm_fs::lexical_normalize(&base_dir.join(path)))
                    .collect(),
            );
        }
        overlay_some(&mut config.config_dependencies, self.config_dependencies.take());
        if let Some(v) = self.allow_builds.take() {
            config.allow_builds = decided_allow_builds(v);
        }
        overlay(&mut config.dangerously_allow_all_builds, self.dangerously_allow_all_builds.take());
        overlay(&mut config.strict_dep_builds, self.strict_dep_builds.take());
        overlay(&mut config.ignore_scripts, self.ignore_scripts.take());
        overlay(&mut config.ignore_pnpmfile, self.ignore_pnpmfile.take());
        overlay(&mut config.git_checks, self.git_checks.take());
        overlay(&mut config.engine_strict, self.engine_strict.take());
        overlay_some(&mut config.node_version, self.node_version.take());
        overlay_some(&mut config.runtime_on_fail, self.runtime_on_fail.take());
        overlay(&mut config.node_download_mirrors, self.node_download_mirrors.take());
    }

    /// Settings that shape the processes an install spawns.
    pub(super) fn apply_process_settings(&mut self, config: &mut Config) {
        // npm's spelling first, so the canonical one wins when a single
        // file carries both.
        overlay_some(&mut config.max_sockets, self.maxsockets.take());
        overlay_some(&mut config.max_sockets, self.max_sockets.take());
        overlay(&mut config.scripts_prepend_node_path, self.scripts_prepend_node_path.take());
        overlay(&mut config.script_shell, self.script_shell.take());
        overlay(&mut config.node_options, self.node_options.take());
        overlay(&mut config.unsafe_perm, self.unsafe_perm.take());
        if cfg!(windows) {
            config.unsafe_perm = true;
        }
        if let Some(v) = self.child_concurrency.take() {
            config.child_concurrency = resolve_child_concurrency(Some(v));
        }
        if let Some(v) = self.workspace_concurrency.take() {
            config.workspace_concurrency = resolve_child_concurrency(Some(v));
        }
        overlay_some(&mut config.supported_architectures, self.supported_architectures.take());
        overlay_some(
            &mut config.ignored_optional_dependencies,
            self.ignored_optional_dependencies.take(),
        );
    }

    /// Settings that rewrite what resolution sees.
    pub(super) fn apply_resolution_settings(&mut self, config: &mut Config, base_dir: &Path) {
        // `$dep-name` self-references are resolved by
        // [`crate::override_version_references::resolve_version_references`]
        // once the cascade knows the workspace root, whose manifest
        // carries the direct dependencies they point at.
        if let Some(v) = self.overrides.take() {
            config.overrides = (!v.is_empty()).then_some(v);
        }
        if let Some(v) = self.package_extensions.take() {
            config.package_extensions = (!v.is_empty()).then_some(v);
        }
        if let Some(v) = self.package_configs.take() {
            let record = v.into_record();
            config.package_configs = (!record.is_empty()).then_some(record);
        }
        if let Some(v) = self.cache_dir.take() {
            config.cache_dir = resolve(base_dir, &v);
        }
    }

    /// Release-age, trust and audit policy.
    pub(super) fn apply_policy_settings(
        &mut self,
        config: &mut Config,
        audit_level_in_yaml: bool,
        audit_config_in_yaml: bool,
    ) {
        overlay_some(&mut config.minimum_release_age, self.minimum_release_age.take());
        overlay_some(
            &mut config.minimum_release_age_exclude,
            self.minimum_release_age_exclude.take(),
        );
        overlay(
            &mut config.minimum_release_age_ignore_missing_time,
            self.minimum_release_age_ignore_missing_time.take(),
        );
        overlay_some(
            &mut config.minimum_release_age_strict,
            self.minimum_release_age_strict.take(),
        );
        overlay(&mut config.trust_lockfile, self.trust_lockfile.take());
        overlay(&mut config.trust_policy, self.trust_policy.take());
        overlay_some(&mut config.pm_on_fail, self.pm_on_fail.take());
        overlay_some(&mut config.audit_level, self.audit_level.take());
        overlay(&mut config.audit_config, self.audit_config.take());

        self.apply_audit_section(config, audit_level_in_yaml, audit_config_in_yaml);
        overlay(&mut config.versioning, self.versioning.take());
        overlay_some(&mut config.trust_policy_exclude, self.trust_policy_exclude.take());
        overlay(&mut config.trust_policy_exclude_prune, self.trust_policy_exclude_prune.take());
        overlay_some(&mut config.trust_policy_ignore_after, self.trust_policy_ignore_after.take());
    }

    /// The `audit` section supersedes the deprecated `auditLevel` and
    /// `auditConfig`. Applied after them so it overrides values set in the
    /// same file; each redundant pairing is warned about.
    pub(super) fn apply_audit_section(
        &mut self,
        config: &mut Config,
        audit_level_in_yaml: bool,
        audit_config_in_yaml: bool,
    ) {
        let Some(audit) = self.audit.take() else {
            return;
        };
        if let Some(level) = audit.level {
            warn_deprecated_pairing(audit_level_in_yaml, "auditLevel");
            config.audit_level = Some(level);
        }
        if let Some(ignore) = audit.ignore {
            warn_deprecated_pairing(audit_config_in_yaml, "auditConfig");
            config.audit_config.ignore_ghsas = ignore;
        }
        overlay_some(&mut config.audit_ignore_prune, audit.ignore_prune);
    }

    /// Overlay this file's proxy keys onto the merged view and re-resolve.
    ///
    /// A key named here occupies it even when the value reads as unset —
    /// see the [`crate::proxy_keys`] module docs.
    pub(crate) fn apply_proxy_to(
        &self,
        proxy_config: &mut pnpm_network::ProxyConfig,
        keys: &mut ProxyKeys,
    ) {
        for (key, raw) in [
            (&mut keys.https_proxy, self.https_proxy.as_deref()),
            (&mut keys.http_proxy, self.http_proxy.as_deref()),
        ] {
            if let Some(raw) = raw {
                *key = ProxyValue::from_config(raw);
            }
        }
        if let Some(raw) = self.proxy.as_deref() {
            keys.legacy_proxy = ProxyValue::legacy_from_config(raw);
        }
        for (key, raw) in [
            (&mut keys.no_proxy, self.no_proxy.as_ref()),
            (&mut keys.noproxy, self.noproxy.as_ref()),
        ] {
            if let Some(raw) = raw {
                *key = ProxyValue::from_config(&no_proxy_scalar(raw));
            }
        }
        *proxy_config = keys.resolve();
    }
}
