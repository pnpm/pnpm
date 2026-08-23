//! `pacquet store add` — fetch packages into the store without installing
//! them anywhere.

use crate::cli_args::registry_client::build_registry_client;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_deps_restorer::{manifest_file_count, manifest_unpacked_size};
use pnpm_lockfile::LockfileResolution;
use pnpm_network::ThrottledClient;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter, emit_global_warning};
use pnpm_resolving_default_resolver::standalone::{StandaloneChainOptions, build_standalone_chain};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_resolving_resolver_base::{ResolveOptions, WantedDependency};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter};
use pnpm_tarball::DownloadTarballToStore;
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
    let http_client = Arc::new(build_registry_client(config)?);
    let resolver = build_standalone_chain(&StandaloneChainOptions {
        config,
        http_client: &http_client,
        // The store only needs the tarball URL and its integrity, both of
        // which the abbreviated document carries.
        full_metadata: false,
        filter_metadata: false,
    })
    .map_err(miette::Report::new)?;
    let resolve_options = ResolveOptions {
        project_dir: dir.to_path_buf(),
        lockfile_dir: dir.to_path_buf(),
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
        let outcome = add_one::<Reporter>(AddOne {
            config,
            http_client: &http_client,
            resolver: &resolver,
            resolve_options: &resolve_options,
            store_index: store_index.clone(),
            store_index_writer: &store_index_writer,
            verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
            requester: &requester,
            package,
        })
        .await;
        match outcome {
            Ok(package_id) => {
                Reporter::emit(&LogEvent::Global(GlobalLog {
                    level: LogLevel::Info,
                    message: format!("+ {package_id}"),
                }));
            }
            // The command keeps going so one bad specifier doesn't strand
            // the rest, which leaves this line as the only place the cause
            // is reported — so it carries the code the top-level handler
            // would otherwise have printed.
            Err(error) => {
                has_failures = true;
                let code = error.code().map_or_else(String::new, |code| format!("{code}: "));
                emit_global_warning::<Reporter>(&format!("{code}{error}"));
            }
        }
    }

    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some rows may not be persisted").await;

    if has_failures { Err(StoreAddFailureError.into()) } else { Ok(()) }
}

/// Everything one specifier's resolve-and-fetch needs. Grouped so the
/// per-package step keeps to the repository's argument-count convention.
struct AddOne<'a> {
    config: &'static Config,
    http_client: &'a Arc<ThrottledClient>,
    resolver: &'a pnpm_resolving_default_resolver::DefaultResolver,
    resolve_options: &'a ResolveOptions,
    store_index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
    store_index_writer: &'a Arc<StoreIndexWriter>,
    verified_files_cache: SharedVerifiedFilesCache,
    requester: &'a str,
    package: &'a str,
}

/// Resolve one specifier and pull its tarball into the store, returning
/// the package id pnpm reports it under.
async fn add_one<Reporter: self::Reporter>(args: AddOne<'_>) -> miette::Result<String> {
    let AddOne {
        config,
        http_client,
        resolver,
        resolve_options,
        store_index,
        store_index_writer,
        verified_files_cache,
        requester,
        package,
    } = args;
    let parsed = parse_wanted_dependency(package);
    let wanted_dependency = WantedDependency {
        alias: parsed.alias,
        bare_specifier: parsed.bare_specifier.filter(|spec| !spec.trim().is_empty()),
        injected: None,
        prev_specifier: None,
        optional: None,
    };
    let resolved = resolver
        .resolve(&wanted_dependency, resolve_options)
        .await
        .map_err(|error| miette::miette!("{package}: {error}"))?;
    let package_id = resolved.id.to_string();
    let Some((package_url, integrity)) = archive_to_fetch(&resolved.resolution) else {
        return Err(UnsupportedSpecError {
            package: package.to_owned(),
            resolved_via: resolved.resolved_via,
        }
        .into());
    };

    DownloadTarballToStore {
        http_client,
        store_dir: &config.store_dir,
        store_index,
        store_index_writer: Some(Arc::clone(store_index_writer)),
        verify_store_integrity: config.verify_store_integrity,
        strict_store_pkg_content_check: config.strict_store_pkg_content_check,
        verified_files_cache,
        package_integrity: integrity.as_ref(),
        package_unpacked_size: manifest_unpacked_size(resolved.manifest.as_deref()),
        package_file_count: manifest_file_count(resolved.manifest.as_deref()),
        package_url,
        package_id: &package_id,
        auth_headers: &config.auth_headers,
        requester,
        prefetched_cas_paths: None,
        retry_opts: pnpm_network::RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: std::time::Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: std::time::Duration::from_millis(config.fetch_retry_maxtimeout),
        },
        ignore_file_pattern: None,
        offline: config.offline,
        progress_reported: None,
        append_manifest: None,
    }
    .run_without_mem_cache::<Reporter>()
    .await
    .map_err(miette::Report::new)?;

    Ok(package_id)
}
