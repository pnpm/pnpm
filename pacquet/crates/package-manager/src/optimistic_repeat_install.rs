//! Pre-install fast path: when nothing has changed since the last
//! install, skip the entire pipeline.
//!
//! Port of upstream's `optimisticRepeatInstall` + [`checkDepsStatus`]
//! dispatch. `installDeps` calls `checkDepsStatus` before any of the
//! install setup runs and logs "Already up to date" when nothing has
//! changed. The check keys off `<workspace_root>/node_modules/.pnpm-workspace-state-v1.json`'s
//! `lastValidatedTimestamp` against each project's `package.json`
//! mtime — never touching the lockfile, the verifier cache, or any
//! resolver state.
//!
//! Mirrors:
//! - [`installing/commands/src/installDeps.ts:179-194`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/commands/src/installDeps.ts#L179-L194)
//!   — dispatch.
//! - [`deps/status/src/checkDepsStatus.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts)
//!   — the underlying check.
//!
//! Scope: the mtime-vs-`lastValidatedTimestamp` branch (upstream's
//! `modifiedProjects.length === 0` exit at lines 263-271), plus the
//! patch-file branch of upstream's
//! [`patchesOrHooksAreModified`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L597-L612):
//! a configured patch file whose mtime is newer than
//! `lastValidatedTimestamp` invalidates the fast path even when its
//! `patchedDependencies` config entry is unchanged (a content edit the
//! key→path settings comparison can't see). The pnpmfile branch of
//! `patchesOrHooksAreModified` and the `assertWantedLockfileUpToDate`
//! re-verification are NOT ported here. When this function returns
//! `Decision::Skipped` the caller proceeds with the full install path,
//! which still has its own freshness guards (`check_lockfile_freshness`,
//! the no-op short-circuit). Remaining work tracked at
//! pnpm/pnpm#11940 (this issue).
//!
//! [`checkDepsStatus`]: https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts
//!
//! ## Why a separate module
//!
//! Lives in `pacquet-package-manager` rather than a new
//! `pacquet-deps-status` crate because the only call site today is
//! `Install::run`. When pacquet ports `verifyDepsBeforeRun` (the
//! second consumer of `checkDepsStatus` upstream), extract this into
//! its own crate to match pnpm's `@pnpm/deps.status` package.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use pacquet_config::{Config, LinkWorkspacePackages, NodeLinker};
use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manifest::PackageManifest;
use pacquet_workspace_state::{
    NodeLinker as WorkspaceStateNodeLinker, WorkspaceState, WorkspaceStateSettings,
    load_workspace_state,
};

/// Outcome of [`check_optimistic_repeat_install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The install is fully up to date — emit "Already up to date"
    /// and exit before any of the install setup runs. Mirrors pnpm's
    /// [`upToDate: true`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L270)
    /// outcome.
    UpToDate,
    /// Fall through to the full install path. `reason` is the short
    /// string the upstream `issue` field would carry; surfaced via
    /// `tracing::debug!` for diagnosability without contaminating the
    /// reporter stream.
    Skipped { reason: &'static str },
}

/// Run the workspace-state freshness fast path. Returns
/// [`Decision::UpToDate`] when the install can short-circuit.
///
/// `workspace_root` is the directory containing `pnpm-workspace.yaml`
/// (or the project root when no workspace manifest exists — same
/// fallback as [`Install::run`](crate::Install::run)).
///
/// `project_manifests` lists every importer's `(root_dir, manifest)`
/// pair. For a single-project install it's just the root manifest;
/// for a workspace install it's every project the resolver would
/// otherwise walk. The caller passes this in (rather than this
/// function rediscovering it) so the same walk seeds the regular
/// install path on the fall-through.
///
/// `is_workspace_install` is `true` when a `pnpm-workspace.yaml`
/// drives the install — that selects pnpm's
/// [`allProjects && workspaceDir` branch](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L187)
/// which exits purely on the per-manifest mtime check. `false` (no
/// workspace manifest) selects the
/// [single-project branch](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L387-L462)
/// which additionally requires `pnpm-lock.yaml` to exist on disk —
/// pnpm throws `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` otherwise, which
/// the outer `try` converts into `upToDate: false`.
///
/// Always returns `Decision::Skipped` when
/// `config.optimistic_repeat_install` is `false`.
pub fn check_optimistic_repeat_install(
    workspace_root: &Path,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    project_manifests: &[(PathBuf, &PackageManifest)],
    is_workspace_install: bool,
) -> Decision {
    if !config.optimistic_repeat_install {
        return Decision::Skipped { reason: "optimistic_repeat_install disabled" };
    }

    // No workspace state means no previous install has completed
    // (or the file was deleted) — there's no `lastValidatedTimestamp`
    // to compare against. Mirrors upstream's
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L80-L86>
    // first-return guard.
    let Ok(Some(state)) = load_workspace_state(workspace_root) else {
        return Decision::Skipped { reason: "no workspace state on disk" };
    };

    if !settings_match(&state, config, node_linker, included) {
        return Decision::Skipped { reason: "settings drift" };
    }

    if !project_structure_matches(&state, project_manifests) {
        return Decision::Skipped { reason: "workspace project list changed" };
    }

    // The "modules dir exists when the project has deps" gate from
    // upstream's
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L237-L249>:
    // a project with `dependencies`/`devDependencies` but no
    // `node_modules` cannot be up to date. Pnpm reads `modulesDir`
    // off the per-project config; pacquet doesn't track per-importer
    // overrides yet, so check the install-time `config.modules_dir`
    // for the root + `<project_root>/node_modules` for siblings,
    // matching pnpm's `isolated`-linker default.
    if !modules_dirs_present(config, project_manifests) {
        return Decision::Skipped {
            reason: "project has dependencies but no node_modules directory",
        };
    }

    // Single-project installs require `pnpm-lock.yaml` on disk to
    // even attempt the fast path. Upstream's single-project branch
    // at <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L396-L401>
    // throws `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` when
    // `wantedLockfileStats` is absent, which the outer `try`
    // converts into `upToDate: false`. Workspace installs skip this
    // gate — pnpm's workspace branch returns `upToDate: true` purely
    // off the manifest-mtime check (its only lockfile probe,
    // `findConflictedLockfileDir`, silently `continue`s on ENOENT at
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L593-L596>).
    if !is_workspace_install && !workspace_root.join(Lockfile::FILE_NAME).exists() {
        return Decision::Skipped { reason: "wanted lockfile missing" };
    }

    // A patch file edited in place keeps the same `patchedDependencies`
    // key→path entry (so `settings_match` can't see the change) but
    // changes the patched output and the patch hash. Upstream catches
    // this in `patchesOrHooksAreModified` before the manifest-modified
    // exit; mirror that ordering so the patch reason wins when both a
    // patch and a manifest are newer than the last validation.
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return Decision::Skipped { reason: "a patch file is newer than the last validation" };
    }

    // The fast-path conclusion. Upstream walks every manifest and
    // returns `upToDate: true` when none have an mtime newer than
    // `workspaceState.lastValidatedTimestamp`. The walk has to
    // succeed (read errors mean we can't *prove* freshness, so fall
    // through), and any newer mtime invalidates.
    if !manifests_unchanged_since(state.last_validated_timestamp, project_manifests) {
        return Decision::Skipped { reason: "a manifest is newer than the last validation" };
    }

    Decision::UpToDate
}

/// Compare today's settings against what the previous install
/// recorded.
///
/// Only the fields pacquet populates via [`current_settings`]
/// participate in the comparison; the rest are listed at the end of
/// this function with the reason each is safe to skip.
///
/// pnpm's [`checkDepsStatus`](https://github.com/pnpm/pnpm/blob/20f9362161/deps/status/src/checkDepsStatus.ts#L138)
/// iterates the full `WORKSPACE_STATE_SETTING_KEYS` list, reading a key
/// absent from the recorded state as `undefined`. So the reverse
/// scenario (pacquet wrote the state, pnpm reads it next) stays on the
/// fast path only for keys whose pnpm-resolved value is also
/// `undefined`. Every key pnpm resolves to a concrete default —
/// `excludeLinksFromLockfile` (`false`), `minimumReleaseAge` (`1440`),
/// `minimumReleaseAgeIgnoreMissingTime` (`true`) — must therefore be
/// written by [`current_settings`] and compared here, or pnpm would
/// report drift and re-run a (no-op) install on every command after a
/// pacquet install. `enableGlobalVirtualStore` is `undefined` by
/// default (concrete only under `--global`/CI), so pacquet's omit-when-
/// off encoding already matches. The `allowBuilds` coercion mirrors
/// pnpm's [`opts.allowBuilds ?? {}`](https://github.com/pnpm/pnpm/blob/20f9362161/deps/status/src/checkDepsStatus.ts#L143)
/// on the read side and pnpm's tolerance of an absent `allowBuilds` key
/// in the recorded state on the write side.
fn settings_match(
    state: &WorkspaceState,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> bool {
    let current = current_settings(config, node_linker, included);
    let recorded = &state.settings;
    let live = &current;
    allow_builds_match(recorded.allow_builds.as_ref(), live.allow_builds.as_ref())
        && recorded.auto_install_peers == live.auto_install_peers
        && recorded.dedupe_direct_deps == live.dedupe_direct_deps
        && recorded.dedupe_injected_deps == live.dedupe_injected_deps
        && recorded.dedupe_peer_dependents == live.dedupe_peer_dependents
        && recorded.dedupe_peers == live.dedupe_peers
        && recorded.dev == live.dev
        && enable_global_virtual_store_match(
            recorded.enable_global_virtual_store,
            live.enable_global_virtual_store,
        )
        && recorded.exclude_links_from_lockfile == live.exclude_links_from_lockfile
        && recorded.hoist_pattern == live.hoist_pattern
        && recorded.hoist_workspace_packages == live.hoist_workspace_packages
        && recorded.ignored_optional_dependencies == live.ignored_optional_dependencies
        && recorded.inject_workspace_packages == live.inject_workspace_packages
        && recorded.link_workspace_packages == live.link_workspace_packages
        && recorded.minimum_release_age == live.minimum_release_age
        && recorded.minimum_release_age_ignore_missing_time
            == live.minimum_release_age_ignore_missing_time
        && recorded.node_linker == live.node_linker
        && recorded.optional == live.optional
        && recorded.overrides == live.overrides
        && package_extensions_match(
            recorded.package_extensions.as_ref(),
            live.package_extensions.as_ref(),
        )
        && recorded.patched_dependencies == live.patched_dependencies
        && recorded.peers_suffix_max_length == live.peers_suffix_max_length
        && recorded.prefer_workspace_packages == live.prefer_workspace_packages
        && recorded.production == live.production
        && recorded.public_hoist_pattern == live.public_hoist_pattern
    // Deliberately *not* compared. pnpm leaves the first group
    // `undefined` by default, so omitting them here still matches pnpm's
    // all-key freshness check (`undefined == undefined`):
    //   catalogs                    (pnpm always ignores; see
    //                                ignoredSettings.add('catalogs'))
    //   minimumReleaseAgeStrict     (pnpm sets it only when the user
    //                                explicitly sets minimumReleaseAge)
    //   minimumReleaseAgeExclude
    //   trustPolicy*                (all `undefined` until configured)
    //   workspacePackagePatterns    (concrete for a multi-package
    //                                workspace, but lives in the
    //                                workspace manifest, not `Config`;
    //                                threading it into `current_settings`
    //                                is a separate follow-up. pacquet
    //                                detects project-set changes via
    //                                `project_structure_matches`).
}

/// `enableGlobalVirtualStore` has no `?? default` coercion on pnpm's
/// read side, but its `undefined` default and an explicit `false` both
/// mean "global virtual store off". pnpm omits the key for the former
/// and records `false` only when CI forces it; pacquet omits both.
/// Normalize the absent and `false` forms before comparing so a
/// pnpm-written file (omitted or `false`) matches a pacquet install
/// with the store off, while a real `true`/`false` flip still trips.
fn enable_global_virtual_store_match(
    state_value: Option<bool>,
    current_value: Option<bool>,
) -> bool {
    state_value.unwrap_or(false) == current_value.unwrap_or(false)
}

/// Pnpm writes `Some({})` for an empty `allowBuilds`; pacquet writes
/// `None` for the same effective value. Treat them as equivalent so
/// cross-package-manager state files don't trip the comparison.
fn allow_builds_match(
    state_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
    current_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> bool {
    match (state_value, current_value) {
        (None, None) => true,
        (Some(map), None) | (None, Some(map)) => map.is_empty(),
        (Some(state_map), Some(current_map)) => state_map == current_map,
    }
}

/// `packageExtensions` are compared as opaque `serde_json::Value`
/// trees so the workspace-state file written by either implementation
/// round-trips through the other. Empty maps are equivalent to absent
/// — pacquet's [`pacquet_config::WorkspaceSettings::apply_to`] already collapses
/// `packageExtensions: {}` to `None`, but pnpm may write `Some({})`
/// directly, and the workspace-state file is shared across the two.
fn package_extensions_match(
    state_value: Option<&serde_json::Value>,
    current_value: Option<&serde_json::Value>,
) -> bool {
    fn is_empty(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.is_empty(),
            serde_json::Value::Null => true,
            _ => false,
        }
    }
    match (state_value, current_value) {
        (None, None) => true,
        (Some(value), None) | (None, Some(value)) => is_empty(value),
        (Some(state_value), Some(current_value)) => state_value == current_value,
    }
}

/// Build the [`WorkspaceStateSettings`] that today's install would
/// write. Shared with `install::build_workspace_state` so the
/// freshness check sees the same byte shape the writer produced —
/// when one side grows a field, the other automatically does too.
pub(crate) fn current_settings(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> WorkspaceStateSettings {
    let allow_builds = (!config.allow_builds.is_empty()).then(|| {
        config.allow_builds.iter().map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v))).collect()
    });
    WorkspaceStateSettings {
        allow_builds,
        auto_install_peers: Some(config.auto_install_peers),
        dedupe_direct_deps: Some(config.dedupe_direct_deps),
        dedupe_injected_deps: Some(config.dedupe_injected_deps),
        dedupe_peer_dependents: Some(config.dedupe_peer_dependents),
        dedupe_peers: Some(config.dedupe_peers),
        dev: Some(included.dev_dependencies),
        // Mirror pnpm's writer, which omits the key for its `undefined`
        // default and records a concrete value only when forced. pacquet
        // has no `--global` flow, so the only "on" value it ever writes
        // is `true`; an off store maps back to the omitted `None`.
        enable_global_virtual_store: config.enable_global_virtual_store.then_some(true),
        exclude_links_from_lockfile: Some(config.exclude_links_from_lockfile),
        hoist_pattern: config.hoist_pattern.clone(),
        hoist_workspace_packages: Some(config.hoist_workspace_packages),
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        inject_workspace_packages: Some(config.inject_workspace_packages),
        link_workspace_packages: Some(link_workspace_packages_to_json(
            config.link_workspace_packages,
        )),
        minimum_release_age: config.minimum_release_age,
        minimum_release_age_ignore_missing_time: Some(
            config.minimum_release_age_ignore_missing_time,
        ),
        node_linker: Some(map_node_linker(node_linker)),
        optional: Some(included.optional_dependencies),
        overrides: config
            .overrides
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        package_extensions: config
            .package_extensions
            .as_ref()
            .and_then(|map| serde_json::to_value(map).ok()),
        patched_dependencies: config.patched_dependencies.clone(),
        peers_suffix_max_length: Some(
            u32::try_from(config.peers_suffix_max_length).unwrap_or(u32::MAX),
        ),
        prefer_workspace_packages: Some(config.prefer_workspace_packages),
        production: Some(included.dependencies),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        ..Default::default()
    }
}

fn link_workspace_packages_to_json(value: LinkWorkspacePackages) -> serde_json::Value {
    match value {
        LinkWorkspacePackages::Off => serde_json::Value::Bool(false),
        LinkWorkspacePackages::DirectOnly => serde_json::Value::Bool(true),
        LinkWorkspacePackages::Deep => serde_json::Value::String("deep".to_string()),
    }
}

fn map_node_linker(linker: NodeLinker) -> WorkspaceStateNodeLinker {
    match linker {
        NodeLinker::Isolated => WorkspaceStateNodeLinker::Isolated,
        NodeLinker::Hoisted => WorkspaceStateNodeLinker::Hoisted,
        NodeLinker::Pnp => WorkspaceStateNodeLinker::Pnp,
    }
}

/// Project count + per-project (key, name, version) match between the
/// cached state and today's walk. The key is the project's root dir;
/// `build_workspace_state` and pnpm both use it as the map key, so a
/// renamed / removed / added project trips the check immediately.
fn project_structure_matches(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    if state.projects.len() != project_manifests.len() {
        return false;
    }
    project_manifests.iter().all(|(root_dir, manifest)| {
        let key = root_dir.to_string_lossy().into_owned();
        let Some(entry) = state.projects.get(&key) else {
            return false;
        };
        entry.name.as_deref() == manifest_string_field(manifest, "name").as_deref()
            && entry.version.as_deref().unwrap_or("0.0.0")
                == manifest_string_field(manifest, "version").as_deref().unwrap_or("0.0.0")
    })
}

fn modules_dirs_present(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    project_manifests.iter().all(|(root_dir, manifest)| {
        if !manifest_has_runtime_deps(manifest) {
            return true;
        }
        // The root importer uses `config.modules_dir`; siblings use
        // their own `<root>/node_modules`. Matches the isolated-linker
        // default — `config.modules_dir` is `<workspace_root>/node_modules`
        // unless the user overrode it explicitly.
        let modules_dir = if *root_dir == workspace_dir_of(config, root_dir) {
            config.modules_dir.clone()
        } else {
            root_dir.join("node_modules")
        };
        modules_dir.exists()
    })
}

/// Recover the workspace root from `config.modules_dir`. The root
/// importer's `root_dir` equals `config.modules_dir.parent()` because
/// `config.modules_dir` is `<workspace_root>/node_modules`. Used by
/// [`modules_dirs_present`] to tell root from sibling — a brittle
/// shape but it matches how the install path itself derives
/// `config.modules_dir`.
fn workspace_dir_of(config: &Config, fallback: &Path) -> PathBuf {
    config.modules_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| fallback.to_path_buf())
}

fn manifest_has_runtime_deps(manifest: &PackageManifest) -> bool {
    let value = manifest.value();
    [value.get("dependencies"), value.get("devDependencies"), value.get("optionalDependencies")]
        .into_iter()
        .flatten()
        .any(|deps| deps.as_object().is_some_and(|map| !map.is_empty()))
}

fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest.value().get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

/// Stat every project's `package.json` and check that no mtime is
/// newer than `cutoff_ms`. Any stat failure is treated as "can't
/// prove freshness, fall through" — matching pnpm's
/// [`statManifestFile`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/statManifestFile.ts)
/// behavior on missing files (it throws, which `checkDepsStatus`
/// catches via the outer `try`).
/// Whether any configured patch file's mtime is newer than the last
/// validation. Mirrors the patch branch of upstream's
/// [`patchesOrHooksAreModified`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L604-L613):
/// `allPatchStats.some(patch => patch && patch.mtime > lastValidatedTimestamp)`.
/// A patch that can't be stat'd is treated as not-modified — pnpm's
/// `safeStat` returns null and the `patch &&` guard drops it, leaving a
/// genuinely missing patch to surface on the full install path. Patch
/// paths are resolved against `workspace_root` (the `pnpm-workspace.yaml`
/// dir, where `patchedDependencies` is declared), matching how
/// [`Config::patched_dependency_hashes`] resolves them.
fn patches_modified_since(workspace_root: &Path, config: &Config, cutoff_ms: i64) -> bool {
    let Some(patches) = config.patched_dependencies.as_ref() else {
        return false;
    };
    patches.values().any(|rel_or_abs| {
        let candidate = Path::new(rel_or_abs);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace_root.join(candidate)
        };
        let Ok(modified) = fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            return false;
        };
        let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        let modified_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        modified_ms > cutoff_ms
    })
}

fn manifests_unchanged_since(
    cutoff_ms: i64,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    project_manifests.iter().all(|(_, manifest)| {
        let Ok(metadata) = fs::metadata(manifest.path()) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        // Convert wall-clock to ms-since-epoch the same way
        // `pacquet_workspace_state::now_millis` does on the write
        // side, so a `> cutoff` comparison is apples-to-apples.
        let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        let modified_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        modified_ms <= cutoff_ms
    })
}

#[cfg(test)]
mod tests;
