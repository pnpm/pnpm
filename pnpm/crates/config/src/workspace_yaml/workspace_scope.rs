use super::{IndexMap, RegistryEntry, WorkspaceSettings};

macro_rules! define_workspace_only_groups {
    ($( $clear_fn:ident { $( $field:ident ),* $(,)? } )+) => {
        impl WorkspaceSettings {
            $(
                pub(super) fn $clear_fn(&mut self) {
                    $( self.$field = None; )*
                }
            )+

            /// The camelCase keys [`Self::clear_workspace_only_fields`] zeroes,
            /// derived from the same field list so the two cannot drift.
            #[cfg(test)]
            pub(crate) fn workspace_only_keys() -> &'static super::HashSet<String> {
                static SET: std::sync::OnceLock<super::HashSet<String>> =
                    std::sync::OnceLock::new();
                SET.get_or_init(|| {
                    [
                        $( $( super::to_camel_case(stringify!($field)), )* )+
                    ]
                    .into_iter()
                    .collect()
                })
            }
        }
    };
}

define_workspace_only_groups! {
    clear_workspace_project_fields {
        versioning,
        publish_branch,
        cargo,
        python,
        packages,
        catalog,
        catalogs,
        boot_config,
        // Task declarations and pipelines describe the workspace's own scripts.
        tasks,
        pipelines,
        required_scripts,
        pipeline_base,
        enable_pre_post_scripts,
        workspace_concurrency,
        include_workspace_root,
        fail_if_no_match,
        ignore_workspace_cycles,
        disallow_workspace_cycles,
        ignore_workspace,
        stream,
        aggregate_output,
        reporter_hide_prefix,
        // A pnpmfile belongs to the project that ships it; honoring
        // `ignorePnpmfile` globally would drop a repository's hooks on one
        // machine and resolve a different graph.
        ignore_pnpmfile,
        only_built_dependencies,
        never_built_dependencies,
        ignored_built_dependencies,
    }
    clear_workspace_layout_fields {
        hoist,
        embed_readme,
        ignore_workspace_root_check,
        pending,
        recursive_install,
        reverse,
        skip_manifest_obfuscation,
        sort,
        hoist_pattern,
        public_hoist_pattern,
        shamefully_hoist,
        modules_dir,
        package_configs,
        node_linker,
        symlink,
        virtual_store_only,
        peers_suffix_max_length,
        lockfile,
        lockfile_dir,
        frozen_lockfile,
        deploy_all_files,
        force_legacy_deploy,
        shared_workspace_lockfile,
        git_branch_lockfile,
        merge_git_branch_lockfiles,
        merge_git_branch_lockfiles_branch_pattern,
        offline,
        lockfile_include_tarball_url,
        strict_store_pkg_content_check,
        runtime,
        runtime_on_fail,
        pm_on_fail,
    }
    clear_workspace_resolution_fields {
        auto_install_peers,
        auto_install_peers_from_highest_match,
        hoist_workspace_packages,
        link_workspace_packages,
        save_workspace_protocol,
        inject_workspace_packages,
        dedupe_peer_dependents,
        dedupe_peers,
        auto_dedupe,
        dedupe_direct_deps,
        prefer_workspace_packages,
        dedupe_injected_deps,
        strict_peer_dependencies,
        ignore_compatibility_db,
        add_missing_peer_types,
        resolve_peers_from_workspace_root,
        project_scoped_optional_peers,
        hoisting_limits,
        external_dependencies,
        catalog_mode,
        catalog_prune,
        cleanup_unused_catalogs,
        save_types,
        allowed_deprecated_versions,
        update,
        update_config,
        peer_dependency_rules,
        minimum_release_age_exclude_prune,
        trust_policy_exclude_prune,
        audit,
        audit_level,
        audit_config,
    }
    clear_workspace_hooks_fields {
        patched_dependencies,
        patches_dir,
        pnpmfile,
        config_dependencies,
        allow_builds,
        permissions,
        skills,
        supported_architectures,
        ignored_optional_dependencies,
        overrides,
        package_extensions,
        test_pattern,
        changed_files_ignore_pattern,
        legacy_dir_filtering,
        sync_injected_deps_after_scripts,
        allow_unused_patches,
        save_catalog_name,
        save_peer,
    }
}

impl WorkspaceSettings {
    /// Zero out the release-age and trust policies for `self-update`.
    ///
    /// `self-update` replaces the pnpm binary every later install runs
    /// through, so a repository must not get a say in whether it may be
    /// replaced. Both policies are dangerous in both directions here: a
    /// cooldown lowered waives the protection the user configured, raised it
    /// pins the machine to the installed pnpm — including past a release that
    /// fixes a vulnerability in it; a trust policy turned off accepts a pnpm
    /// release whose trust evidence the user meant to reject, turned on blocks
    /// the update the same way. Unlike a blocked dependency upgrade, those
    /// decisions follow the user out of the repository. The policies therefore
    /// come from the built-in defaults, the global `config.yaml`, and
    /// `PNPM_CONFIG_*` env vars only (plus CLI flags, applied by the caller).
    pub fn clear_self_update_policy(&mut self) {
        self.minimum_release_age = None;
        self.minimum_release_age_exclude = None;
        self.minimum_release_age_ignore_missing_time = None;
        self.minimum_release_age_strict = None;
        self.trust_policy = None;
        self.trust_policy_exclude = None;
        self.trust_policy_ignore_after = None;
        self.boot_config = None;
    }

    /// Zero out fields not permitted in the global `config.yaml`.
    ///
    /// Matches [`crate::config_types::is_config_file_key`]: every field listed
    /// in `define_workspace_only_groups!` is a workspace-only or CLI-only knob
    /// that pnpm reads from `pnpm-workspace.yaml`, the environment, or the
    /// command line, never from the global `config.yaml`.
    pub fn clear_workspace_only_fields(&mut self) {
        // Only the layout half of a registry declaration is workspace-only: it
        // decides which tarball URLs are omitted from the lockfile, so a
        // machine-local setting would make one developer write a lockfile
        // their collaborators read back with a different layout. The routes to
        // the registry are a legitimate global preference.
        for entry in self.registries.iter_mut().flat_map(IndexMap::values_mut) {
            if let RegistryEntry::Declaration(declaration) = entry {
                declaration.server_type = None;
            }
        }
        self.clear_workspace_project_fields();
        self.clear_workspace_layout_fields();
        self.clear_workspace_resolution_fields();
        self.clear_workspace_hooks_fields();
    }
}
