use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        VecDeque,
    },
    fs,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use pnpm_config::{
    Config,
    Tool,
};
use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::{
    ExoticSubdepsPolicy,
    ExoticSubdepsScope,
    ReplacedCandidate,
    ReplacedEntries,
    VerifyLockfileResolutionsOptions,
    verify_lockfile_resolutions,
};
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use pnpm_resolving_resolver_base::ResolutionVerifier;

/// Builds the [`ExoticSubdepsPolicy`] for `config`, scoping importer and runtime-mirror
/// exemptions to the workspace's real importers and configured runtime mirrors.
#[must_use]
pub fn config_exotic_subdeps_policy<'ids, 'dirs>(
    config: &Config,
    known_importer_ids: Option<impl IntoIterator<Item = &'ids str>>,
    extra_workspace_dirs: impl IntoIterator<Item = &'dirs str>,
) -> ExoticSubdepsPolicy {
    if !config.block_exotic_subdeps {
        return ExoticSubdepsPolicy::Allow;
    }
    let node_channel_mirrors = config.tool_channel_mirrors(Tool::Node);
    let bun_channel_mirrors = config.tool_channel_mirrors(Tool::Bun);
    let node_mirrors = config.node_download_mirrors
        .values()
        .map(String::as_str)
        .chain(config.tool_mirror(Tool::Node))
        .chain(node_channel_mirrors.values().map(String::as_str));
    let bun_mirrors = config
        .tool_mirror(Tool::Bun)
        .into_iter()
        .chain(bun_channel_mirrors.values().map(String::as_str));
    ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        known_importer_ids,
        extra_workspace_dirs,
        node_mirrors,
        bun_mirrors,
    ))
}

/// Collects independently verified workspace-project directories (project roots,
/// confined `publishConfig.directory` targets, and recursive path-based `workspace:`
/// targets) relative to `lockfile_dir`.
#[must_use]
pub fn verified_project_workspace_dirs<'m>(
    lockfile_dir: &Path,
    manifests: impl IntoIterator<Item = (&'m Path, &'m serde_json::Value)>,
) -> Vec<String> {
    let mut collector = WorkspaceDirCollector {
        lockfile_dir,
        dirs: BTreeSet::new(),
        manifest_cache: BTreeMap::new(),
        visited_targets: BTreeSet::new(),
        queue: VecDeque::new(),
    };
    for (project_root, manifest_json) in manifests {
        collector.add_importer_manifest(project_root, manifest_json);
    }
    collector.dirs.into_iter().collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManifestDepScope {
    Importer,
    Dependency,
}

struct WorkspaceDirCollector<'dir> {
    lockfile_dir: &'dir Path,
    dirs: BTreeSet<String>,
    manifest_cache: BTreeMap<PathBuf, Option<serde_json::Value>>,
    visited_targets: BTreeSet<PathBuf>,
    queue: VecDeque<(PathBuf, serde_json::Value)>,
}

impl WorkspaceDirCollector<'_> {
    fn add_importer_manifest(&mut self, project_root: &Path, manifest_json: &serde_json::Value) {
        let normalized_root = pnpm_fs::lexical_normalize(project_root);
        self.record_root_and_publish_dir(&normalized_root, manifest_json);
        self.enqueue_targets(&normalized_root, manifest_json, ManifestDepScope::Importer);
        while let Some((current_dir, current_manifest)) = self.queue.pop_front() {
            self.enqueue_targets(&current_dir, &current_manifest, ManifestDepScope::Dependency);
        }
    }

    fn enqueue_targets(
        &mut self,
        parent_root: &Path,
        manifest_json: &serde_json::Value,
        scope: ManifestDepScope,
    ) {
        for raw_spec in manifest_dependency_specifiers(manifest_json, scope) {
            if let Some((target_dir, target_manifest)) =
                self.resolve_workspace_path_spec(parent_root, raw_spec)
                && self.visited_targets.insert(target_dir.clone())
            {
                self.record_root_and_publish_dir(&target_dir, &target_manifest);
                self.queue.push_back((target_dir, target_manifest));
            }
        }
    }

    fn record_root_and_publish_dir(&mut self, root_dir: &Path, manifest_json: &serde_json::Value) {
        self.dirs.insert(pnpm_workspace::importer_id_from_root_dir(self.lockfile_dir, root_dir));
        let package_dir = pnpm_fs::lexical_normalize(
            &pnpm_resolving_npm_resolver::resolve_workspace_package_dir(root_dir, manifest_json),
        );
        if package_dir != root_dir && is_verified_publish_dir(root_dir, &package_dir) {
            self.dirs.insert(pnpm_workspace::importer_id_from_root_dir(
                self.lockfile_dir,
                &package_dir,
            ));
        }
    }

    fn resolve_workspace_path_spec(
        &mut self,
        project_root: &Path,
        raw_spec: &str,
    ) -> Option<(PathBuf, serde_json::Value)> {
        let trimmed = raw_spec.trim();
        if !pnpm_workspace_spec::is_workspace_path_specifier(trimmed) {
            return None;
        }
        let candidate = pnpm_resolving_local_resolver::local_spec_path(trimmed, project_root);
        if !is_real_workspace_target(project_root, &candidate) {
            return None;
        }
        let cached = self.manifest_cache
            .entry(candidate.clone())
            .or_insert_with(|| safe_read_project_manifest_from_dir(&candidate).ok().flatten())
            .clone()?;
        Some((candidate, cached))
    }
}

fn manifest_dependency_specifiers(
    manifest_json: &serde_json::Value,
    scope: ManifestDepScope,
) -> impl Iterator<Item = &str> {
    let fields: &[&str] = match scope {
        ManifestDepScope::Importer => &["dependencies", "devDependencies", "optionalDependencies"],
        ManifestDepScope::Dependency => &["dependencies", "optionalDependencies"],
    };
    fields
        .iter()
        .filter_map(|field| manifest_json.get(*field)?.as_object())
        .flat_map(serde_json::Map::values)
        .filter_map(serde_json::Value::as_str)
}

fn is_real_workspace_target(project_root: &Path, candidate: &Path) -> bool {
    let normalized_root = pnpm_fs::lexical_normalize(project_root);
    let Some(common_base) = common_lexical_ancestor(&normalized_root, candidate) else {
        return false;
    };
    let Some(relative) = confined_subpath(&common_base, candidate, SubpathScope::AllowBaseDir)
    else {
        return false;
    };
    let is_real_dir = fs::symlink_metadata(candidate)
        .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_dir());
    if !is_real_dir {
        return false;
    }
    match (fs::canonicalize(&common_base), fs::canonicalize(candidate)) {
        (Ok(real_base), Ok(real_candidate)) => real_candidate == real_base.join(relative),
        _ => false,
    }
}

fn common_lexical_ancestor(left: &Path, right: &Path) -> Option<PathBuf> {
    let mut ancestor = PathBuf::new();
    for (left_comp, right_comp) in left.components().zip(right.components()) {
        if left_comp != right_comp {
            break;
        }
        ancestor.push(left_comp);
    }
    (!ancestor.as_os_str().is_empty()).then_some(ancestor)
}

/// Whether a manifest-declared `publish_dir` stays confined within `root_dir`
/// without traversing symlinks or `node_modules`, even before `prepare` creates
/// the final output directory on a clean checkout.
#[must_use]
pub fn is_verified_publish_dir(root_dir: &Path, publish_dir: &Path) -> bool {
    let Some(relative) = confined_subpath(root_dir, publish_dir, SubpathScope::StrictDescendant)
    else {
        return false;
    };
    let Ok(real_root_dir) = fs::canonicalize(root_dir) else {
        return false;
    };
    let mut current = root_dir.to_path_buf();
    let mut expected_real = real_root_dir;
    for component in relative.components() {
        current.push(component);
        expected_real.push(component);
        match fs::symlink_metadata(&current) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return true,
            Ok(meta)
                if !meta.file_type().is_symlink()
                    && meta.is_dir()
                    && fs::canonicalize(&current).is_ok_and(|real| real == expected_real) => {}
            _ => return false,
        }
    }
    true
}

#[derive(Clone, Copy)]
enum SubpathScope {
    AllowBaseDir,
    StrictDescendant,
}

fn confined_subpath<'p>(
    base_dir: &Path,
    target_dir: &'p Path,
    scope: SubpathScope,
) -> Option<&'p Path> {
    let relative = target_dir.strip_prefix(base_dir).ok()?;
    let scope_allows = match scope {
        SubpathScope::AllowBaseDir => true,
        SubpathScope::StrictDescendant => !relative.as_os_str().is_empty(),
    };
    let valid = scope_allows
        && relative
            .components()
            .all(|component| {
                matches!(
                    component,
                    std::path::Component::Normal(seg) if !seg.eq_ignore_ascii_case("node_modules"),
                )
            });
    valid.then_some(relative)
}

/// Whether `publish_dir` is a real directory inside `root_dir`, reached through
/// no symlink escape and no `node_modules` segment.
#[must_use]
pub fn is_own_publish_dir(root_dir: &Path, publish_dir: &Path) -> bool {
    let Some(relative) = confined_subpath(root_dir, publish_dir, SubpathScope::StrictDescendant)
    else {
        return false;
    };
    match (fs::canonicalize(root_dir), fs::canonicalize(publish_dir)) {
        (Ok(real_root_dir), Ok(real_publish_dir)) => {
            real_publish_dir == real_root_dir.join(relative)
        }
        _ => false,
    }
}

/// The pre-resolve lockfile-verification fan-out, spawned so its
/// registry round trips overlap the fresh path's resolve and
/// materialization instead of serializing in front of them, the same
/// concurrent-gate contract the frozen path's `select!` provides. The
/// verdict still gates everything sensitive:
/// [`InstallWithFreshLockfile`](crate::install_with_fresh_lockfile::InstallWithFreshLockfile)
/// awaits the gate before bin linking, dependency builds, and the lockfile save.
///
/// Aborts the fan-out on drop so an install that fails before reaching
/// the gate doesn't leave verification requests running in the host
/// process (the napi embedding outlives a failed install).
pub struct LockfileVerificationGate(
    tokio::task::JoinHandle<Result<(), pnpm_lockfile_verification::VerifyError>>,
);

/// Owned form of [`ReplacedEntries`], for the spawned gate.
pub(crate) type IsReplaced = Arc<dyn Fn(ReplacedCandidate<'_>) -> bool + Send + Sync>;

impl LockfileVerificationGate {
    /// Start the fan-out in the background, or `None` when no verifier
    /// is active (`trustLockfile`).
    #[cfg_attr(
        dylint_lib = "perfectionist",
        expect(
            perfectionist::needless_borrowed_parameters,
            reason = "the cache dir is copied only when a verifier is active, and the caller borrows it from the config"
        )
    )]
    pub(super) fn spawn<Reporter: pnpm_reporter::Reporter + Send + 'static>(
        lockfile: Arc<Lockfile>,
        verifiers: &[Arc<dyn ResolutionVerifier>],
        exotic_subdeps: ExoticSubdepsPolicy,
        lockfile_source: Option<pnpm_lockfile_verification::LockfileCacheSource<'_>>,
        cache_dir: &Path,
        replaced: Option<IsReplaced>,
        repairs_lockfile: bool,
    ) -> Option<Self> {
        if verifiers.is_empty() && !exotic_subdeps.is_enforced() {
            return None;
        }
        let verifiers = verifiers.to_vec();
        let lockfile_path = lockfile_source.map(|source| source.path.to_path_buf());
        let lockfile_stat = lockfile_source.and_then(|source| source.stat.cloned());
        let cache_dir = cache_dir.to_path_buf();
        Some(Self(tokio::spawn(async move {
            verify_lockfile_resolutions::<Reporter>(
                &lockfile,
                &verifiers,
                &VerifyLockfileResolutionsOptions {
                    concurrency: None,
                    lockfile_path: lockfile_path.as_deref(),
                    lockfile_stat,
                    cache_dir: Some(&cache_dir),
                    replaced: replaced.as_deref().map(ReplacedEntries),
                    repairs_lockfile,
                    exotic_subdeps,
                },
            )
            .await
        })))
    }

    /// Block on the verdict.
    pub(crate) async fn wait(mut self) -> Result<(), pnpm_lockfile_verification::VerifyError> {
        (&mut self.0).await.expect(
            "the lockfile verification task is only aborted by dropping the gate unawaited",
        )
    }
}

impl Drop for LockfileVerificationGate {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests;
