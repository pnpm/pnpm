use std::collections::{
    BTreeSet,
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageKey,
    PackageMetadata,
    PkgName,
    ProjectSnapshot,
    ResolvedDependencySpec,
    SnapshotEntry,
};
use sha2::{
    Digest,
    Sha256,
};

use super::{
    classification::normalize_mirror_prefix,
    metadata_for,
    paths::normalize_posix_rel_path,
};

/// Workspace and mirror context governing offline `blockExoticSubdeps` checks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExoticSubdepsScope {
    /// Real workspace importer IDs (`"."`, `"packages/a"`). `None` only in
    /// standalone unit tests that construct an unscoped policy.
    pub known_importer_ids: Option<BTreeSet<String>>,
    /// Verified workspace-project directories relative to the lockfile dir
    /// (discovered project roots, verified `publishConfig.directory` paths,
    /// declared `workspace:` directory targets, and `pnpm deploy` siblings).
    pub extra_workspace_dirs: BTreeSet<String>,
    /// Allowed custom Node.js mirror prefixes (each normalized with a trailing `/`).
    pub node_mirrors: BTreeSet<String>,
    /// Allowed custom Bun mirror prefixes (each normalized with a trailing `/`).
    pub bun_mirrors: BTreeSet<String>,
}

impl ExoticSubdepsScope {
    #[must_use]
    pub fn new<'ids, 'dirs, 'node, 'bun>(
        known_importer_ids: Option<impl IntoIterator<Item = &'ids str>>,
        extra_workspace_dirs: impl IntoIterator<Item = &'dirs str>,
        node_mirrors: impl IntoIterator<Item = &'node str>,
        bun_mirrors: impl IntoIterator<Item = &'bun str>,
    ) -> Self {
        Self {
            known_importer_ids: known_importer_ids.map(|ids| {
                ids.into_iter()
                    .map(normalize_posix_rel_path)
                    .collect()
            }),
            extra_workspace_dirs: extra_workspace_dirs
                .into_iter()
                .map(normalize_posix_rel_path)
                .collect(),
            node_mirrors: node_mirrors
                .into_iter()
                .filter_map(normalize_mirror_prefix)
                .collect(),
            bun_mirrors: bun_mirrors
                .into_iter()
                .filter_map(normalize_mirror_prefix)
                .collect(),
        }
    }

    pub(crate) fn cache_digest(&self) -> String {
        let mut hasher = Sha256::new();
        match &self.known_importer_ids {
            None => hasher.update(b"importers:*\0"),
            Some(ids) => hash_string_set(&mut hasher, b"importers:scoped\0", ids),
        }
        hash_string_set(&mut hasher, b"extra_dirs\0", &self.extra_workspace_dirs);
        hash_string_set(&mut hasher, b"node_mirrors\0", &self.node_mirrors);
        hash_string_set(&mut hasher, b"bun_mirrors\0", &self.bun_mirrors);
        format!("{:x}", hasher.finalize())
    }
}

fn hash_string_set(hasher: &mut Sha256, tag: &[u8], set: &BTreeSet<String>) {
    hasher.update(tag);
    hasher.update((set.len() as u64).to_be_bytes());
    for entry in set {
        hasher.update((entry.len() as u64).to_be_bytes());
        hasher.update(entry.as_bytes());
    }
}

fn is_canonical_importer_id(id: &str) -> bool {
    !id.is_empty() && !id.contains('\\') && normalize_posix_rel_path(id) == id
}

fn trusted_importers<'a>(
    lockfile: &'a Lockfile,
    scope: &'a ExoticSubdepsScope,
) -> impl Iterator<Item = (&'a String, &'a ProjectSnapshot)> {
    lockfile.importers
        .iter()
        .filter(|(id, _)| {
            is_canonical_importer_id(id)
                && scope.known_importer_ids
                    .as_ref()
                    .is_none_or(|known| known.contains(id.as_str()))
        })
}

pub(super) fn collect_workspace_directories(
    lockfile: &Lockfile,
    scope: &ExoticSubdepsScope,
) -> HashSet<String> {
    let mut dirs: HashSet<String> = scope.extra_workspace_dirs
        .iter()
        .map(|dir| normalize_posix_rel_path(dir))
        .collect();
    for (importer_id, _) in trusted_importers(lockfile, scope) {
        dirs.insert(normalize_posix_rel_path(importer_id));
    }
    dirs
}

fn importer_dependency_specs(
    importer: &ProjectSnapshot,
) -> impl Iterator<Item = (&PkgName, &ResolvedDependencySpec)> {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
}

pub(super) fn is_workspace_snapshot(
    key: &PackageKey,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    workspace_dirs: &HashSet<String>,
) -> bool {
    let Some(PackageMetadata {
        resolution: LockfileResolution::Directory(dir_res),
        ..
    }) = metadata_for(packages, key)
    else {
        return false;
    };
    !dir_res.directory.contains('\\')
        && workspace_dirs.contains(&normalize_posix_rel_path(&dir_res.directory))
}

pub(super) fn collect_allowed_peer_providers(
    lockfile: &Lockfile,
    scope: &ExoticSubdepsScope,
    workspace_dirs: &HashSet<String>,
) -> HashSet<PackageKey> {
    let mut providers = HashSet::new();
    for (_, importer) in trusted_importers(lockfile, scope) {
        add_importer_peer_providers(&mut providers, importer);
    }
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return providers };
    let packages = lockfile.packages.as_ref();
    for (parent_key, snapshot) in snapshots {
        if is_workspace_snapshot(parent_key, packages, workspace_dirs) {
            add_snapshot_peer_providers(&mut providers, snapshot);
        }
    }
    providers
}

fn add_importer_peer_providers(providers: &mut HashSet<PackageKey>, importer: &ProjectSnapshot) {
    for (alias, spec) in importer_dependency_specs(importer) {
        if let Some(key) = spec.version.resolved_key(alias) {
            providers.insert(key);
        }
    }
}

fn add_snapshot_peer_providers(providers: &mut HashSet<PackageKey>, snapshot: &SnapshotEntry) {
    for (alias, dep_ref) in snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
    {
        if let Some(child_key) = dep_ref.resolve(alias) {
            providers.insert(child_key);
        }
    }
}
