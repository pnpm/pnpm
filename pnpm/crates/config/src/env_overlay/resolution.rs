//! The peer and resolution settings of the environment overlay.

use super::{HoistingLimits, StringReader, WorkspaceSettings, parse_json, parse_json_or_string};

impl WorkspaceSettings {
    pub(super) fn read_peer_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, auto_install_peers, "AUTO_INSTALL_PEERS");
        json_field!(
            settings,
            reader,
            auto_install_peers_from_highest_match,
            "AUTO_INSTALL_PEERS_FROM_HIGHEST_MATCH"
        );
        json_field!(settings, reader, dedupe_peer_dependents, "DEDUPE_PEER_DEPENDENTS");
        json_field!(settings, reader, dedupe_peers, "DEDUPE_PEERS");
        json_field!(settings, reader, strict_peer_dependencies, "STRICT_PEER_DEPENDENCIES");
        json_field!(settings, reader, add_missing_peer_types, "ADD_MISSING_PEER_TYPES");
        json_field!(
            settings,
            reader,
            resolve_peers_from_workspace_root,
            "RESOLVE_PEERS_FROM_WORKSPACE_ROOT"
        );
    }

    pub(super) fn read_resolution_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, exclude_links_from_lockfile, "EXCLUDE_LINKS_FROM_LOCKFILE");
        json_field!(settings, reader, hoist_workspace_packages, "HOIST_WORKSPACE_PACKAGES");
        enum_field!(settings, reader, hoisting_limits, "HOISTING_LIMITS", HoistingLimits);
        json_field!(settings, reader, external_dependencies, "EXTERNAL_DEPENDENCIES");
        json_field!(settings, reader, auto_dedupe, "AUTO_DEDUPE");
        json_field!(settings, reader, dedupe_direct_deps, "DEDUPE_DIRECT_DEPS");
        json_field!(settings, reader, prefer_workspace_packages, "PREFER_WORKSPACE_PACKAGES");
        json_field!(settings, reader, dedupe_injected_deps, "DEDUPE_INJECTED_DEPS");
        json_field!(settings, reader, ignore_compatibility_db, "IGNORE_COMPATIBILITY_DB");
        json_field!(settings, reader, block_exotic_subdeps, "BLOCK_EXOTIC_SUBDEPS");
        json_field!(settings, reader, verify_store_integrity, "VERIFY_STORE_INTEGRITY");
        json_field!(
            settings,
            reader,
            strict_store_pkg_content_check,
            "STRICT_STORE_PKG_CONTENT_CHECK"
        );
        json_field!(settings, reader, include_workspace_root, "INCLUDE_WORKSPACE_ROOT");
        json_field!(settings, reader, fail_if_no_match, "FAIL_IF_NO_MATCH");
        json_field!(settings, reader, ignore_workspace_cycles, "IGNORE_WORKSPACE_CYCLES");
        json_field!(settings, reader, disallow_workspace_cycles, "DISALLOW_WORKSPACE_CYCLES");
        json_field!(settings, reader, side_effects_cache, "SIDE_EFFECTS_CACHE");
        json_field!(settings, reader, side_effects_cache_readonly, "SIDE_EFFECTS_CACHE_READONLY");
    }
}
