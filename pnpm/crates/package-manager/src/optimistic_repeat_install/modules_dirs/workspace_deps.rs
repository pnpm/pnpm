use crate::optimistic_repeat_install::{
    CatalogAnchor, CatalogResolutionResult, Catalogs, OptimisticRepeatInstallCheck,
    WantedDependency, manifest_string_field, resolve_from_catalog,
};
use pnpm_fs::lexical_normalize;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_semver_include_prerelease::IncludePrereleaseRange;
use pnpm_workspace_state::WorkspaceState;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

struct WorkspaceProjectIndex<'a> {
    workspace_dir: PathBuf,
    by_dir: HashMap<PathBuf, &'a PackageManifest>,
    by_name: HashMap<String, Vec<(PathBuf, Option<String>)>>,
    link_workspace_packages: bool,
    only_link_deps: bool,
    catalogs: &'a Catalogs,
}

impl<'a> WorkspaceProjectIndex<'a> {
    fn from_check(check: &OptimisticRepeatInstallCheck<'a>) -> Self {
        let mut by_dir = HashMap::new();
        let mut by_name: HashMap<String, Vec<(PathBuf, Option<String>)>> = HashMap::new();
        for (dir, manifest) in check.project_manifests {
            let norm = lexical_normalize(dir);
            by_dir.insert(norm.clone(), *manifest);
            if let Some(name) = manifest_string_field(manifest, "name") {
                let version = manifest_string_field(manifest, "version");
                by_name
                    .entry(name)
                    .or_default()
                    .push((norm, version));
            }
        }
        // Where `pnpm-workspace.yaml` sits, so a local-path catalog entry is
        // re-anchored from the workspace directory even when `lockfile_dir`
        // moves `check.workspace_root` elsewhere.
        let workspace_dir = check.config.workspace_dir
            .as_deref()
            .or_else(|| {
                check.config.lockfile_dir
                    .is_some()
                    .then(|| check.project_manifests.first().map(|(dir, _)| dir.as_path()))
                    .flatten()
            })
            .unwrap_or(check.workspace_root);
        Self {
            workspace_dir: lexical_normalize(workspace_dir),
            by_dir,
            by_name,
            link_workspace_packages: check.config.link_workspace_packages.enabled_at_depth(0),
            only_link_deps: !check.config.shares_one_lockfile()
                && check.config.inject_workspace_packages,
            catalogs: check.catalogs,
        }
    }

    fn dereference_catalog<'b>(
        &self,
        dir: &Path,
        alias: &str,
        spec: &'b str,
    ) -> Option<Cow<'b, str>> {
        if !spec.starts_with("catalog:") {
            return Some(Cow::Borrowed(spec));
        }
        let CatalogResolutionResult::Found(found) = resolve_from_catalog(
            self.catalogs,
            &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
            CatalogAnchor::Reanchor { workspace_dir: &self.workspace_dir, consumer_dir: Some(dir) },
        ) else {
            return None;
        };
        Some(Cow::Owned(found.resolution.specifier))
    }

    fn dep_targets(&self, dir: &Path, alias: &str, spec: &str) -> Vec<PathBuf> {
        let Some(spec) = self.dereference_catalog(dir, alias, spec) else {
            return Vec::new();
        };
        let spec = spec.as_ref();
        if let Some(rel) = spec.strip_prefix("link:") {
            return self.resolve_dir_target(dir, rel);
        }
        if self.only_link_deps {
            return Vec::new();
        }
        if let Some(rel) = spec.strip_prefix("file:") {
            return self.resolve_dir_target(dir, rel);
        }
        let is_workspace = spec.starts_with("workspace:");
        if !is_workspace && !self.link_workspace_packages {
            return Vec::new();
        }
        if let Some(ws_spec) = spec.strip_prefix("workspace:")
            && is_relative_workspace_path(ws_spec)
        {
            return self.resolve_dir_target(dir, ws_spec);
        }
        let (target_name, range) = crate::install::spec_target_and_range(spec);
        let name = target_name.unwrap_or(alias);
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|(_, version)| {
                workspace_version_matches_range(version.as_deref(), range, is_workspace)
            })
            .map(|(dir, _)| dir.clone())
            .collect()
    }

    fn resolve_dir_target(&self, dir: &Path, rel: &str) -> Vec<PathBuf> {
        let normalized_rel = rel.replace('\\', "/");
        let target = lexical_normalize(&dir.join(normalized_rel));
        self.by_dir
            .contains_key(&target)
            .then_some(target)
            .into_iter()
            .collect()
    }
}

fn is_relative_workspace_path(ws_spec: &str) -> bool {
    matches!(ws_spec, "." | "..")
        || ws_spec.starts_with("./")
        || ws_spec.starts_with("../")
        || ws_spec.starts_with(r".\")
        || ws_spec.starts_with(r"..\")
}

pub(super) fn reached_workspace_project_dirs(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    roots: &[&Path],
) -> HashSet<PathBuf> {
    let index = WorkspaceProjectIndex::from_check(check);
    let groups = [
        (state.settings.production != Some(false)).then_some(DependencyGroup::Prod),
        (state.settings.dev != Some(false)).then_some(DependencyGroup::Dev),
        (state.settings.optional != Some(false)).then_some(DependencyGroup::Optional),
        (check.config.auto_install_peers && state.settings.auto_install_peers != Some(false))
            .then_some(DependencyGroup::Peer),
    ];
    let mut reached = HashSet::new();
    let mut stack: Vec<PathBuf> = roots
        .iter()
        .map(|dir| lexical_normalize(dir))
        .collect();
    while let Some(dir) = stack.pop() {
        let Some(&manifest) = index.by_dir
            .get(&dir)
            .filter(|_| reached.insert(dir.clone()))
        else {
            continue;
        };
        for (alias, spec) in manifest.dependencies(groups.into_iter().flatten()) {
            stack.extend(index.dep_targets(&dir, alias, spec));
        }
    }
    reached
}

fn workspace_version_matches_range(version: Option<&str>, range: &str, is_workspace: bool) -> bool {
    if matches!(range, "" | "*" | "^" | "~") {
        return true;
    }
    if range.parse::<node_semver::Range>().is_err() {
        return is_workspace;
    }
    let Some(version) = version else {
        return is_workspace;
    };
    let parsed_range = IncludePrereleaseRange::parse(range);
    version == range
        || version
            .parse::<node_semver::Version>()
            .is_ok_and(|parsed| parsed_range.satisfies(&parsed))
}
