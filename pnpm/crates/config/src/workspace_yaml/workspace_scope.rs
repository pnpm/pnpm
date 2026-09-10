use super::{BTreeMap, RegistryEntry, WorkspaceSettings};

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
    }

    /// Zero out fields not permitted in the global `config.yaml`.
    ///
    /// Every field listed here is a key excluded from the global
    /// config, plus the programmatic-only and workspace-only knobs
    /// (`patchedDependencies`, `allowBuilds`,
    /// `supportedArchitectures`, `ignoredOptionalDependencies`,
    /// `hoistingLimits`, `externalDependencies`) that pnpm only reads
    /// from `pnpm-workspace.yaml` or the legacy `package.json#pnpm`
    /// field. Without this filter a user could put `nodeLinker:
    /// hoisted` in `~/.config/pnpm/config.yaml` and pacquet would
    /// honor it while pnpm wouldn't — anti-parity.
    pub fn clear_workspace_only_fields(&mut self) {
        // Only the layout half of a registry declaration is workspace-only: it
        // decides which tarball URLs are omitted from the lockfile, so a
        // machine-local setting would make one developer write a lockfile
        // their collaborators read back with a different layout. The routes to
        // the registry are a legitimate global preference.
        for entry in self.registries.iter_mut().flat_map(BTreeMap::values_mut) {
            if let RegistryEntry::Declaration(declaration) = entry {
                declaration.server_type = None;
            }
        }
        self.clear_workspace_project_fields();
        self.clear_workspace_layout_fields();
        self.clear_workspace_resolution_fields();
        self.clear_workspace_hooks_fields();
    }

    pub(super) fn clear_workspace_project_fields(&mut self) {
        self.versioning = None;
        self.cargo = None;
        self.python = None;
        self.packages = None;
        self.catalog = None;
        // Task declarations describe the workspace's own scripts; pnpm's
        // config-file key filter drops them from the global file too. The
        // same holds for the pipelines built from them.
        self.tasks = None;
        self.pipelines = None;
        self.pipeline_base = None;
        // A pnpmfile belongs to the project that ships it, and pnpm reads
        // `ignorePnpmfile` from `pnpm-workspace.yaml` and the environment but
        // not from here. Honoring it globally would silently drop a
        // repository's hooks on one machine and resolve a different graph.
        self.ignore_pnpmfile = None;
        self.catalogs = None;
        self.only_built_dependencies = None;
        self.never_built_dependencies = None;
        self.ignored_built_dependencies = None;
    }

    pub(super) fn clear_workspace_layout_fields(&mut self) {
        self.hoist = None;
        self.embed_readme = None;
        self.ignore_workspace_root_check = None;
        self.pending = None;
        self.recursive_install = None;
        self.reverse = None;
        self.skip_manifest_obfuscation = None;
        self.sort = None;
        self.hoist_pattern = None;
        self.public_hoist_pattern = None;
        self.shamefully_hoist = None;
        self.modules_dir = None;
        self.package_configs = None;
        self.node_linker = None;
        self.symlink = None;
        self.lockfile = None;
        self.frozen_lockfile = None;
        self.deploy_all_files = None;
        self.force_legacy_deploy = None;
        self.shared_workspace_lockfile = None;
        self.git_branch_lockfile = None;
        self.merge_git_branch_lockfiles = None;
        self.merge_git_branch_lockfiles_branch_pattern = None;
        self.offline = None;
        self.lockfile_include_tarball_url = None;
    }

    pub(super) fn clear_workspace_resolution_fields(&mut self) {
        self.auto_install_peers = None;
        self.auto_install_peers_from_highest_match = None;
        self.exclude_links_from_lockfile = None;
        self.hoist_workspace_packages = None;
        self.link_workspace_packages = None;
        self.save_workspace_protocol = None;
        self.inject_workspace_packages = None;
        self.dedupe_peer_dependents = None;
        self.dedupe_peers = None;
        self.dedupe_direct_deps = None;
        self.prefer_workspace_packages = None;
        self.dedupe_injected_deps = None;
        self.strict_peer_dependencies = None;
        self.ignore_compatibility_db = None;
        self.resolve_peers_from_workspace_root = None;
        self.block_exotic_subdeps = None;
        self.hoisting_limits = None;
        self.external_dependencies = None;
    }

    pub(super) fn clear_workspace_hooks_fields(&mut self) {
        self.patched_dependencies = None;
        self.pnpmfile = None;
        self.config_dependencies = None;
        self.allow_builds = None;
        self.supported_architectures = None;
        self.ignored_optional_dependencies = None;
        self.overrides = None;
        self.package_extensions = None;
        self.test_pattern = None;
        self.changed_files_ignore_pattern = None;
        self.legacy_dir_filtering = None;
        self.sync_injected_deps_after_scripts = None;
        self.allow_unused_patches = None;
        self.save_catalog_name = None;
        self.save_peer = None;
    }
}
