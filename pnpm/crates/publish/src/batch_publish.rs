//! Publish several packed packages through pnpr's atomic batch endpoint.

pub use error::BatchPublishError;

use pnpm_network_web_auth::Host as WebAuthHost;
use pnpm_reporter::Reporter;
use serde_json::Value;

use crate::{
    PublishFailure,
    failed_to_publish_error::FailedToPublishError,
    global_log::{global_info, global_warn},
    publish_options::{find_registry_info, resolve_access},
    publish_packed_pkg::{
        DistHashes, PackedPkg, PublishNetwork, PublishPackedPkgOptions, build_publish_document,
        join_registry, publish_with_otp_handling, registry_for_display, web_auth_fetch_options,
    },
    publish_summary::PublishSummary,
    publish_wait::{PublishWaitError, wait_for_published_packages},
    registry_config_keys::NormalizedRegistryUrl,
    staged_archives::{
        MAX_STAGED_ARCHIVES_PER_PUBLISH, StagedArchive, reference_staged_archive, stage_archives,
    },
};

mod error;
mod variants;

const BATCH_PUBLISH_ENDPOINT: &str = "-/pnpr/v0/publish";

struct BatchGroup<'a> {
    registry: NormalizedRegistryUrl,
    package_names: Vec<String>,
    summary_indexes: Vec<usize>,
    documents: Vec<Value>,
    /// How each document's archives travel, in the order of `documents`.
    archives: Vec<DocumentArchives<'a>>,
    /// The archives uploaded before the commit, for a registry that stages
    /// them; `None` sends each archive inline in its document.
    staged: Option<Vec<StagedArchive<'a>>>,
}

/// The archives of one document of a batch.
enum DocumentArchives<'a> {
    /// The version's one archive, by its integrity: inline in the document,
    /// or staged and referenced when its group stages.
    Single(String),
    /// A version with platform variants, or with an `any` archive another
    /// job staged, whose archives are always staged and referenced.
    Variants(&'a PackedPkg<'a>),
}

impl BatchGroup<'_> {
    /// Make the documents of a staging group that has one archive each name
    /// it by integrity. A group with more archives than one commit may
    /// reference sends them inline, as a registry that does not stage gets
    /// them, unless a version with variants needs them staged.
    fn reference_staged_archives(&mut self) -> Result<(), BatchPublishError> {
        let Some(archives) = &self.staged else {
            return Ok(());
        };
        let references = archives.len() + variants::references_staged_before(&self.archives);
        if references > MAX_STAGED_ARCHIVES_PER_PUBLISH {
            if self.archives
                .iter()
                .any(|archives| matches!(archives, DocumentArchives::Variants(_)))
            {
                return Err(BatchPublishError::TooManyArchives {
                    registry: registry_for_display(&self.registry),
                    count: references,
                });
            }
            self.staged = None;
            return Ok(());
        }
        for (document, archives) in self.documents.iter_mut().zip(&self.archives) {
            if let DocumentArchives::Single(integrity) = archives {
                reference_staged_archive(document, integrity);
            }
        }
        Ok(())
    }
}

/// Reject publish modes whose per-package artifacts cannot be represented by
/// one batch request.
pub fn validate_batch_publish_options(
    opts: &PublishPackedPkgOptions,
) -> Result<(), BatchPublishError> {
    opts.validate()?;
    if opts.stage {
        return Err(BatchPublishError::Stage);
    }
    if opts.registry.provenance == Some(true) {
        return Err(BatchPublishError::Provenance);
    }
    Ok(())
}

/// Publish every packed package in one request per target registry, calling
/// `on_group_complete` with the input indexes after each completed registry
/// group (including dry-run groups).
pub async fn batch_publish_packed_pkgs<'a, Reporter, OnGroupComplete, Error>(
    packages: &'a [PackedPkg<'a>],
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
    mut on_group_complete: OnGroupComplete,
) -> Result<Vec<PublishSummary>, PublishFailure<Error>>
where
    Reporter: self::Reporter,
    OnGroupComplete: FnMut(&[usize]) -> Result<(), Error>,
    Error: From<BatchPublishError>,
{
    validate_batch_publish_options(opts).map_err(Error::from)?;

    let mut summaries = Vec::with_capacity(packages.len());
    let mut groups = group_packed_pkgs(packages, opts, &mut summaries).map_err(Error::from)?;

    let authorizations = prepare_groups(&mut groups, opts.dry_run, network)
        .await
        .map_err(Error::from)?;

    let mut accepted = Vec::new();
    let result = async {
        for (group, authorization) in groups.into_iter().zip(authorizations) {
            publish_group::<Reporter>(&group, authorization.as_deref(), opts, network, &summaries)
                .await?;
            if !opts.dry_run {
                accepted.extend(
                    group.summary_indexes
                        .iter()
                        .map(|&index| summaries[index].clone()),
                );
                wait_for_group::<Reporter>(&group, &summaries, opts, network)
                    .await
                    .map_err(BatchPublishError::Wait)?;
            }
            on_group_complete(&group.summary_indexes)?;
        }
        Ok::<_, Error>(())
    }
    .await;
    result.map_err(|error| PublishFailure { published: accepted, error })?;

    Ok(summaries)
}

/// Each group's credential, once every group's documents reference the
/// archives other jobs staged. A dry run sends nothing, so it needs neither.
async fn prepare_groups(
    groups: &mut [BatchGroup<'_>],
    dry_run: bool,
    network: &PublishNetwork<'_>,
) -> Result<Vec<Option<String>>, BatchPublishError> {
    if dry_run {
        return Ok(vec![None; groups.len()]);
    }
    let authorizations = groups
        .iter()
        .map(|group| batch_authorization(group, network))
        .collect::<Result<Vec<_>, _>>()?;
    variants::reference_every_group(groups, &authorizations, network.client).await?;
    Ok(authorizations)
}

async fn publish_group<Reporter: self::Reporter>(
    group: &BatchGroup<'_>,
    authorization: Option<&str>,
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
    summaries: &[PublishSummary],
) -> Result<(), BatchPublishError> {
    let registry = registry_for_display(&group.registry);
    for &summary_index in &group.summary_indexes {
        global_info::<Reporter>(&format!("📦 {} → {registry}", summaries[summary_index].id));
    }
    if opts.dry_run {
        global_warn::<Reporter>(&format!(
            "Skip publishing {} package(s) to {registry} (dry run)",
            group.documents.len(),
        ));
        return Ok(());
    }
    if let Some(archives) = &group.staged {
        let uploaded =
            stage_archives(network.client, &group.registry, authorization, archives).await?;
        global_info::<Reporter>(&format!(
            "Uploaded {uploaded} of {} archive(s) to {registry} ahead of the publish",
            archives.len(),
        ));
    }
    put_batch::<Reporter>(group, authorization, opts, network, &registry).await?;
    global_info::<Reporter>(&format!(
        "✅ Published {} package(s) to {registry} in a single request",
        group.summary_indexes.len(),
    ));
    Ok(())
}

async fn wait_for_group<Reporter: self::Reporter>(
    group: &BatchGroup<'_>,
    summaries: &[PublishSummary],
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
) -> Result<(), PublishWaitError> {
    let packages = group.summary_indexes
        .iter()
        .map(|&index| {
            let summary = &summaries[index];
            (summary.name.as_str(), summary.version.as_str())
        })
        .collect::<Vec<_>>();
    wait_for_published_packages::<Reporter>(&packages, &group.registry, network, opts.wait_timeout)
        .await
}

/// Summarize every packed package into `summaries` and group the packages
/// by the registry they are bound for, each group's documents referencing
/// the archives it stages.
fn group_packed_pkgs<'a>(
    packages: &'a [PackedPkg<'a>],
    opts: &PublishPackedPkgOptions,
    summaries: &mut Vec<PublishSummary>,
) -> Result<Vec<BatchGroup<'a>>, BatchPublishError> {
    let mut groups: Vec<BatchGroup> = Vec::new();
    for package in packages {
        group_packed_pkg(package, opts, summaries, &mut groups)?;
    }
    for group in &mut groups {
        group.reference_staged_archives()?;
    }
    Ok(groups)
}

/// Summarize one packed package and fold it into the group of everything
/// bound for the same registry.
fn group_packed_pkg<'a>(
    package: &'a PackedPkg<'a>,
    opts: &PublishPackedPkgOptions,
    summaries: &mut Vec<PublishSummary>,
    groups: &mut Vec<BatchGroup<'a>>,
) -> Result<(), BatchPublishError> {
    let manifest = package.published_manifest;
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let registry = find_registry_info(
        name,
        &opts.registry.default,
        &opts.registry.scoped,
        crate::publish_options::publish_config_registry(manifest, name),
    )?;
    crate::publish_packed_pkg::check_variants_registry(package, &opts.pnpr_registries, &registry)?;
    let summary = package.summary();
    let mut document = build_publish_document(
        manifest,
        package.tarball_data,
        &registry,
        resolve_access(opts.registry.access, manifest),
        &opts.registry.tag,
        &DistHashes { integrity: &summary.integrity, shasum: &summary.shasum },
    )?;
    opts.release_notes.attach_version_notes(&mut document, package.notes, &registry);
    let stages = opts.pnpr_registries.contains(registry.as_str());
    let (archives, staged) = variants::document_archives(package, &summary.integrity, stages);
    let summary_index = summaries.len();
    summaries.push(summary);

    let group = group_for(groups, registry, stages);
    group.package_names.push(name.to_string());
    group.summary_indexes.push(summary_index);
    group.documents.push(document);
    group.archives.push(archives);
    if let Some(group_staged) = &mut group.staged {
        group_staged.extend(staged);
    }
    Ok(())
}

/// The group of everything bound for `registry`, started when this is its
/// first package. A group `stages` its archives when its registry does.
fn group_for<'a, 'g>(
    groups: &'g mut Vec<BatchGroup<'a>>,
    registry: NormalizedRegistryUrl,
    stages: bool,
) -> &'g mut BatchGroup<'a> {
    if let Some(index) = groups
        .iter()
        .position(|group| group.registry == registry)
    {
        return &mut groups[index];
    }
    groups.push(BatchGroup {
        registry,
        package_names: Vec::new(),
        summary_indexes: Vec::new(),
        documents: Vec::new(),
        archives: Vec::new(),
        staged: stages.then(Vec::new),
    });
    groups.last_mut().expect("a group was just pushed")
}

/// Send one registry's whole batch in a single request.
async fn put_batch<Reporter: self::Reporter>(
    group: &BatchGroup<'_>,
    authorization: Option<&str>,
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
    registry: &str,
) -> Result<(), BatchPublishError> {
    let put_url = join_registry(&group.registry, BATCH_PUBLISH_ENDPOINT)?;
    let body = bytes::Bytes::from(
        serde_json::to_vec(&opts.release_notes.batch_body(&group.registry, &group.documents))
            .expect("serialize batch publish documents"),
    );
    let response = publish_with_otp_handling::<WebAuthHost, Reporter>(
        network.client,
        &put_url,
        authorization,
        "publish",
        body,
        opts.registry.otp.as_deref(),
        false,
        web_auth_fetch_options(&opts.registry.http),
    )
    .await?;
    if response.ok {
        return Ok(());
    }
    if matches!(response.status, 404 | 405) {
        return Err(BatchPublishError::Unsupported { registry: registry.to_string() });
    }
    Err(BatchPublishError::Failed(FailedToPublishError::new_batch(
        group.package_names.len(),
        registry,
        response.status,
        response.status_text,
        response.body,
    )))
}

fn batch_authorization(
    group: &BatchGroup<'_>,
    network: &PublishNetwork<'_>,
) -> Result<Option<String>, BatchPublishError> {
    let mut package_names = group.package_names.iter();
    let authorization = package_names
        .next()
        .and_then(|name| {
            network.auth_headers.for_url_with_package(group.registry.as_str(), Some(name))
        });
    if package_names.any(|name| {
        network.auth_headers.for_url_with_package(group.registry.as_str(), Some(name))
            != authorization
    }) {
        return Err(BatchPublishError::ConflictingCredentials {
            registry: registry_for_display(&group.registry),
        });
    }
    Ok(authorization)
}

#[cfg(test)]
mod tests;
