//! `pack <spec>`: pack a package spec instead of the project in the
//! working directory. A spec names a registry package
//! (`name[@version|tag|range]`), a tarball URL, a local directory, or a
//! local tarball file. Git specs are rejected: packing one would need the
//! git fetcher's clone-and-prepare pipeline, which `pack` does not drive.

use super::{
    PackArgs,
    PackSharedContext,
    set_injected_changelog,
};
use crate::cli_args::{
    catalogs::configured_catalogs,
    registry_client::build_registry_client,
};
use derive_more::{
    Display,
    Error,
};
use miette::{
    Context,
    Diagnostic,
    IntoDiagnostic,
};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{
    FetchedPackOptions,
    Host,
    PackResult,
    api,
    format_pack_output,
    pack_fetched_tarball,
    to_pack_result_json,
};
use pnpm_reporter::Reporter;
use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

#[cfg(not(target_family = "wasm"))]
use home::home_dir;
#[cfg(target_family = "wasm")]
use pnpm_fs::home_dir;

/// Failures specific to the positional `pack <spec>` form. pnpm defines no
/// codes for these rejections; the codes are new.
#[derive(Debug, Display, Error, Diagnostic)]
enum PackSpecError {
    #[display("Cannot use --recursive with a package spec")]
    #[diagnostic(code(ERR_PNPM_INVALID_OPTION))]
    RecursiveWithSpec,

    #[display("Cannot pack \"{spec}\": {reason}")]
    #[diagnostic(code(ERR_PNPM_PACK_UNSUPPORTED_SPEC))]
    UnsupportedSpec {
        #[error(not(source))]
        spec: String,
        #[error(not(source))]
        reason: &'static str,
    },

    #[display("Cannot pack \"{spec}\": no file or directory at {path}")]
    #[diagnostic(code(ERR_PNPM_PACK_SPEC_NOT_FOUND))]
    NotFound {
        #[error(not(source))]
        spec: String,
        #[error(not(source))]
        path: String,
    },
}

pub(super) async fn run_specs<Reporter: self::Reporter>(
    args: &PackArgs,
    dir: &Path,
    config: &Config,
    recursive: bool,
    before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
) -> miette::Result<String> {
    if recursive {
        return Err(PackSpecError::RecursiveWithSpec.into());
    }
    let mut results = Vec::new();
    for spec in &args.packages {
        results.push(to_pack_result_json(
            &pack_spec::<Reporter>(args, dir, config, &before_packing_hooks, spec).await?,
        ));
    }
    Ok(format_pack_output(&results, args.json, false))
}

async fn pack_spec<Reporter: self::Reporter>(
    args: &PackArgs,
    dir: &Path,
    config: &Config,
    before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    spec: &str,
) -> miette::Result<PackResult> {
    match classify(spec, dir)? {
        PackSpec::LocalDir(path) => {
            pack_local_dir::<Reporter>(args, dir, config, before_packing_hooks, path).await
        }
        PackSpec::LocalTarball(path) => {
            let bytes = std::fs::read(&path)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()))?;
            Ok(pack_fetched_tarball::<Host>(&fetched_options(args, dir, spec, &bytes, None))?)
        }
        PackSpec::RemoteTarballUrl(url) => {
            let bytes = download_tarball(config, &url).await?;
            Ok(pack_fetched_tarball::<Host>(&fetched_options(
                args,
                dir,
                &redact_spec(&url),
                &bytes,
                None,
            ))?)
        }
        PackSpec::Registry => pack_registry_spec(args, dir, config, spec).await,
    }
}

/// A spec as errors may print it: credentials in a URL spec are redacted.
fn redact_spec(spec: &str) -> String {
    if spec.starts_with("http:") || spec.starts_with("https:") {
        pnpm_network::redact_url_credentials(spec)
    } else {
        spec.to_string()
    }
}

fn fetched_options<'a>(
    args: &'a PackArgs,
    dir: &'a Path,
    source: &'a str,
    bytes: &'a [u8],
    expected_integrity: Option<&'a ssri::Integrity>,
) -> FetchedPackOptions<'a> {
    FetchedPackOptions {
        bytes,
        source,
        expected_integrity,
        base_dir: dir,
        dry_run: args.dry_run,
        out: args.out.as_deref(),
        destination: args.pack_destination.as_deref(),
    }
}

/// Pack a local directory spec through the same pipeline as a bare
/// `pnpm pack`, with the output anchored at the CLI directory the way
/// `npm pack ./dir` writes to the working directory.
async fn pack_local_dir<Reporter: self::Reporter>(
    args: &PackArgs,
    dir: &Path,
    config: &Config,
    before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    path: PathBuf,
) -> miette::Result<PackResult> {
    // A directory outside the current workspace is packed without the
    // workspace's context: its catalogs, workspace package versions,
    // composed changelog, and root LICENSE belong to the calling project,
    // not to the packed one.
    let in_workspace = config.workspace_dir
        .as_deref()
        .is_some_and(|workspace_dir| path.starts_with(workspace_dir));
    let shared = PackSharedContext {
        catalogs: if in_workspace { configured_catalogs(config)? } else { Default::default() },
        output_base_dir: Some(dir.to_path_buf()),
        out: args.out.clone(),
        pack_destination: args.pack_destination.clone(),
        before_packing_hooks: before_packing_hooks.to_vec(),
        workspace_packages: None,
    };
    let mut options = args.pack_options(path.clone(), config, shared);
    if in_workspace {
        set_injected_changelog(&mut options, config, &path).await?;
    } else {
        options.workspace_dir = None;
        options.manifest.catalogs_dir = None;
    }
    Ok(api::<Reporter, Host>(&options).await?)
}

async fn pack_registry_spec(
    args: &PackArgs,
    dir: &Path,
    config: &Config,
    spec: &str,
) -> miette::Result<PackResult> {
    let fetched = crate::cli_args::view::fetch_package_metadata(config, None, spec, "pack").await?;
    let dist = &fetched.picked.dist;
    if dist.tarball.is_empty() {
        return Err(PackSpecError::UnsupportedSpec {
            spec: redact_spec(spec),
            reason: "the registry metadata names no tarball URL",
        }
        .into());
    }
    let bytes = download_tarball(config, &dist.tarball).await?;
    Ok(pack_fetched_tarball::<Host>(&fetched_options(
        args,
        dir,
        spec,
        &bytes,
        dist.integrity.as_ref(),
    ))?)
}

async fn download_tarball(config: &Config, url: &str) -> miette::Result<Vec<u8>> {
    let client = build_registry_client(config)?;
    let response = client
        .get_bytes_with_secure_auth_and_retry(url, &config.auth_headers, None, config.retry_opts())
        .await
        .map_err(|error| pnpm_tarball::NetworkError::new(url, error))?;
    if !response.status.is_success() {
        return Err(pnpm_tarball::HttpStatusError {
            url: pnpm_network::redact_url_credentials(url),
            status: response.status.as_u16(),
        }
        .into());
    }
    Ok(response.body)
}

/// What a positional spec points at, classified in the resolver chain's
/// probe order: git, remote tarball URL, local filesystem, registry.
enum PackSpec {
    LocalDir(PathBuf),
    LocalTarball(PathBuf),
    RemoteTarballUrl(String),
    Registry,
}

fn classify(spec: &str, dir: &Path) -> Result<PackSpec, PackSpecError> {
    if pnpm_resolving_git_resolver::parse_bare_specifier(spec).is_some() {
        return Err(PackSpecError::UnsupportedSpec {
            spec: redact_spec(spec),
            reason: "packing a git dependency is not supported yet",
        });
    }
    if let Some(id) = pnpm_resolving_tarball_resolver::remote_tarball_id(spec) {
        return id
            .map(PackSpec::RemoteTarballUrl)
            .map_err(|_| PackSpecError::UnsupportedSpec {
                spec: redact_spec(spec),
                reason: "invalid tarball URL",
            });
    }
    if pnpm_local_spec::is_local_filesystem_specifier(spec) {
        let path = local_spec_path(spec, dir);
        return match (path.is_dir(), path.is_file()) {
            (true, _) => Ok(PackSpec::LocalDir(path)),
            (false, true) => Ok(PackSpec::LocalTarball(path)),
            (false, false) => Err(PackSpecError::NotFound {
                spec: redact_spec(spec),
                path: path.display().to_string(),
            }),
        };
    }
    Ok(PackSpec::Registry)
}

/// The filesystem path a `file:` / `link:` / path-shaped spec names,
/// resolved against the CLI directory and lexically normalized, so a
/// workspace-membership check can compare prefixes.
fn local_spec_path(spec: &str, dir: &Path) -> PathBuf {
    let path = spec
        .strip_prefix("file:")
        .or_else(|| spec.strip_prefix("link:"))
        .unwrap_or(spec);
    if let Some(home_relative) = path.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        return pnpm_fs::lexical_normalize(&home.join(home_relative));
    }
    pnpm_fs::lexical_normalize(&dir.join(path))
}
