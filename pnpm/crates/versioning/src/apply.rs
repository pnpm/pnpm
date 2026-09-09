use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
};

use serde::Serialize;

use crate::{
    changelog::{compose_changelog_section, prepend_changelog_section},
    error::VersioningError,
    intents::{ChangeIntent, IntentBumpType},
    ledger::{Ledger, PackageConsumption, append_to_ledger, build_consumption_index},
    pending::{remove_pending_changelog, write_pending_changelog},
    plan::{PlannedRelease, ProjectRefIndex, ReleasePlan, WorkspaceProject, index_project_refs},
    settings::{ChangelogStorage, VersioningSettings, changelog_storage},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedRelease {
    pub name: String,
    pub current_version: String,
    pub new_version: String,
}

/// Applies an assembled release plan: manifest version updates, changelog
/// sections, the consumed-intents ledger, and intent-file cleanup.
/// `all_intents` is every intent file currently in the workspace, used to
/// decide which files are fully consumed after this run and can be deleted.
///
/// In `registry` changelog storage the ledger alone does not authorize
/// deletion: the repository is the only copy of the prose until the release is
/// published, so an entry counts as consumed only once the registry confirms
/// its version carries the composed section. `confirmed_published` is the set
/// of `package@version` ledger keys the caller has confirmed (see
/// `read_pending_changelog` for the section it verifies against); their parked
/// section files are removed here, their prose now living in the published
/// tarball. It is ignored in `repository` storage, where the committed
/// changelog makes the ledger alone sufficient.
pub fn apply_release_plan(
    plan: &ReleasePlan,
    workspace_dir: &Path,
    projects: &[WorkspaceProject],
    all_intents: &[ChangeIntent],
    versioning: Option<&VersioningSettings>,
    confirmed_published: &HashSet<String>,
) -> Result<Vec<AppliedRelease>, VersioningError> {
    let storage = changelog_storage(versioning);

    let applied = write_new_versions(plan)?;
    for release in &plan.releases {
        write_changelog_section(release, workspace_dir, storage)?;
    }

    let ledger = append_to_ledger(workspace_dir, &ledger_entries(plan))?;
    let refs = index_project_refs(projects, workspace_dir);
    remove_consumed_intents(
        all_intents,
        &refs,
        versioning,
        &consumed_ledger(ledger, storage, confirmed_published),
    )?;

    // A confirmed release's parked section has served its purpose — its prose
    // is now in the published tarball — so collect it regardless of whether
    // the intents behind it were also collected.
    if storage == ChangelogStorage::Registry {
        for key in confirmed_published {
            if let Some((name, version)) = split_ledger_key(key) {
                remove_pending_changelog(workspace_dir, name, version)?;
            }
        }
    }

    Ok(applied)
}

/// Stamp every released manifest with its new version.
fn write_new_versions(plan: &ReleasePlan) -> Result<Vec<AppliedRelease>, VersioningError> {
    let mut applied = Vec::with_capacity(plan.releases.len());
    for release in &plan.releases {
        let manifest_path = release.root_dir.join("package.json");
        let mut manifest = pnpm_package_manifest::PackageManifest::from_path(manifest_path)
            .map_err(VersioningError::Manifest)?;
        manifest.value_mut()["version"] = serde_json::Value::String(release.new_version.clone());
        manifest.save().map_err(VersioningError::Manifest)?;
        applied.push(AppliedRelease {
            name: release.name.clone(),
            current_version: release.current_version.clone(),
            new_version: release.new_version.clone(),
        });
    }
    Ok(applied)
}

/// In `repository` storage the section is committed to CHANGELOG.md now. In
/// `registry` storage nothing is committed to the package; the section is
/// parked until publish, when it is packed into the tarball.
fn write_changelog_section(
    release: &PlannedRelease,
    workspace_dir: &Path,
    storage: ChangelogStorage,
) -> Result<(), VersioningError> {
    let section = compose_changelog_section(release);
    match storage {
        ChangelogStorage::Repository => {
            prepend_changelog_section(&release.root_dir, &release.name, &section)
        }
        ChangelogStorage::Registry => {
            write_pending_changelog(workspace_dir, &release.name, &release.new_version, &section)
        }
    }
}

/// The ledger entries this run adds: one per release that consumed intents,
/// naming the intent ids it carried.
fn ledger_entries(plan: &ReleasePlan) -> BTreeMap<String, (String, Vec<String>)> {
    let mut entries = BTreeMap::new();
    for release in &plan.releases {
        if release.intents.is_empty() {
            continue;
        }
        let mut ids: Vec<String> = release.intents.iter().map(|intent| intent.id.clone()).collect();
        ids.sort();
        entries.insert(
            format!("{}@{}", release.name, release.new_version),
            (release.dir.clone(), ids),
        );
    }
    entries
}

/// In `registry` storage only registry-confirmed entries count as consumed
/// (see the function contract), so the ledger is filtered down to those.
fn consumed_ledger(
    ledger: Ledger,
    storage: ChangelogStorage,
    confirmed_published: &HashSet<String>,
) -> Ledger {
    match storage {
        ChangelogStorage::Repository => ledger,
        ChangelogStorage::Registry => {
            ledger.into_iter().filter(|(key, _)| confirmed_published.contains(key)).collect()
        }
    }
}

/// Delete every intent file whose work is fully released.
fn remove_consumed_intents(
    all_intents: &[ChangeIntent],
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
    consumed_ledger: &Ledger,
) -> Result<(), VersioningError> {
    let consumption = build_consumption_index(consumed_ledger, |name| refs.name_to_dirs(name))?;
    let mut lane_dirs: HashSet<String> = HashSet::new();
    for reference in versioning.map(|settings| settings.lanes.keys()).into_iter().flatten() {
        lane_dirs.extend(refs.ref_to_dirs(reference));
    }

    for intent in all_intents {
        let deletable = intent.releases.iter().all(|(reference, bump_type)| {
            is_release_consumed(intent, reference, *bump_type, refs, &consumption, &lane_dirs)
        });
        if deletable {
            fs::remove_file(&intent.file_path).map_err(|source| VersioningError::Remove {
                path: intent.file_path.clone(),
                source,
            })?;
        }
    }
    Ok(())
}

/// Whether one of an intent's releases no longer needs the intent file.
///
/// A project needs a consumed ledger entry for the intent, with one
/// exemption: while a project is still on a lane, entries against prerelease
/// versions alone keep the file alive — its prose is still needed to compose
/// the stable changelog section at graduation. Declined (`none`) entries
/// demand no release and never block deletion. References here were already
/// validated by the plan assembly, so an unresolvable one just keeps its file
/// around.
fn is_release_consumed(
    intent: &ChangeIntent,
    reference: &str,
    bump_type: IntentBumpType,
    refs: &ProjectRefIndex,
    consumption: &HashMap<String, PackageConsumption>,
    lane_dirs: &HashSet<String>,
) -> bool {
    if bump_type == IntentBumpType::None {
        return true;
    }
    let dirs = refs.ref_to_dirs(reference);
    let [dir] = dirs.as_slice() else {
        return false;
    };
    let empty = PackageConsumption::default();
    let consumed = consumption.get(dir).unwrap_or(&empty);
    consumed.all_ids.contains(&intent.id)
        && !(lane_dirs.contains(dir) && consumed.prerelease_only_ids.contains(&intent.id))
}

/// Splits a `package@version` ledger key. The leading `@` of a scoped name is
/// not a separator, so the split is on the last `@` and a key that is all name
/// (index 0 or absent) yields `None`.
fn split_ledger_key(key: &str) -> Option<(&str, &str)> {
    let at = key.rfind('@')?;
    if at == 0 {
        return None;
    }
    Some((&key[..at], &key[at + 1..]))
}

#[cfg(test)]
mod tests;
