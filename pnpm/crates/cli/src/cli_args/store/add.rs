//! `pacquet store add` — fetch packages into the store without installing
//! them anywhere.

use crate::cli_args::registry_client::build_registry_client;
use derive_more::{Display, Error};
use miette::{Diagnostic, WrapErr};
use pnpm_config::Config;
use pnpm_deps_restorer::{manifest_file_count, manifest_unpacked_size};
use pnpm_lockfile::LockfileResolution;
use pnpm_network::ThrottledClient;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter, emit_global_warning};
use pnpm_resolving_default_resolver::standalone::{StandaloneChainOptions, build_standalone_chain};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_resolving_resolver_base::{ResolveOptions, ResolveResult, WantedDependency};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter};
use pnpm_tarball::IngestTarballToStore;
use ssri::Integrity;
use std::{path::Path, sync::Arc};

/// At least one specifier could not be added.
///
/// Each failure is reported as it happens, the way pnpm logs them, so this
/// only has to say the command as a whole did not succeed.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Some packages have not been added correctly")]
#[diagnostic(code(ERR_PNPM_STORE_ADD_FAILURE))]
pub struct StoreAddFailureError;

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
/// marker as `ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED` — a gap that
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
/// A tarball published without an integrity hash is still fetchable —
/// pnpm fetches those unverified — so the hash is optional.
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
    if packages.is_empty() {
        return Ok(());
    }
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Debug,
        message: "Waiting for the store add operation lock".to_string(),
    }));
    let _store_lock = if config.frozen_store {
        config.store_dir.lock_for_frozen_use()
    } else {
        config.store_dir.lock_for_use()
    }
    .wrap_err("lock the store while adding packages")?;
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
    let http_client = Arc::new(build_registry_client(config)?);
    let resolver = store_resolver(config, &http_client)?;
    let resolve_options = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: dir.to_path_buf(),
            lockfile_dir: dir.to_path_buf(),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    // Both halves honour `frozenStore`, so a read-only store root gains
    // neither an `index.db` write nor its WAL / SHM sidecars.
    let store_index = StoreIndex::open_shared(&config.store_dir, config.frozen_store).await;
    let (store_index_writer, writer_task) =
        StoreIndexWriter::spawn_for(&config.store_dir, config.frozen_store);
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let requester = dir.display().to_string();

    let mut has_failures = false;
    for package in packages {
        has_failures |= report_add_outcome::<Reporter>(
            add_one::<Reporter>(AddOne {
                config,
                http_client: &http_client,
                resolver: &resolver,
                resolve_options: &resolve_options,
                requester: &requester,
                package,
                store: StoreAddIndex {
                    index: store_index.clone(),
                    index_writer: &store_index_writer,
                    verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
                },
            })
            .await,
        );
    }

    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some rows may not be persisted").await;

    if has_failures { Err(StoreAddFailureError.into()) } else { Ok(()) }
}

/// Report one specifier's outcome; `true` when it failed.
///
/// The command keeps going so one bad specifier doesn't strand
/// the rest, which leaves this line as the only place the cause
/// is reported — so it carries the code the top-level handler
/// would otherwise have printed.
fn report_add_outcome<Reporter: self::Reporter>(outcome: miette::Result<String>) -> bool {
    match outcome {
        Ok(package_id) => {
            Reporter::emit(&LogEvent::Global(GlobalLog {
                level: LogLevel::Info,
                message: format!("+ {package_id}"),
            }));
            false
        }
        Err(error) => {
            let code = error
                .code()
                .map_or_else(String::new, |code| format!("{code}: "));
            emit_global_warning::<Reporter>(&format!("{code}{error}"));
            true
        }
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
    package: &'a str,
    store: StoreAddIndex<'a>,
}

pub(crate) struct StoreAddIndex<'a> {
    index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
    index_writer: &'a Arc<StoreIndexWriter>,
    verified_files_cache: SharedVerifiedFilesCache,
}

/// Resolve one specifier and pull its tarball into the store, returning
/// the package id pnpm reports it under.
async fn add_one<Reporter: self::Reporter>(args: AddOne<'_>) -> miette::Result<String> {
    let resolved = args.resolve().await?;
    let package_id = resolved.id.to_string();
    let Some((package_url, integrity)) = archive_to_fetch(&resolved.resolution) else {
        return Err(UnsupportedSpecError {
            package: args.package.to_owned(),
            resolved_via: resolved.resolved_via,
        }
        .into());
    };

    IngestTarballToStore {
        fetching: args.archive_options(),
        package: pnpm_tarball::TarballPackage {
            integrity: integrity.as_ref(),
            unpacked_size: manifest_unpacked_size(resolved.package.manifest.as_deref()),
            file_count: manifest_file_count(resolved.package.manifest.as_deref()),
            url: package_url,
            id: &package_id,
        },
        store: pnpm_tarball::ArchiveStoreContext {
            dir: &args.config.store_dir,
            index: args.store.index,
            index_writer: Some(Arc::clone(args.store.index_writer)),
            verify_integrity: args.config.verify_store_integrity,
            strict_pkg_content_check: args.config.strict_store_pkg_content_check,
            verified_files_cache: args.store.verified_files_cache,
            prefetched_cas_paths: None,
            fallback_dir: args.config.fallback_store(),
        },

        requester: args.requester,

        ignore_file_pattern: None,

        progress_reported: None,
        store_projection: pnpm_tarball::ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<Reporter>()
    .await
    .map_err(miette::Report::new)?;

    Ok(package_id)
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
    })
    .map_err(miette::Report::new)
}

impl<'a> AddOne<'a> {
    fn archive_options(&self) -> pnpm_tarball::ArchiveFetchOptions<'a> {
        pnpm_tarball::ArchiveFetchOptions {
            http_client: self.http_client,
            auth_headers: &self.config.auth_headers,
            retry_opts: self.config.retry_opts(),
            offline: self.config.offline,
        }
    }

    async fn resolve(&self) -> miette::Result<ResolveResult> {
        let wanted = store_wanted_dependency(self.package);
        self.resolver
            .resolve(&wanted, self.resolve_options)
            .await
            .map_err(|error| miette::miette!("{}: {error}", self.package))
    }
}
