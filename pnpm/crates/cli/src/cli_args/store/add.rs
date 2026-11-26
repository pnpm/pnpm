//! `pacquet store add`: fetch packages into the store without installing
//! them anywhere.

use crate::cli_args::registry_client::build_registry_client;
use derive_more::{
    Display,
    Error,
};
use futures_util::StreamExt;
use miette::{
    Diagnostic,
    WrapErr,
};
use pnpm_config::Config;
use pnpm_deps_restorer::{
    manifest_file_count,
    manifest_unpacked_size,
};
use pnpm_lockfile::LockfileResolution;
use pnpm_network::{
    ThrottledClient,
    has_http_scheme,
    redact_and_sanitize,
    redact_specifier_for_display,
};
use pnpm_reporter::{
    GlobalLog,
    LogEvent,
    LogLevel,
    Reporter,
    TarballWarningLog,
    emit_global_warning,
};
use pnpm_resolving_default_resolver::standalone::{
    StandaloneChainOptions,
    build_standalone_chain,
};
use pnpm_resolving_local_resolver::LocalTarballResolution;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_resolving_resolver_base::{
    ResolveOptions,
    ResolveResult,
    WantedDependency,
};
use pnpm_store_dir::{
    SharedVerifiedFilesCache,
    StoreIndex,
    StoreIndexWriter,
};
use pnpm_tarball::IngestTarballToStore;
use ssri::Integrity;
use std::{
    path::Path,
    sync::Arc,
};
use tokio::sync::OnceCell;

mod downloaded_tarball;
mod lockfile_entries;

/// At least one specifier could not be added.
///
/// Each failure is reported as it happens, the way pnpm logs them, so this
/// only has to say the command as a whole did not succeed.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Some packages have not been added correctly")]
#[diagnostic(code(ERR_PNPM_STORE_ADD_FAILURE))]
pub struct StoreAddFailureError;

/// `store add` exists to write to the store, which `frozenStore` opens
/// read-only.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot add packages to the store because frozenStore is enabled (the store is opened read-only)"
)]
#[diagnostic(
    code(ERR_PNPM_FROZEN_STORE_WRITE),
    help("Turn off frozenStore to add packages to the store.")
)]
pub struct FrozenStoreAddError;

/// A specifier that resolved to something this command cannot put in the
/// store.
///
/// The resolver chain claims every protocol pnpm supports, but warming the
/// store means extracting an archive into it, and a git checkout or a
/// local directory does not name one. pacquet has fetchers for those on
/// the install path; routing them through here needs a package name the
/// standalone chain does not resolve, so they are refused by name instead
/// of failing further down as a shape mismatch.
///
/// The code is pacquet's own because pnpm has none to match: pnpm's
/// `storeAdd` fetches these successfully. That makes this the same kind of
/// marker as `ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED`, a gap that
/// fails loudly and by name rather than one that quietly does something
/// other than what was asked.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot add \"{package}\" to the store: a {resolved_via} dependency has no archive to fetch"
)]
#[diagnostic(
    code(ERR_PNPM_STORE_ADD_UNSUPPORTED_SPEC),
    help(
        "`pnpm store add` takes registry and tarball specifiers, e.g. `pnpm store add express@4`."
    )
)]
pub struct UnsupportedSpecError {
    pub package: String,
    #[error(not(source))]
    pub resolved_via: String,
}

/// The archive a resolution points at, if it points at one.
///
/// A tarball published without an integrity hash is still fetchable
/// (pnpm fetches those unverified) so the hash is optional.
fn archive_to_fetch(resolution: &LockfileResolution) -> Option<(&str, Option<Integrity>)> {
    match resolution {
        LockfileResolution::Tarball(tarball) => {
            Some((tarball.tarball.as_str(), tarball.integrity.clone()))
        }
        // A registry resolution records only the hash; the URL that goes
        // with it is the lockfile's, derived from the configured
        // registry. The npm resolver hands this path a `Tarball`.
        LockfileResolution::Registry(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => None,
    }
}

pub(super) async fn run<Reporter: self::Reporter>(
    config: &'static Config,
    dir: &Path,
    packages: &[String],
) -> miette::Result<()> {
    if config.frozen_store {
        return Err(FrozenStoreAddError.into());
    }
    if packages.is_empty() {
        return Ok(());
    }
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Debug,
        message: "Waiting for the store add operation lock".to_string(),
    }));
    let _store_lock =
        config.store_dir.lock_for_use().wrap_err("lock the store while adding packages")?;
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Debug,
        message: "Acquired the store add operation lock".to_string(),
    }));

    add_packages::<Reporter>(config, dir, packages).await
}

async fn add_packages<Reporter: self::Reporter>(
    config: &'static Config,
    dir: &Path,
    packages: &[String],
) -> miette::Result<()> {
    let packages = downloaded_tarball::expand_tarball_directories(dir, packages)?;
    let http_client = Arc::new(build_registry_client(config)?);
    let resolver = store_resolver(config, &http_client)?;
    let resolve_options = store_resolve_options(dir);
    let (store_index_writer, writer_task) =
        StoreIndexWriter::spawn_checked(&config.store_dir, config.store_index_format);
    let registry_prefixes: Vec<String> = config
        .merged_registries_by_prefix()
        .into_keys()
        .collect();
    let requester = dir.display().to_string();
    let adder = AddOne {
        config,
        http_client: &http_client,
        resolver: &resolver,
        resolve_options: &resolve_options,
        requester: &requester,
        lockfile_entries: OnceCell::new(),
        store: StoreAddIndex {
            index: StoreIndex::open_shared(&config.store_dir, false).await,
            index_writer: &store_index_writer,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            registry_prefixes: &registry_prefixes,
        },
    };
    let outcome = adder.add_all::<Reporter>(packages).await;

    drop(store_index_writer);
    writer_task.await
        .map_err(|error| miette::miette!("wait for store-index writes: {error}"))?
        .wrap_err("write store index")?;

    for package_id in &outcome.added {
        Reporter::emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Info,
            message: format!("+ {package_id}"),
        }));
    }
    if outcome.has_failures { Err(StoreAddFailureError.into()) } else { Ok(()) }
}

/// Specifiers [`AddOne::add_all`] fetched. Their `+ <id>` lines are printed
/// only after the store index write succeeds.
#[derive(Default)]
struct AddAllOutcome {
    added: Vec<String>,
    has_failures: bool,
}

fn store_resolve_options(dir: &Path) -> ResolveOptions {
    ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: dir.to_path_buf(),
            lockfile_dir: dir.to_path_buf(),
            ..Default::default()
        },
        ..ResolveOptions::default()
    }
}

/// Report a failed specifier.
///
/// The command keeps going so one bad specifier doesn't strand
/// the rest, which leaves this line as the only place the cause
/// is reported, so it carries the code the top-level handler
/// would otherwise have printed.
fn report_add_failure<Reporter: self::Reporter>(error: &miette::Report, tarball: Option<&str>) {
    let code = error
        .code()
        .map_or_else(String::new, |code| format!("{code}: "));
    if let Some(tarball) = tarball {
        let display = redact_specifier_for_display(tarball);
        let cause = redact_and_sanitize(&error.to_string().replace(tarball, &display));
        Reporter::emit(&LogEvent::TarballWarning(TarballWarningLog {
            log: GlobalLog { level: LogLevel::Warn, message: format!("{display}: {code}{cause}") },
            tarball: display,
        }));
    } else {
        emit_global_warning::<Reporter>(&format!("{code}{error}"));
    }
}

/// Everything one specifier's resolve-and-fetch needs. Grouped so the
/// per-package step keeps to the repository's argument-count convention.
struct AddOne<'a> {
    config: &'static Config,
    http_client: &'a Arc<ThrottledClient>,
    resolver: &'a pnpm_resolving_default_resolver::DefaultResolver,
    resolve_options: &'a ResolveOptions,
    requester: &'a str,
    /// Read when the first tarball on disk is added, so adding packages by
    /// name does not depend on the project's lockfile.
    lockfile_entries: OnceCell<lockfile_entries::LockfileEntries>,
    store: StoreAddIndex<'a>,
}

pub(crate) struct StoreAddIndex<'a> {
    index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
    index_writer: &'a Arc<StoreIndexWriter>,
    verified_files_cache: SharedVerifiedFilesCache,
    registry_prefixes: &'a [String],
}

fn store_wanted_dependency(package: &str) -> WantedDependency {
    let parsed = parse_wanted_dependency(package);
    WantedDependency {
        alias: parsed.alias,
        bare_specifier: parsed.bare_specifier.filter(|spec| !spec.trim().is_empty()),
        injected: None,
        prev_specifier: None,
        optional: None,
    }
}

fn store_resolver(
    config: &'static Config,
    http_client: &Arc<ThrottledClient>,
) -> miette::Result<pnpm_resolving_default_resolver::DefaultResolver> {
    build_standalone_chain(&StandaloneChainOptions {
        config,
        http_client,
        // The store only needs the tarball URL and its integrity, both of
        // which the abbreviated document carries.
        full_metadata: false,
        filter_metadata: false,
        local_tarball_resolution: LocalTarballResolution::DeferToFetcher,
    })
    .map_err(miette::Report::new)
}

/// The archive a specifier resolved to, as the store indexes it.
#[derive(Clone, Copy)]
struct ArchiveToAdd<'r> {
    url: &'r str,
    id: &'r str,
    manifest: Option<&'r serde_json::Value>,
}

impl<'a> AddOne<'a> {
    async fn add_all<Reporter: self::Reporter>(&self, packages: Vec<String>) -> AddAllOutcome {
        futures_util::stream::iter(packages)
            .map(|package| async move { self.add_one::<Reporter>(&package).await })
            .buffer_unordered(self.config.network_concurrency.max(1))
            .fold(AddAllOutcome::default(), |mut all, outcome| {
                match outcome {
                    Some(package_id) => all.added.push(package_id),
                    None => all.has_failures = true,
                }
                std::future::ready(all)
            })
            .await
    }

    /// Resolve one specifier and pull its tarball into the store, returning
    /// the package id pnpm reports it under.
    async fn add_one<Reporter: self::Reporter>(&self, package: &str) -> Option<String> {
        let resolved = self
            .resolve(package)
            .await
            .inspect_err(|error| {
                let tarball =
                    has_http_scheme(package) || pnpm_local_spec::is_tarball_filename(package);
                if tarball {
                    report_add_failure::<Reporter>(error, Some(package));
                } else {
                    emit_global_warning::<Reporter>(&format!(
                        "{}: {error}",
                        redact_specifier_for_display(package),
                    ));
                }
            })
            .ok()?;
        let package_id = resolved.id.to_string();
        let Some((package_url, integrity)) = archive_to_fetch(&resolved.resolution) else {
            let error = UnsupportedSpecError {
                package: package.to_owned(),
                resolved_via: resolved.resolved_via,
            };
            report_add_failure::<Reporter>(&error.into(), None);
            return None;
        };

        let archive = ArchiveToAdd {
            url: package_url,
            id: &package_id,
            manifest: resolved.package.manifest.as_deref(),
        };
        self.fetch_archive::<Reporter>(&archive, &resolved.resolved_via, integrity.as_ref())
            .await
            .inspect_err(|error| report_add_failure::<Reporter>(error, Some(package_url)))
            .ok()?;
        Some(package_id)
    }

    async fn fetch_archive<Reporter: self::Reporter>(
        &self,
        archive: &ArchiveToAdd<'_>,
        resolved_via: &str,
        integrity: Option<&Integrity>,
    ) -> miette::Result<()> {
        let lockfile_dir = &self.resolve_options.project.lockfile_dir;
        if let Some(path) =
            downloaded_tarball::tarball_on_disk(resolved_via, archive.url, lockfile_dir)
        {
            self.add_downloaded_tarball::<Reporter>(&path, archive).await
        } else {
            let ingestion = self.ingestion(archive, integrity);
            if integrity.is_some() {
                ingestion
                    .run_without_mem_cache::<Reporter>()
                    .await
                    .map(|_| ())
            } else {
                ingestion
                    .fetch_and_extract::<Reporter>()
                    .await
                    .map(|_| ())
            }
            .map_err(miette::Report::new)
        }
    }

    fn ingestion<'r>(
        &'r self,
        archive: &ArchiveToAdd<'r>,
        integrity: Option<&'r Integrity>,
    ) -> IngestTarballToStore<'r> {
        IngestTarballToStore {
            fetching: self.archive_options(),
            package: pnpm_tarball::TarballPackage {
                integrity_kind: pnpm_tarball::IntegrityKind::Resource,
                integrity,
                unpacked_size: manifest_unpacked_size(archive.manifest),
                file_count: manifest_file_count(archive.manifest),
                url: archive.url,
                id: archive.id,
            },
            store: pnpm_tarball::ArchiveStoreContext {
                dir: std::borrow::Cow::Borrowed(&self.config.store_dir),
                index: self.store.index.clone(),
                index_writer: Some(Arc::clone(self.store.index_writer)),
                verify_integrity: self.config.verify_store_integrity,
                strict_pkg_content_check: self.config.strict_store_pkg_content_check,
                verified_files_cache: SharedVerifiedFilesCache::clone(
                    &self.store.verified_files_cache,
                ),
                prefetched_cas_paths: None,
            },

            requester: self.requester,

            ignore_file_pattern: None,

            progress_reported: None,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: None,
            },
        }
    }

    fn archive_options(&self) -> pnpm_tarball::ArchiveFetchOptions<'a> {
        pnpm_tarball::ArchiveFetchOptions {
            http_client: self.http_client,
            auth_headers: &self.config.auth_headers,
            retry_opts: self.config.retry_opts(),
            offline: self.config.offline,
            tar_limits: pnpm_tarball::TarLimits::new(self.config.max_tarball_unpacked_size),
        }
    }

    async fn resolve(&self, package: &str) -> miette::Result<ResolveResult> {
        let wanted = store_wanted_dependency(package);
        self.resolver
            .resolve(&wanted, self.resolve_options)
            .await
            .map_err(|error| miette::miette!("{error}"))
    }
}
