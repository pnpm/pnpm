use crate::{ImporterDepVersion, ProjectSnapshot, ResolvedDependencyMap};
use pnpm_fs::lexical_normalize_posix;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashMap;

/// The lockfile-relative directory that `link:` references to the importer
/// at `importer_id` point at, following the `publishConfig.directory` and
/// `linkDirectory` rules of the workspace resolver.
#[must_use]
pub fn importer_link_target(importer_id: &str, importer: &ProjectSnapshot) -> String {
    match importer.publish_directory.as_deref() {
        Some(publish_directory) if importer.link_directory != Some(false) => {
            normalize_dir(&format!("{importer_id}/{publish_directory}"))
        }
        _ => normalize_dir(importer_id),
    }
}

/// Normalize `dir` without a trailing slash, so `dist/` and `dist` compare equal.
fn normalize_dir(dir: &str) -> String {
    let normalized = lexical_normalize_posix(dir);
    match normalized.strip_suffix('/') {
        Some(trimmed) if !trimmed.is_empty() => trimmed.to_owned(),
        _ => normalized,
    }
}

/// Reconciling `publishDirectory` or `linkDirectory` moves where links to an
/// importer point. A dependent's `link:` reference that the fold took from the
/// branch lockfile still points at the old target, so the `pre_merge`
/// reference is restored when it points at the reconciled one.
///
/// `merged_link_targets` holds each importer's [`importer_link_target`] before
/// reconciliation. Reports whether any reference was restored.
pub fn restore_moved_links(
    importers: &mut HashMap<String, ProjectSnapshot>,
    pre_merge_importers: &HashMap<String, ProjectSnapshot>,
    merged_link_targets: &HashMap<String, String>,
    manifests: &[(String, &PackageManifest)],
) -> bool {
    let moved_targets: HashMap<&str, String> = merged_link_targets
        .iter()
        .filter_map(|(importer_id, merged_target)| {
            let reconciled_target = importer_link_target(importer_id, importers.get(importer_id)?);
            (reconciled_target != *merged_target).then_some((
                merged_target.as_str(),
                reconciled_target,
            ))
        })
        .collect();
    if moved_targets.is_empty() {
        return false;
    }
    let mut restored = false;
    for (importer_id, manifest) in manifests {
        let (Some(importer), Some(pre_merge)) =
            (importers.get_mut(importer_id), pre_merge_importers.get(importer_id))
        else {
            continue;
        };
        let target = MovedLinkContext { importer_id, manifest, moved_targets: &moved_targets };
        restored |= target.restore_importer(importer, pre_merge);
    }
    restored
}

struct MovedLinkContext<'ctx> {
    importer_id: &'ctx str,
    manifest: &'ctx PackageManifest,
    moved_targets: &'ctx HashMap<&'ctx str, String>,
}

impl MovedLinkContext<'_> {
    fn restore_importer(
        &self,
        importer: &mut ProjectSnapshot,
        pre_merge: &ProjectSnapshot,
    ) -> bool {
        let mut restored_specifiers = Vec::new();
        for (group, field) in [
            (DependencyGroup::Prod, &mut importer.dependencies),
            (DependencyGroup::Dev, &mut importer.dev_dependencies),
            (DependencyGroup::Optional, &mut importer.optional_dependencies),
        ] {
            let (Some(dependencies), Some(pre_deps)) =
                (field.as_mut(), pre_merge.get_map_by_group(group))
            else {
                continue;
            };
            self.restore_group(group, dependencies, pre_deps, &mut restored_specifiers);
        }
        if restored_specifiers.is_empty() {
            return false;
        }
        if let Some(specifiers) = importer.specifiers.as_mut() {
            specifiers.extend(restored_specifiers);
        }
        true
    }

    fn restore_group(
        &self,
        group: DependencyGroup,
        dependencies: &mut ResolvedDependencyMap,
        pre_deps: &ResolvedDependencyMap,
        restored_specifiers: &mut Vec<(String, String)>,
    ) {
        let declared_specs: HashMap<&str, &str> = self.manifest
            .dependencies([group])
            .collect();
        for (dep_name, merged_spec) in dependencies.iter_mut() {
            let Some(pre_spec) = pre_deps.get(dep_name) else { continue };
            let dep_key = dep_name.to_string();
            let expected_spec = declared_specs
                .get(dep_key.as_str())
                .copied()
                .unwrap_or(merged_spec.specifier.as_str());
            if pre_spec.specifier == expected_spec
                && self.is_moved_link(&merged_spec.version, &pre_spec.version)
            {
                *merged_spec = pre_spec.clone();
                restored_specifiers.push((dep_key, pre_spec.specifier.clone()));
            }
        }
    }

    fn is_moved_link(&self, merged: &ImporterDepVersion, pre: &ImporterDepVersion) -> bool {
        let (ImporterDepVersion::Link(merged_path), ImporterDepVersion::Link(pre_path)) =
            (merged, pre)
        else {
            return false;
        };
        merged_path != pre_path
            && self.moved_targets
                .get(self.resolve_link(merged_path).as_str())
                .is_some_and(|reconciled_target| *reconciled_target == self.resolve_link(pre_path))
    }

    fn resolve_link(&self, link_path: &str) -> String {
        normalize_dir(&format!("{}/{link_path}", self.importer_id))
    }
}

#[cfg(test)]
mod tests;
