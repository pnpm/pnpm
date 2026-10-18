//! Tying a lockfile's tar integrity to the `.tgz` integrity a registry
//! signature covers.

use super::{Config, HashMap, Packument, SignaturePackage, ThrottledClient};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{StoreIndex, StoreIndexWriter, store_index_key};
use pnpm_tarball::{FetchTarballForResolution, IntegrityKind, ResolutionStore, TarballPackage};
use ssri::Integrity;

/// The integrity a registry signature covers, for every package whose
/// lockfile entry pins its tar, keyed by [`tie_key`].
///
/// That is the registry's `dist.integrity`, once the lockfile's tar hash is
/// tied to it: by the store row reachable under it, which records the tar
/// it holds, or else by downloading the archive, checking it against
/// `dist.integrity`, and hashing its tar. An `Err` says why an entry could
/// not be tied.
pub(super) async fn tie_tar_integrities(
    packages: &[SignaturePackage],
    packuments: &HashMap<(String, String), Result<Option<Packument>, String>>,
    config: &'static Config,
    http_client: &ThrottledClient,
) -> HashMap<TieKey, Result<String, String>> {
    let pending = pending_ties(packages, packuments);
    let stored = stored_tar_integrities(&pending, config);
    let (index_writer, writer_task) = StoreIndexWriter::spawn_for(
        &config.store_dir,
        config.frozen_store,
        config.store_index_format,
    );
    let downloads = DownloadContext { config, http_client, index_writer: &index_writer };
    let ties = pending
        .iter()
        .map(|tie| async {
            let key = tie_key(tie.pkg);
            (key, tie.resolve(&stored, &downloads).await)
        });
    let ties = futures_util::future::join_all(ties).await.into_iter().collect();
    drop(index_writer);
    StoreIndexWriter::drain(writer_task, " after tying tar integrities").await;
    ties
}

/// The integrity the registry's signature covers: the lockfile's own, or for
/// a tar pin, the one it was tied to.
pub(super) fn signed_integrity(
    pkg: &SignaturePackage,
    tie: Option<&Result<String, String>>,
    integrity: String,
) -> Result<String, String> {
    match tie {
        Some(tie) => tie.clone(),
        None if pkg.pins_tar => {
            Err(format!("{}@{} has no signed integrity tied to its tar", pkg.name, pkg.version))
        }
        None => Ok(integrity),
    }
}

/// One lockfile entry: a `name@version` from a registry, pinned by an
/// integrity.
pub(super) type TieKey = (String, String, String, Option<String>);

pub(super) fn tie_key(pkg: &SignaturePackage) -> TieKey {
    (pkg.registry.clone(), pkg.name.clone(), pkg.version.clone(), pkg.integrity.clone())
}

/// The signed versions whose lockfile entry pins their tar.
fn pending_ties<'a>(
    packages: &'a [SignaturePackage],
    packuments: &'a HashMap<(String, String), Result<Option<Packument>, String>>,
) -> Vec<PendingTie<'a>> {
    packages
        .iter()
        .filter(|pkg| pkg.pins_tar)
        .filter_map(|pkg| {
            let Some(Ok(Some(packument))) =
                packuments.get(&(pkg.registry.clone(), pkg.name.clone()))
            else {
                return None;
            };
            let dist = packument.versions
                .get(&pkg.version)
                .and_then(|version| version.dist.as_ref())?;
            // A version without well-formed signatures fails whatever its
            // tar hash is tied to.
            super::parse_signatures(dist.signatures.as_ref())
                .filter(|signatures| !signatures.is_empty())?;
            Some(PendingTie {
                pkg,
                lockfile_integrity: pkg.integrity.as_deref()?,
                published: dist.integrity.as_deref().zip(dist.tarball.as_deref()),
            })
        })
        .collect()
}

/// What a download that ties a tar integrity needs.
struct DownloadContext<'a> {
    config: &'static Config,
    http_client: &'a ThrottledClient,
    index_writer: &'a std::sync::Arc<StoreIndexWriter>,
}

struct PendingTie<'a> {
    pkg: &'a SignaturePackage,
    lockfile_integrity: &'a str,
    /// The published `dist.integrity` and `dist.tarball`.
    published: Option<(&'a str, &'a str)>,
}

impl PendingTie<'_> {
    fn package_id(&self) -> String {
        format!("{}@{}", self.pkg.name, self.pkg.version)
    }

    /// The signed integrity this entry is tied to: directly when the registry
    /// publishes the tar's, through the store row that recorded the tar, or by
    /// downloading the archive.
    async fn resolve(
        &self,
        stored: &HashMap<String, String>,
        downloads: &DownloadContext<'_>,
    ) -> Result<String, String> {
        let Some((signed, tarball)) = self.published else {
            return Err(format!(
                "{}@{} has no published integrity to tie its tar integrity to",
                self.pkg.name, self.pkg.version,
            ));
        };
        let pins_bytes = self.lockfile_integrity
            .parse::<Integrity>()
            .is_ok_and(|integrity| !integrity.hashes.is_empty());
        if !pins_bytes {
            return Err(format!(
                "{}@{} has a lockfile integrity that pins no archive",
                self.pkg.name, self.pkg.version,
            ));
        }
        let tied = signed == self.lockfile_integrity
            || stored
                .get(&store_index_key(signed, &self.package_id()))
                .map(String::as_str)
                == Some(self.lockfile_integrity);
        if !tied {
            download_tie(self, signed, tarball, downloads).await?;
        }
        Ok(signed.to_string())
    }
}

/// The tar integrity the store row of each pending package's published
/// archive records, by that row's store-index key: one `name@version` from
/// two registries is two archives.
fn stored_tar_integrities(pending: &[PendingTie<'_>], config: &Config) -> HashMap<String, String> {
    let Ok(index) = StoreIndex::open_readonly_in(&config.store_dir) else { return HashMap::new() };
    pending
        .iter()
        .filter_map(|tie| {
            let (signed, _) = tie.published?;
            let key = store_index_key(signed, &tie.package_id());
            let row = index.get(&key).ok().flatten()?;
            Some((key, row.tar_integrity?))
        })
        .collect()
}

async fn download_tie(
    tie: &PendingTie<'_>,
    signed: &str,
    tarball: &str,
    downloads: &DownloadContext<'_>,
) -> Result<(), String> {
    let DownloadContext { config, http_client, index_writer } = *downloads;
    let package_id = tie.package_id();
    let signed_integrity: Integrity = signed
        .parse()
        .map_err(|_| format!("{package_id} has an unparsable published integrity"))?;
    let resolved = FetchTarballForResolution {
        http_client,
        store: ResolutionStore {
            dir: &config.store_dir,
            index_writer: Some(std::sync::Arc::clone(index_writer)),
        },
        package: TarballPackage {
            integrity_kind: IntegrityKind::Resource,
            integrity: Some(&signed_integrity),
            unpacked_size: None,
            file_count: None,
            url: tarball,
            id: &package_id,
        },
        auth_headers: &config.auth_headers,
        retry_opts: config.retry_opts(),
        tar_limits: config.tar_limits(),
        manifest_subdir: None,
        revision_addressed: false,
    }
    .run::<SilentReporter>(None)
    .await
    .map_err(|error| {
        format!("{package_id} could not be downloaded to tie its tar integrity: {error}")
    })?;
    match resolved.tar_integrity {
        Some(tar) if tar.to_string() == tie.lockfile_integrity => Ok(()),
        _ => Err(format!(
            "{package_id} has a lockfile tar integrity that does not match the published archive",
        )),
    }
}
