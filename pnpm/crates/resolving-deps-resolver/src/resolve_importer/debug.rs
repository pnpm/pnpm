use super::{ManifestTransformHooks, ResolveImporterOptions};

impl std::fmt::Debug for ResolveImporterOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let overrider = self.resolution.override_bare_specifier
            .as_ref()
            .map(|_| "<overrider>");
        self.hooks.finish_debug(
            f
                .debug_struct("ResolveImporterOptions")
                .field("auto_install_peers", &self.peers.auto_install_peers)
                .field(
                    "auto_install_peers_from_highest_match",
                    &self.peers.auto_install_peers_from_highest_match,
                )
                .field(
                    "resolve_peers_from_workspace_root",
                    &self.peers.resolve_peers_from_workspace_root,
                )
                .field("dedupe_peers", &self.peers.dedupe_peers)
                .field("dedupe_peer_dependents", &self.peers.dedupe_peer_dependents)
                .field(
                    "all_preferred_versions",
                    &self.resolution.all_preferred_versions,
                )
                .field("override_bare_specifier", &overrider)
                .field(
                    "patched_dependencies",
                    &self.resolution.patched_dependencies,
                )
                .field("base_opts", &self.base_opts)
                .field("pick_lowest_direct", &self.resolution.pick_lowest_direct)
                .field("subdep_published_by", &self.resolution.subdep_published_by)
                .field("catalogs", &self.resolution.catalogs)
                .field(
                    "exclude_links_from_lockfile",
                    &self.links.exclude_links_from_lockfile,
                )
                .field("lockfile_dir", &self.links.lockfile_dir)
                .field("modules_dir", &self.links.modules_dir)
                .field("peers_suffix_max_length", &self.peers_suffix_max_length)
                .field("catalog_server", &self.resolution.catalog_server),
        )
    }
}

impl ManifestTransformHooks {
    fn finish_debug(&self, debug: &mut std::fmt::DebugStruct<'_, '_>) -> std::fmt::Result {
        debug
            .field(
                "manifest_hook",
                &self.manifest_hook
                    .as_ref()
                    .map(|_| "<hook>"),
            )
            .field(
                "overrides_hook",
                &self.overrides_hook
                    .as_ref()
                    .map(|_| "<hook>"),
            )
            .field(
                "pnpmfile_hook",
                &self.pnpmfile_hook
                    .as_ref()
                    .map(|_| "<hook>"),
            )
            .finish()
    }
}
