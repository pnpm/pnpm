//! Tying a lockfile's tar integrity to the `.tgz` integrity a registry
//! signature covers.

use super::{Config, HashMap, Packument, SignaturePackage, ThrottledClient};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{StoreIndex, StoreIndexWriter, store_index_key};
use pnpm_tarball::{FetchTarballForResolution, IntegrityKind, ResolutionStore, TarballPackage};
use ssri::Integrity;

/// The integrity a registry signature covers, for every package whose
/// lockfile entry pins its tar, keyed by `(registry, name, version)`.
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
) -> HashMap<(String, String, String), Result<String, String>> {
    let pending: Vec<PendingTie<'_>> = packages
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
                .and_then(|version| version.dist.as_ref());
            // A version without signatures is reported as missing whatever
            // its tar hash is tied to.
            dist.and_then(|dist| dist.signatures.as_ref())
                .and_then(serde_json::Value::as_array)
                .filter(|signatures| !signatures.is_empty())?;
            Some(PendingTie {
                pkg,
                lockfile_integrity: pkg.integrity.as_deref()?,
                published: dist.and_then(|dist| {
                    dist.integrity.as_deref().zip(dist.tarball.as_deref())
                }),
            })
        })
        .collect();
    let stored = stored_tar_integrities(&pending, config);
    let (index_writer, writer_task) = StoreIndexWriter::spawn_for(
        &config.store_dir,
        config.frozen_store,
        config.store_index_format,
    );
    let ties = pending
        .iter()
        .map(|tie| {
            let stored = &stored;
            let index_writer = &index_writer;
            async move {
                let outcome = match tie.published {
                    None => Err(format!(
                        "{}@{} has no published integrity to tie its tar integrity to",
                        tie.pkg.name, tie.pkg.version,
                    )),
                    Some((signed, _)) if signed == tie.lockfile_integrity => Ok(signed.to_string()),
                    Some((signed, _))
                        if stored
                            .get(&tie.package_id())
                            .map(String::as_str)
                            == Some(tie.lockfile_integrity) =>
                    {
                        Ok(signed.to_string())
                    }
                    Some((signed, tarball)) => {
                        download_tie(tie, signed, tarball, config, http_client, index_writer)
                            .await
                            .map(|()| signed.to_string())
                    }
                };
                ((tie.pkg.registry.clone(), tie.pkg.name.clone(), tie.pkg.version.clone()), outcome)
            }
        });
    let ties = futures_util::future::join_all(ties).await.into_iter().collect();
    drop(index_writer);
    StoreIndexWriter::drain(writer_task, " after tying tar integrities").await;
    ties
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
}

/// The tar integrity the store row of each pending package's published
/// archive records, by `name@version`.
fn stored_tar_integrities(pending: &[PendingTie<'_>], config: &Config) -> HashMap<String, String> {
    let Ok(index) = StoreIndex::open_readonly_in(&config.store_dir) else { return HashMap::new() };
    pending
        .iter()
        .filter_map(|tie| {
            let (signed, _) = tie.published?;
            let package_id = tie.package_id();
            let row = index
                .get(&store_index_key(signed, &package_id))
                .ok()
                .flatten()?;
            Some((package_id, row.tar_integrity?))
        })
        .collect()
}

async fn download_tie(
    tie: &PendingTie<'_>,
    signed: &str,
    tarball: &str,
    config: &'static Config,
    http_client: &ThrottledClient,
    index_writer: &std::sync::Arc<StoreIndexWriter>,
) -> Result<(), String> {
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
