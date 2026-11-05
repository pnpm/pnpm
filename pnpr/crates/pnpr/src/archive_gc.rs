//! `pnpr gc`: reclaiming the archives no package references, in every hosted
//! registry, while the registry keeps serving and other replicas keep
//! publishing.
//!
//! Such archives are left by a publish refused at its commit whose cleanup
//! lost a race, by an unpublish whose archive removal failed, by an image
//! push that never got its manifest, or by a blob a client deleted halfway.
//! Every removal goes through the metadata database's fence (see
//! [`pnpr_storage::metadata::ArchiveFence`]), so a commit that depends on an
//! archive being removed is refused rather than left pointing at nothing.

pub(crate) mod liveness;
pub(crate) mod reclaim;

use crate::{Config, Ecosystem, RegistryError, Result};
use futures_util::TryStreamExt as _;
use liveness::{Live, archive_version};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{HostedBlobFile, Storage, metadata::ReclaimTarget};
use std::time::Duration;

/// What `pnpr gc` collects, and how.
#[derive(Debug, Default, Clone)]
pub struct GcOptions {
    /// The hosted registries to collect in; every hosted registry when
    /// empty.
    pub registries: Vec<String>,
    /// Archives younger than this are kept, so an upload whose commit is
    /// still coming is not refused for it. Named versions ignore it.
    pub min_age: Duration,
    /// Report what would be removed without removing it.
    pub dry_run: bool,
    /// `<name>@<version>` specs whose archives to remove regardless of age,
    /// in the one registry `registries` names.
    pub versions: Vec<String>,
}

/// What a collection removed, or would remove in a dry run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcReport {
    pub archives: usize,
    /// The bytes of the archives listed with their size; named versions'
    /// archives are counted without.
    pub bytes: u64,
}

impl GcReport {
    fn add(&mut self, other: Self) {
        self.archives += other.archives;
        self.bytes += other.bytes;
    }
}

/// Collect unreferenced archives as `options` say.
pub async fn collect_archives(config: &Config, options: &GcOptions) -> Result<GcReport> {
    if !options.versions.is_empty() && options.registries.len() != 1 {
        return Err(RegistryError::BadRequest {
            reason: "naming versions to remove requires exactly one --registry".to_string(),
        });
    }
    let registries = selected_registries(config, options)?;
    let storage = Storage::from_config(&config.storage)?;
    let manifest_limit = config.http.oci.max_manifest_bytes;
    let mut report = GcReport::default();
    for (org, ecosystem) in registries {
        let view = storage.for_hosted(&org, ecosystem);
        let collection = Collection { storage: &view, catalog: &org, ecosystem, manifest_limit };
        report.add(collection.run(options).await?);
    }
    Ok(report)
}

/// The `(org, ecosystem)` namespaces of the registries `options` selects,
/// each once.
fn selected_registries(config: &Config, options: &GcOptions) -> Result<Vec<(String, Ecosystem)>> {
    let names: Vec<&String> = if options.registries.is_empty() {
        config.routing.hosted.keys().collect()
    } else {
        options.registries.iter().collect()
    };
    let mut selected = Vec::new();
    for name in names {
        let hosted = config.routing.hosted
            .get(name)
            .ok_or_else(|| RegistryError::BadRequest {
                reason: format!("{name:?} is not a hosted registry"),
            })?;
        let ecosystem = config.routing.registries
            .ecosystem(name)
            .ok_or_else(|| RegistryError::BadRequest {
                reason: format!("{name:?} is not a concrete registry"),
            })?;
        let namespace = (hosted.org.clone(), ecosystem);
        if !selected.contains(&namespace) {
            selected.push(namespace);
        }
    }
    Ok(selected)
}

/// One `(org, ecosystem)` namespace being collected.
pub(crate) struct Collection<'storage> {
    /// The namespace's view of the hosted store.
    pub(crate) storage: &'storage Storage,
    pub(crate) catalog: &'storage str,
    pub(crate) ecosystem: Ecosystem,
    pub(crate) manifest_limit: usize,
}

impl Collection<'_> {
    /// Collect the namespace as `options` say.
    async fn run(&self, options: &GcOptions) -> Result<GcReport> {
        if options.dry_run {
            self.storage.check_hosted_layout().await?;
        } else {
            self.storage.ensure_hosted_layout().await?;
            self.finish_pending().await?;
        }
        if options.versions.is_empty() {
            self.collect(options.min_age, options.dry_run).await
        } else {
            self.collect_versions(&options.versions, options.dry_run).await
        }
    }

    /// Finish the reclaims of this namespace that stopped after marking
    /// their archives, which no commit can depend on until they finish.
    pub(crate) async fn finish_pending(&self) -> Result<()> {
        let pending: Vec<_> = self.storage
            .sql_metadata()
            .pending_reclaims(self.catalog)
            .await?
            .into_iter()
            .filter(|archive| archive.ecosystem == self.ecosystem)
            .collect();
        for archive in reclaim::finish(self.storage, self.catalog, pending).await? {
            tracing::info!(
                package = %archive.name,
                artifact = %archive.artifact,
                "finished an interrupted archive reclaim",
            );
        }
        Ok(())
    }

    /// Reclaim every archive of the namespace older than `min_age` that its
    /// package does not reference.
    pub(crate) async fn collect(&self, min_age: Duration, dry_run: bool) -> Result<GcReport> {
        let mut report = GcReport::default();
        let mut files = self.storage.hosted_blob_files();
        // The listing yields a package's files together, so each package is
        // collected once its files are all listed. One listed again later is
        // collected again, which finds what the first pass left.
        // A package whose metadata cannot be read keeps every archive, and
        // the others are still collected; the first such error is returned.
        let mut failure = None;
        let mut package: Option<(String, Vec<HostedBlobFile>)> = None;
        loop {
            let file = files.try_next().await?;
            let next_package = file.as_ref().map(|file| file.package.as_str());
            if package
                .as_ref()
                .is_some_and(|(current, _)| Some(current.as_str()) != next_package)
                && let Some((name, listed)) = package.take()
            {
                match self.collect_package(&name, listed, dry_run).await {
                    Ok(collected) => report.add(collected),
                    Err(error) => {
                        tracing::warn!(package = %name, %error, "archives not collected");
                        failure.get_or_insert(error);
                    }
                }
            }
            let Some(file) = file else { break };
            if file.blob.is_none() || file.modified.elapsed().unwrap_or_default() < min_age {
                continue;
            }
            match &mut package {
                Some((_, listed)) => listed.push(file),
                None => package = Some((file.package.clone(), vec![file])),
            }
        }
        failure.map_or(Ok(report), Err)
    }

    async fn collect_package(
        &self,
        package: &str,
        listed: Vec<HostedBlobFile>,
        dry_run: bool,
    ) -> Result<GcReport> {
        let Ok(name) = CanonicalPackageName::parse(package, self.ecosystem) else {
            return Ok(GcReport::default());
        };
        let (expected, live) = self.read_live(&name).await?;
        let unreferenced: Vec<(String, u64)> = listed
            .into_iter()
            .filter_map(|file| Some((file.blob?, file.size)))
            .filter(|(blob, _)| live.is_unreferenced(&name, blob))
            .collect();
        self.reclaim_files(&name, expected, unreferenced, dry_run).await
    }

    /// Reclaim the archives of each `<name>@<version>` of `specs`, whatever
    /// their age. A version the package still holds is refused.
    pub(crate) async fn collect_versions(
        &self,
        specs: &[String],
        dry_run: bool,
    ) -> Result<GcReport> {
        if self.ecosystem == Ecosystem::Oci {
            return Err(RegistryError::BadRequest {
                reason: "an image blob is named by its digest, not a version; delete it through \
                         the registry's blob DELETE route"
                    .to_string(),
            });
        }
        let mut report = GcReport::default();
        for spec in specs {
            let (raw, version) = parse_version_spec(spec)?;
            let name = CanonicalPackageName::parse(raw, self.ecosystem)?;
            let files: Vec<String> = self.storage
                .hosted_package_blobs(&name)
                .await?
                .into_iter()
                .filter(|blob| archive_version(&name, blob).as_deref() == Some(version))
                .collect();
            let (expected, live) = self.read_live(&name).await?;
            if files
                .iter()
                .any(|blob| !live.is_unreferenced(&name, blob))
            {
                return Err(RegistryError::BadRequest {
                    reason: format!(
                        "{spec} is a live version; unpublish it rather than removing its archives",
                    ),
                });
            }
            if files.is_empty() {
                tracing::info!(%spec, "no archive of this version is stored");
                continue;
            }
            let files = files
                .into_iter()
                .map(|blob| (blob, 0))
                .collect();
            report.add(self.reclaim_files(&name, expected, files, dry_run).await?);
        }
        Ok(report)
    }

    /// What `name`'s metadata keeps alive, with the package generation it
    /// was read at.
    async fn read_live(&self, name: &CanonicalPackageName) -> Result<(Option<u64>, Live)> {
        let base = self.storage.read_hosted_document_for_publish(name).await?;
        let bytes = base.stored.as_ref().map(|stored| stored.bytes.as_slice());
        let live = Live::of(self.storage, name, bytes, self.manifest_limit).await?;
        Ok((base.package_generation(), live))
    }

    async fn reclaim_files(
        &self,
        name: &CanonicalPackageName,
        expected: Option<u64>,
        files: Vec<(String, u64)>,
        dry_run: bool,
    ) -> Result<GcReport> {
        if files.is_empty() {
            return Ok(GcReport::default());
        }
        if dry_run {
            for (artifact, size) in &files {
                tracing::info!(package = name.as_str(), %artifact, size, "unreferenced archive");
            }
            let bytes = files.iter().map(|(_, size)| size).sum();
            return Ok(GcReport { archives: files.len(), bytes });
        }
        let target = ReclaimTarget {
            ecosystem: self.ecosystem,
            name: name.as_str().to_string(),
            expected,
            artifacts: files
                .iter()
                .map(|(artifact, _)| artifact.clone())
                .collect(),
        };
        let removed = reclaim::reclaim(self.storage, self.catalog, vec![target]).await?;
        let mut report = GcReport::default();
        for archive in removed {
            let size = files
                .iter()
                .find(|(artifact, _)| *artifact == archive.artifact)
                .map_or(0, |(_, size)| *size);
            tracing::info!(
                package = name.as_str(),
                artifact = %archive.artifact,
                size,
                "removed an unreferenced archive",
            );
            report.archives += 1;
            report.bytes += size;
        }
        Ok(report)
    }
}

/// Split `<name>@<version>`, where the name may start with a scope's `@`.
fn parse_version_spec(spec: &str) -> Result<(&str, &str)> {
    spec.rsplit_once('@')
        .filter(|(name, version)| !name.is_empty() && !version.is_empty())
        .ok_or_else(|| RegistryError::BadRequest {
            reason: format!("{spec:?} is not <name>@<version>"),
        })
}

#[cfg(test)]
mod tests;
