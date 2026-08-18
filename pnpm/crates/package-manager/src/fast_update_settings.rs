use crate::fast_update_compose::Drift;
use pnpm_config::Config;
use pnpm_lockfile::{ImporterDepVersion, Lockfile, LockfileSettings, ProjectSnapshot};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// The `settings` block an install under `config` records, matching
/// what [`fn@crate::dependencies_graph_to_lockfile`] writes after a full
/// resolution.
pub(crate) fn lockfile_settings_from_config(config: &Config) -> LockfileSettings {
    LockfileSettings {
        auto_install_peers: config.auto_install_peers,
        dedupe_peers: config.dedupe_peers.then_some(true),
        exclude_links_from_lockfile: config.exclude_links_from_lockfile,
        inject_workspace_packages: config.inject_workspace_packages,
        peers_suffix_max_length: (config.peers_suffix_max_length
            != pnpm_config::default_peers_suffix_max_length())
        .then_some(config.peers_suffix_max_length),
    }
}

/// A lockfile setting whose recorded value no longer matches the
/// current configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangedSetting {
    AutoInstallPeers,
    DedupePeers,
    ExcludeLinksFromLockfile,
    PeersSuffixMaxLength,
    InjectWorkspacePackages,
}

/// Whether the lockfile's recorded `settings` block drifted from the
/// current configuration. Whether the drift is absorbable is
/// [`apply_settings_update`]'s question: it depends on the graph, which
/// the handlers running before it may have rewritten.
pub(crate) fn detect_settings_drift(lockfile: &Lockfile, settings: &LockfileSettings) -> Drift<()> {
    if changed_settings(lockfile.settings.as_ref(), settings).is_empty() {
        Drift::Clean
    } else {
        Drift::Absorb(())
    }
}

/// Record the current configuration's `settings` block on `candidate`,
/// restricted to the drift the candidate itself proves cannot affect
/// its graph: the peer settings when nothing declares a peer
/// dependency, and the link and injection settings when no project
/// depends on a directory or on another workspace project. Checked
/// against the candidate the earlier handlers already rewrote — a peer
/// dependent they removed no longer blocks the setting. `false` leaves
/// the caller on the full-resolution path.
pub(crate) fn apply_settings_update(
    candidate: &mut Lockfile,
    settings: &LockfileSettings,
    manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    let changed = changed_settings(candidate.settings.as_ref(), settings);
    let workspace_package_names = workspace_package_names(manifests);
    if !changed.iter().all(|setting| {
        setting_cannot_affect_lockfile(*setting, candidate, manifests, &workspace_package_names)
    }) {
        return false;
    }
    candidate.settings = Some(settings.clone());
    true
}

fn changed_settings(
    recorded: Option<&LockfileSettings>,
    settings: &LockfileSettings,
) -> Vec<ChangedSetting> {
    let mut changed = Vec::new();
    if pnpm_lockfile::auto_install_peers_changed(recorded, settings.auto_install_peers) {
        changed.push(ChangedSetting::AutoInstallPeers);
    }
    if pnpm_lockfile::recorded_dedupe_peers(recorded)
        != pnpm_lockfile::recorded_dedupe_peers(Some(settings))
    {
        changed.push(ChangedSetting::DedupePeers);
    }
    if pnpm_lockfile::exclude_links_from_lockfile_changed(
        recorded,
        settings.exclude_links_from_lockfile,
    ) {
        changed.push(ChangedSetting::ExcludeLinksFromLockfile);
    }
    if pnpm_lockfile::recorded_peers_suffix_max_length(recorded)
        != pnpm_lockfile::recorded_peers_suffix_max_length(Some(settings))
    {
        changed.push(ChangedSetting::PeersSuffixMaxLength);
    }
    if pnpm_lockfile::recorded_inject_workspace_packages(recorded)
        != pnpm_lockfile::recorded_inject_workspace_packages(Some(settings))
    {
        changed.push(ChangedSetting::InjectWorkspacePackages);
    }
    changed
}

fn setting_cannot_affect_lockfile(
    setting: ChangedSetting,
    lockfile: &Lockfile,
    manifests: &[(PathBuf, &PackageManifest)],
    workspace_package_names: &HashSet<String>,
) -> bool {
    match setting {
        ChangedSetting::AutoInstallPeers
        | ChangedSetting::DedupePeers
        | ChangedSetting::PeersSuffixMaxLength => has_no_peer_dependencies(lockfile, manifests),
        ChangedSetting::ExcludeLinksFromLockfile => {
            has_no_linked_dependencies(lockfile, manifests, workspace_package_names)
        }
        ChangedSetting::InjectWorkspacePackages => {
            has_no_injectable_dependencies(lockfile, manifests, workspace_package_names)
        }
    }
}

/// All three peer settings only change how peer dependencies are
/// resolved, deduplicated, and hashed into depPath suffixes. None of
/// them has anything to act on when no package or project declares a
/// peer dependency and no depPath carries a peers suffix.
fn has_no_peer_dependencies(
    lockfile: &Lockfile,
    manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    let peerless_packages = lockfile.packages.iter().flatten().all(|(key, metadata)| {
        key.suffix.peer().is_empty()
            && metadata.peer_dependencies.as_ref().is_none_or(HashMap::is_empty)
            && metadata.peer_dependencies_meta.as_ref().is_none_or(HashMap::is_empty)
    });
    let peerless_snapshots = lockfile.snapshots.iter().flatten().all(|(key, snapshot)| {
        key.suffix.peer().is_empty()
            && snapshot.transitive_peer_dependencies.as_ref().is_none_or(Vec::is_empty)
    });
    peerless_packages
        && peerless_snapshots
        && manifests
            .iter()
            .all(|(_, manifest)| manifest.dependencies([DependencyGroup::Peer]).next().is_none())
}

/// `excludeLinksFromLockfile` decides whether a dependency that
/// resolves to a directory is recorded in the lockfile. Dependencies
/// declared with the `workspace:` protocol are recorded either way, so
/// only the other directory dependencies matter — including plain
/// ranges that `linkWorkspacePackages` turns into links to a workspace
/// project.
fn has_no_linked_dependencies(
    lockfile: &Lockfile,
    manifests: &[(PathBuf, &PackageManifest)],
    workspace_package_names: &HashSet<String>,
) -> bool {
    !lockfile.importers.values().any(has_directory_reference)
        && manifests.iter().all(|(_, manifest)| {
            manifest.dependencies(DEPENDENCY_GROUPS).all(|(alias, bare_specifier)| {
                bare_specifier.starts_with("workspace:")
                    || !is_directory_dependency(alias, bare_specifier, workspace_package_names)
            })
        })
}

/// `injectWorkspacePackages` replaces the symlinks to workspace
/// projects with hard-linked copies, which the lockfile records as
/// directory dependencies of the importer. An install with no
/// dependency on any workspace project has nothing to inject.
fn has_no_injectable_dependencies(
    lockfile: &Lockfile,
    manifests: &[(PathBuf, &PackageManifest)],
    workspace_package_names: &HashSet<String>,
) -> bool {
    !lockfile.importers.values().any(has_directory_reference)
        && manifests.iter().all(|(_, manifest)| {
            !declares_injected_dependency(manifest)
                && manifest.dependencies(DEPENDENCY_GROUPS).all(|(alias, bare_specifier)| {
                    !is_directory_dependency(alias, bare_specifier, workspace_package_names)
                })
        })
}

/// Whether `alias` resolves to a directory rather than to a registry
/// version: the protocols that name one outright, and a plain range on
/// a workspace project's name, which `linkWorkspacePackages` turns into
/// a link.
pub(crate) fn is_directory_dependency(
    alias: &str,
    bare_specifier: &str,
    workspace_package_names: &HashSet<String>,
) -> bool {
    bare_specifier.starts_with("workspace:")
        || bare_specifier.starts_with("link:")
        || bare_specifier.starts_with("file:")
        || workspace_package_names.contains(alias)
}

fn has_directory_reference(importer: &ProjectSnapshot) -> bool {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .any(|(_, dependency)| {
            matches!(dependency.version, ImporterDepVersion::Link(_) | ImporterDepVersion::File(_))
        })
}

fn declares_injected_dependency(manifest: &PackageManifest) -> bool {
    manifest.value().get("dependenciesMeta").and_then(serde_json::Value::as_object).is_some_and(
        |entries| {
            entries.values().any(|meta| {
                meta.get("injected").and_then(serde_json::Value::as_bool).unwrap_or(false)
            })
        },
    )
}

/// The names every workspace project publishes under, which a plain
/// range on one of them resolves to a directory through.
pub(crate) fn workspace_package_names(
    manifests: &[(PathBuf, &PackageManifest)],
) -> HashSet<String> {
    manifests
        .iter()
        .filter_map(|(_, manifest)| {
            manifest
                .value()
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

const DEPENDENCY_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional];

#[cfg(test)]
mod tests;
