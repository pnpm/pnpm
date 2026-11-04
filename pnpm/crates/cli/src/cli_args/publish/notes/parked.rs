//! The publication `pnpm version -r` parked for a batch: which one a batch
//! publishes, the registry it goes to, and the checks that keep its name
//! and note within the release they were composed for.

use super::{notes_registry_of_manifest, package_id};
use crate::cli_args::{changelog::published_name, publish::PackedDirectory};
use pnpm_config::Config;
use pnpm_publish::PublicationNotes;
use pnpm_versioning::read_parked_publications;
use serde_json::Value;
use std::collections::HashSet;

/// Refuse a batch whose packing moved a member of the composed publication
/// to another registry, or changed its published name or version: the
/// publication's name and note go to the registry chosen before packing, and
/// a later `pnpm version -r` confirms the release under the identity it
/// composed. Other packages of the batch may change as their hooks like.
pub(in crate::cli_args::publish) fn check_packed_members(
    config: &Config,
    publication: &PublicationNotes,
    packed: &[PackedDirectory],
) -> miette::Result<()> {
    let Some(registry) = publication.registry.as_deref() else {
        return Ok(());
    };
    let sources: Vec<&Value> = packed
        .iter()
        .map(|pkg| &pkg.source_manifest)
        .collect();
    let Some(parked) = parked_publication(config, &sources)? else {
        return Ok(());
    };
    let changed = packed
        .iter()
        .find(|pkg| {
            let is_member = release_key(&pkg.source_manifest)
                .is_some_and(|key| parked.members.contains(&key));
            is_member
                && (notes_registry_of_manifest(config, &pkg.published_manifest).as_deref()
                    != Some(registry)
                    || published_identity(&pkg.published_manifest)
                        != published_identity(&pkg.source_manifest))
        });
    let Some(pkg) = changed else {
        return Ok(());
    };
    let package = package_id(&pkg.source_manifest);
    Err(miette::miette!(
        code = "ERR_PNPM_RELEASE_NOTES_PACKAGE_CHANGED",
        help = "Set the registry, name, and version in the workspace manifest rather than in a beforePacking hook.",
        "Packing changed the registry, name, or version of {package}, which the release notes of its release were composed for",
    ))
}

/// The `package@version` a workspace manifest is keyed by in the ledger and
/// in a parked publication.
fn release_key(manifest: &Value) -> Option<String> {
    let name = manifest.get("name")?.as_str()?;
    let version = manifest.get("version")?.as_str()?;
    Some(format!("{name}@{version}"))
}

/// The parked publication that `members` (manifests of the workspace)
/// publish. A batch is one publication, so it may publish the members of at
/// most one parked publication, and all of them: otherwise a composed
/// publication note would be lost or sent for a release it does not
/// describe.
pub(super) fn parked_publication(
    config: &Config,
    members: &[&Value],
) -> miette::Result<Option<pnpm_versioning::ParkedPublication>> {
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(None);
    };
    let keys: HashSet<String> = members
        .iter()
        .filter_map(|manifest| release_key(manifest))
        .collect();
    let mut touched: Vec<pnpm_versioning::ParkedPublication> =
        read_parked_publications(workspace_dir)?
            .into_iter()
            .filter(|publication| {
                publication.members
                    .iter()
                    .any(|member| keys.contains(member))
            })
            .collect();
    match touched.len() {
        0 => Ok(None),
        1 => {
            let publication = touched.remove(0);
            let whole = publication.members
                .iter()
                .all(|member| keys.contains(member));
            if whole {
                Ok(Some(publication))
            } else {
                Err(partial_parked_publication(&publication))
            }
        }
        _ => Err(several_parked_publications()),
    }
}

/// The one registry that stores notes the members of `publication` publish
/// to: its name and note go in that registry's batch request alone. `None`
/// when none of them publishes to such a registry any more. A release whose
/// members publish to several registries, or only partly to one that stores
/// notes, is refused, since its notes would describe packages published
/// without them.
pub(super) fn publication_registry(
    config: &Config,
    publication: &pnpm_versioning::ParkedPublication,
    members: &[&Value],
) -> miette::Result<Option<String>> {
    let registries: HashSet<Option<String>> = members
        .iter()
        .filter(|manifest| {
            release_key(manifest).is_some_and(|key| publication.members.contains(&key))
        })
        .map(|manifest| notes_registry_of_manifest(config, manifest))
        .collect();
    if registries.len() > 1 {
        return Err(miette::miette!(
            code = "ERR_PNPM_RELEASE_NOTES_SEVERAL_REGISTRIES",
            help = "Publish the packages of one release to one registry.",
            r#"The packages of the release that "pnpm version -r" composed release notes for publish to several registries, or only some of them to a registry declared with serverType: pnpr"#,
        ));
    }
    Ok(registries.into_iter().next().flatten())
}

fn several_parked_publications() -> miette::Report {
    miette::miette!(
        code = "ERR_PNPM_RELEASE_NOTES_SEVERAL_PUBLICATIONS",
        help = r#"Publish each release that "pnpm version -r" composed notes for with its own "pnpm publish -r --batch", selecting its packages with --filter."#,
        r#"This batch publishes packages of several releases that "pnpm version -r" composed release notes for, and a batch is one release"#,
    )
}

fn partial_parked_publication(publication: &pnpm_versioning::ParkedPublication) -> miette::Report {
    miette::miette!(
        code = "ERR_PNPM_RELEASE_NOTES_PARTIAL_PUBLICATION",
        help = format!("Publish {} together, in one batch.", publication.members.join(", "),),
        r#"This batch publishes only part of a release that "pnpm version -r" composed release notes for"#,
    )
}

/// The name and version a manifest publishes under: its `publishConfig.name`
/// rename, and its version without build metadata, as packing writes them.
fn published_identity(manifest: &Value) -> (Option<&str>, Option<&str>) {
    let name = published_name(manifest).or_else(|| manifest.get("name").and_then(Value::as_str));
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .map(|version| version.split_once('+').map_or(version, |(version, _)| version));
    (name, version)
}
