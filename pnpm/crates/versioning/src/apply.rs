use std::{fs, path::Path};

use crate::{
    changelog::{compose_changelog_section, prepend_changelog_section},
    error::VersioningError,
    intents::{ChangeIntent, IntentBumpType},
    ledger::{Ledger, PackageConsumption, append_to_ledger, build_consumption_index},
    plan::ReleasePlan,
    settings::{ChangelogStorage, VersioningSettings},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedRelease {
    pub name: String,
    pub current_version: String,
    pub new_version: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ApplyReleasePlanOptions {
    /// Snapshot releases only rewrite manifest versions: they consume no
    /// intent files, write no changelogs, and leave the ledger untouched.
    pub snapshot: bool,
}

/// Applies an assembled release plan: manifest version updates, changelog
/// sections, the consumed-intents ledger, and intent-file cleanup.
/// `all_intents` is every intent file currently in the workspace, used to
/// decide which files are fully consumed after this run and can be deleted.
pub fn apply_release_plan(
    plan: &ReleasePlan,
    workspace_dir: &Path,
    all_intents: &[ChangeIntent],
    versioning: Option<&VersioningSettings>,
    opts: ApplyReleasePlanOptions,
) -> Result<Vec<AppliedRelease>, VersioningError> {
    assert_supported_changelog_storage(versioning)?;

    let mut applied = Vec::with_capacity(plan.releases.len());
    for release in &plan.releases {
        let manifest_path = release.root_dir.join("package.json");
        let mut manifest = pacquet_package_manifest::PackageManifest::from_path(manifest_path)
            .map_err(VersioningError::Manifest)?;
        manifest.value_mut()["version"] = serde_json::Value::String(release.new_version.clone());
        manifest.save().map_err(VersioningError::Manifest)?;
        applied.push(AppliedRelease {
            name: release.name.clone(),
            current_version: release.current_version.clone(),
            new_version: release.new_version.clone(),
        });
    }

    if opts.snapshot {
        return Ok(applied);
    }

    for release in &plan.releases {
        let section = compose_changelog_section(release);
        prepend_changelog_section(&release.root_dir, &release.name, &section)?;
    }

    let mut new_entries = Ledger::new();
    for release in &plan.releases {
        if release.intents.is_empty() {
            continue;
        }
        let mut ids: Vec<String> = release.intents.iter().map(|intent| intent.id.clone()).collect();
        ids.sort();
        new_entries.insert(format!("{}@{}", release.name, release.new_version), ids);
    }
    let ledger = append_to_ledger(workspace_dir, &new_entries)?;
    delete_consumed_intent_files(all_intents, &ledger, versioning).map(|()| applied)
}

/// An intent file is deletable once every package it names has a ledger entry
/// for it, with one exemption: while a package is still on a prerelease line,
/// entries against prerelease versions alone keep the file alive — its prose
/// is still needed to compose the stable changelog section at graduation.
/// Declined (`none`) entries demand no release and never block deletion.
fn delete_consumed_intent_files(
    all_intents: &[ChangeIntent],
    ledger: &Ledger,
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    let consumption = build_consumption_index(ledger);
    let empty = PackageConsumption::default();
    let consumption_of =
        |pkg_name: &str| -> &PackageConsumption { consumption.get(pkg_name).unwrap_or(&empty) };
    let on_prerelease_line = |pkg_name: &str| -> bool {
        versioning.is_some_and(|settings| settings.prereleases.contains_key(pkg_name))
    };

    for intent in all_intents {
        let deletable = intent.releases.iter().all(|(pkg_name, bump_type)| {
            if *bump_type == IntentBumpType::None {
                return true;
            }
            let consumed = consumption_of(pkg_name);
            consumed.all_ids.contains(&intent.id)
                && !(on_prerelease_line(pkg_name)
                    && consumed.prerelease_only_ids.contains(&intent.id))
        });
        if deletable {
            fs::remove_file(&intent.file_path).map_err(|source| VersioningError::Write {
                path: intent.file_path.clone(),
                source,
            })?;
        }
    }
    Ok(())
}

fn assert_supported_changelog_storage(
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    let storage = versioning
        .and_then(|settings| settings.changelog.as_ref())
        .and_then(|changelog| changelog.storage);
    match storage {
        None | Some(ChangelogStorage::Repository) => Ok(()),
        Some(storage) => {
            Err(VersioningError::UnsupportedChangelogStorage { storage: storage.to_string() })
        }
    }
}

#[cfg(test)]
mod tests;
