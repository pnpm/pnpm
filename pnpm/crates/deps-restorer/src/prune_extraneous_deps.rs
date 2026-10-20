//! Remove extraneous entries (symlinks, directories, or files) from each
//! selected importer's `node_modules` directory during `pnpm prune`.

use crate::{
    hoist::HoistedDependencies,
    prune_direct_deps::{
        PruneDirectDepsError, confined_modules_dir, is_real_dir, remove_extraneous_direct_dep,
    },
    prune_stale_modules::{direct_deps_of, remove_emptied_scope_dir},
    symlink_direct_dependencies::{importer_root_dir, validate_importer_id},
};
use pnpm_config::Config;
use pnpm_lockfile::{ImporterDepVersion, Lockfile, PkgName, ProjectSnapshot};
use pnpm_modules_yaml::HoistKind;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, RemovedRoot, Reporter, RootLog, RootMessage};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub(crate) struct PruneExtraneousDirectDeps<'a> {
    pub config: &'a Config,
    pub workspace_root: &'a Path,
    pub wanted_lockfile: &'a Lockfile,
    pub wanted_hoisted_dependencies: Option<&'a HoistedDependencies>,
    pub public_hoist_targets: Option<&'a BTreeMap<String, PathBuf>>,
    pub manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub included_groups: &'a [DependencyGroup],
    pub selected_importer_ids: Option<&'a HashSet<String>>,
}

impl<'a> PruneExtraneousDirectDeps<'a> {
    pub(crate) fn run<Reporter: self::Reporter>(self) -> Result<(), PruneDirectDepsError> {
        let wanted_root_deps = self.wanted_root_deps();
        let manifests_by_dir = self.manifests_by_dir();
        let mut importers: Vec<_> = self.wanted_lockfile.importers.iter().collect();
        importers.sort_by_key(|(id, _)| *id);
        for (importer_id, wanted_snapshot) in importers {
            if self.selected_importer_ids.is_some_and(|selected| !selected.contains(importer_id)) {
                continue;
            }
            self.prune_importer::<Reporter>(
                importer_id,
                wanted_snapshot,
                wanted_root_deps.as_ref(),
                manifests_by_dir.as_ref(),
            )?;
        }
        Ok(())
    }

    fn prune_importer<Reporter: self::Reporter>(
        &self,
        importer_id: &str,
        wanted_snapshot: &ProjectSnapshot,
        wanted_root_deps: Option<&HashMap<&'a PkgName, &'a ImporterDepVersion>>,
        manifests_by_dir: Option<&HashMap<PathBuf, &'a PackageManifest>>,
    ) -> Result<(), PruneDirectDepsError> {
        if validate_importer_id(importer_id).is_err() {
            return Ok(());
        }
        let modules_dir_name: &OsStr = self.config.modules_dir_name();
        let importer_dir = importer_root_dir(self.workspace_root, importer_id);
        let raw_modules_dir = importer_dir.join(modules_dir_name);
        if !is_real_dir(&importer_dir) || !is_real_dir(&raw_modules_dir) {
            return Ok(());
        }
        let Some(modules_dir) = confined_modules_dir(&raw_modules_dir, self.workspace_root) else {
            return Ok(());
        };
        let mut pkg_names = pnpm_fs::read_modules_dir(&modules_dir)
            .map_err(|error| PruneDirectDepsError::ReadModulesDir {
                path: modules_dir.clone(),
                error,
            })?;
        if pkg_names.is_empty() {
            return Ok(());
        }
        pkg_names.sort();
        let dedupe_against_root = (importer_id != ".").then_some(wanted_root_deps).flatten();
        let wanted = self.wanted_direct_names(
            importer_id,
            &importer_dir,
            wanted_snapshot,
            dedupe_against_root,
            manifests_by_dir,
        );
        remove_unwanted_entries::<Reporter>(&modules_dir, &importer_dir, pkg_names, &wanted)
    }

    fn wanted_direct_names(
        &self,
        importer_id: &str,
        importer_dir: &Path,
        wanted_snapshot: &ProjectSnapshot,
        dedupe_against_root: Option<&HashMap<&'a PkgName, &'a ImporterDepVersion>>,
        manifests_by_dir: Option<&HashMap<PathBuf, &'a PackageManifest>>,
    ) -> HashSet<String> {
        let mut wanted: HashSet<String> = direct_deps_of(wanted_snapshot, self.included_groups)
            .into_iter()
            .filter(|(alias, spec, _)| {
                !dedupe_against_root.is_some_and(|root_deps| {
                    root_deps
                        .get(alias)
                        .is_some_and(|version| **version == spec.version)
                })
            })
            .map(|(alias, _, _)| alias.to_string())
            .collect();
        self.add_excluded_manifest_links(importer_dir, manifests_by_dir, &mut wanted);
        if importer_id == "." {
            self.add_root_public_hoists(&mut wanted);
        }
        wanted
    }

    fn add_excluded_manifest_links(
        &self,
        importer_dir: &Path,
        manifests_by_dir: Option<&HashMap<PathBuf, &'a PackageManifest>>,
        wanted: &mut HashSet<String>,
    ) {
        let Some(manifests_by_dir) = manifests_by_dir else {
            return;
        };
        let norm_importer = pnpm_fs::lexical_normalize(importer_dir);
        let Some(manifest) = manifests_by_dir.get(&norm_importer) else {
            return;
        };
        let deps = manifest.dependencies(self.included_groups.iter().copied());
        wanted.extend(
            deps.into_iter()
                .filter(|(_, spec)| spec.starts_with("link:") || spec.starts_with("workspace:"))
                .map(|(alias, _)| alias.to_owned()),
        );
    }

    fn add_root_public_hoists(&self, wanted: &mut HashSet<String>) {
        if let Some(targets) = self.public_hoist_targets {
            wanted.extend(targets.keys().cloned());
        }
        let Some(hoisted) = self.wanted_hoisted_dependencies else {
            return;
        };
        wanted.extend(
            hoisted
                .values()
                .flatten()
                .filter(|(_, kind)| **kind == HoistKind::Public)
                .map(|(alias, _)| alias.clone()),
        );
    }

    fn manifests_by_dir(&self) -> Option<HashMap<PathBuf, &'a PackageManifest>> {
        self.config.exclude_links_from_lockfile.then(|| {
            self.manifests
                .iter()
                .map(|(dir, manifest)| (pnpm_fs::lexical_normalize(dir), *manifest))
                .collect()
        })
    }

    fn wanted_root_deps(&self) -> Option<HashMap<&'a PkgName, &'a ImporterDepVersion>> {
        let root_snapshot = self.config.dedupe_direct_deps.then(|| {
            self.wanted_lockfile.importers.get(".")
        })??;
        Some(
            direct_deps_of(root_snapshot, self.included_groups)
                .into_iter()
                .map(|(alias, spec, _)| (alias, &spec.version))
                .collect(),
        )
    }
}

fn remove_unwanted_entries<Reporter: self::Reporter>(
    modules_dir: &Path,
    importer_dir: &Path,
    pkg_names: Vec<String>,
    wanted: &HashSet<String>,
) -> Result<(), PruneDirectDepsError> {
    let prefix = importer_dir.display().to_string();
    for pkg_name in pkg_names {
        if wanted.contains(&pkg_name) {
            continue;
        }
        let removed = remove_extraneous_direct_dep(modules_dir, &pkg_name)?;
        remove_emptied_scope_dir(modules_dir, &pkg_name);
        if let Some(version) = removed {
            Reporter::emit(&LogEvent::Root(RootLog {
                level: LogLevel::Debug,
                message: RootMessage::Removed {
                    prefix: prefix.clone(),
                    removed: RemovedRoot { name: pkg_name, version, dependency_type: None },
                },
            }));
        }
    }
    Ok(())
}
